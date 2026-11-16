//! `complete-word`: orchestrate the target shell to probe candidates.

use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eyre::{Result, WrapErr, bail};

use crate::config::Config;
use crate::registry::Registry;
use crate::source;

const HARNESS: &str = include_str!("shell/harness.zsh");

#[derive(Debug, Clone)]
pub struct Request {
    pub tool: String,
    pub version: Option<String>,
    pub words: Vec<String>,
    pub cword: Option<usize>,
    pub shell: String,
}

/// Split a trailing `@version`, leaving SSH references alone.
pub fn split_tool_version(input: &str) -> (String, Option<String>) {
    if let Some((base, version)) = input.rsplit_once('@')
        && !input.starts_with("git@")
        && !version.is_empty()
        && !version.contains('/')
        && !base.is_empty()
    {
        return (base.to_string(), Some(version.to_string()));
    }
    (input.to_string(), None)
}

pub fn complete(config: &Config, registry: &Registry, request: &Request) -> Result<Vec<String>> {
    if request.shell != "zsh" {
        bail!("only zsh is supported in v0.0.1");
    }
    let resolved = registry
        .resolve_str(&request.tool)
        .wrap_err_with(|| format!("cannot complete `{}`", request.tool))?;
    let command = resolved.tool.name.clone();
    let bin = source::resolve_binary(&command)?;
    if let Some(expected) = &request.version
        && let Some(actual) = source::version_for_tool(&resolved.tool, &bin)
        && &actual != expected
    {
        bail!("`{command}` resolves to version {actual}, not {expected}");
    }

    let script = crate::cache::load_or_generate(config, registry, &command, "zsh", &bin)?;

    let mut words = if request.words.is_empty() {
        vec![command.clone(), String::new()]
    } else {
        request.words.clone()
    };
    // The harness completes the canonical command, not a shell alias.
    if let Some(first) = words.first_mut() {
        *first = command.clone();
    }
    let cword = request.cword.unwrap_or(words.len());
    if cword == 0 || cword > words.len() {
        bail!("cword must be between 1 and {}", words.len());
    }
    let mut line = words[..cword].join(" ");
    if words[cword - 1].is_empty() {
        line.push(' ');
    }
    if line.contains('\n') {
        bail!("a completion line cannot contain a newline");
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let directory = crate::dirs::cache_dir()?
        .join("harness")
        .join(format!("{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let out = directory.join("candidates");
    let timeout = config.settings.timeout()?;
    let timeout_ms = timeout.as_millis() + 2_000;

    let mut process = Command::new("zsh");
    process
        .arg("-f")
        .arg("-c")
        .arg(HARNESS)
        .arg("shcomp")
        .arg(&script)
        .arg(&command)
        .arg(&line)
        .arg(&out)
        .arg(timeout_ms.to_string());
    let (status, stdout, stderr) = source::run_captured(
        process,
        timeout + Duration::from_secs(2),
        "zsh completion harness",
    )?;
    let _ = std::fs::remove_dir_all(&directory);
    if !status.success() {
        bail!(
            "the zsh completion harness failed\n{}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&stdout);
    let mut candidates = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let mut parts = line.splitn(2, '\t');
        let value = parts.next().unwrap_or_default();
        if value.is_empty() || !seen.insert(value.to_string()) {
            continue;
        }
        let description = parts.next().unwrap_or_default();
        if description.is_empty() {
            candidates.push(value.to_string());
        } else {
            candidates.push(format!("{value}\t{description}"));
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_versions_without_touching_ssh() {
        assert_eq!(
            split_tool_version("gh@2.0.0"),
            ("gh".to_string(), Some("2.0.0".to_string()))
        );
        assert_eq!(
            split_tool_version("cli/cli/gh@2.0.0"),
            ("cli/cli/gh".to_string(), Some("2.0.0".to_string()))
        );
        assert_eq!(
            split_tool_version("git@github.com:cli/cli.git"),
            ("git@github.com:cli/cli.git".to_string(), None)
        );
        assert_eq!(split_tool_version("gh"), ("gh".to_string(), None));
    }
}
