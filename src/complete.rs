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

/// Result of a completion probe, including metadata for `--json` output.
#[derive(Debug, Clone)]
pub struct CompletionResult {
    /// The resolved tool name
    pub command: String,
    /// The registry reference (e.g., `cli/cli/gh`)
    pub reference: String,
    /// The tool description from the registry item
    pub description: String,
    /// The line passed to the harness
    pub line: String,
    /// Completion candidates (tab-separated value and optional description)
    pub candidates: Vec<String>,
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

pub fn complete(
    config: &Config,
    registry: &Registry,
    request: &Request,
) -> Result<CompletionResult> {
    if request.shell != "zsh" {
        bail!("only zsh is supported in v0.0.1");
    }
    let resolved = registry
        .resolve_str(&request.tool)
        .wrap_err_with(|| format!("cannot complete `{}`", request.tool))?;
    let command = resolved.tool.name.clone();
    let reference = resolved.reference.clone();
    let description = resolved.item.description.clone().unwrap_or_default();
    let bin = source::resolve_binary(&command)?;
    if let Some(expected) = &request.version
        && let Some(actual) = source::version_for_tool(&resolved.tool, &bin)
        && &actual != expected
    {
        bail!("`{command}` resolves to version {actual}, not {expected}");
    }

    let script = crate::cache::load_or_generate(config, registry, &command, "zsh", &bin)?;

    let words = if request.words.is_empty() {
        vec![command.clone(), String::new()]
    } else {
        // The CLI passes words starting after the tool name, so prepend the
        // canonical command.  This also ensures the harness sees the real
        // executable name rather than a shell alias.
        let mut w = vec![command.clone()];
        w.extend(request.words.clone());
        w
    };
    let cword = request.cword.unwrap_or(words.len());
    if cword == 0 || cword > words.len() {
        bail!("cword must be between 1 and {}", words.len());
    }
    let mut line = words[..cword].join(" ");
    // The harness needs a trailing space so the Tab key triggers completion.
    // When the last word is empty, the join already produced a trailing space.
    // When the last word is the command itself (cword == 1), we need a trailing
    // space to start subcommand completion. For any other non-empty word, we
    // must not add a trailing space, so zsh completes the current word (flags
    // for "-", filtered subcommands for "ap", etc.).
    if words[cword - 1].is_empty() || cword == 1 {
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
        let desc = parts.next().unwrap_or_default();
        if desc.is_empty() {
            candidates.push(value.to_string());
        } else {
            candidates.push(format!("{value}\t{desc}"));
        }
    }
    Ok(CompletionResult {
        command,
        reference,
        description,
        line,
        candidates,
    })
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
