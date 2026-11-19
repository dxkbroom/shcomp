//! Ask the user's real interactive shell where it looks for completions
//! (ADR-0012).
//!
//! `fpath` and `fish_complete_path` are assembled by dotfiles, plugin
//! managers, and frameworks, so the only reliable answer is the shell's own.
//! `doctor --check` starts it once per shell with a timeout and reads the path
//! list back between two sentinels, ignoring whatever else startup prints.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::registry::Registry;
use crate::report::Finding;

const BEGIN: &str = "__SHCOMP_PROBE_BEGIN__";
const END: &str = "__SHCOMP_PROBE_END__";

/// Interactive startup can be slow (plugin managers, prompts); this bounds it.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The last probe's answer, kept so `install` and `add` can stay quiet about
/// a loading line that is already in effect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeRecord {
    pub dir: PathBuf,
    pub present: bool,
    pub at: u64,
}

fn record_path(config: &Config, shell: &str) -> Result<PathBuf> {
    // Beside the load records, but not one of them: `state::records` reads
    // `<command>.json`, and a probe is not a command.
    Ok(crate::state::state_dir(config)?.join(format!("{shell}.probe.json")))
}

fn shim_dir(config: &Config, shell: &str) -> Result<PathBuf> {
    match shell {
        "zsh" => super::zsh::site_functions(config),
        "fish" => super::fish::site_completions(config),
        other => bail!("`{other}` is not a shell shcomp can probe"),
    }
}

/// Whether shcomp has evidence that `shell` already searches its directory: a
/// shim has loaded through `shcomp load` there, or the last probe found the
/// directory on the path.
pub fn loading_line_in_effect(config: &Config, shell: &str) -> bool {
    if crate::state::has_loaded(config, shell) {
        return true;
    }
    let (Ok(path), Ok(dir)) = (record_path(config, shell), shim_dir(config, shell)) else {
        return false;
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<ProbeRecord>(&text).ok())
        .is_some_and(|record| record.present && record.dir == dir)
}

/// The directories the interactive shell searches, in order.
pub fn search_path(shell: &str) -> Result<Vec<PathBuf>> {
    let script = match shell {
        "zsh" => format!("print -r -- {BEGIN}; print -rl -- $fpath; print -r -- {END}"),
        "fish" => format!("echo {BEGIN}; printf '%s\\n' $fish_complete_path; echo {END}"),
        other => bail!("`{other}` is not a shell shcomp can probe"),
    };
    let mut command = Command::new(shell);
    command.args(["-i", "-c", &script]).stdin(Stdio::null());
    let (status, stdout, stderr) =
        crate::source::run_captured(command, TIMEOUT, &format!("{shell} -i"))?;
    let stdout = String::from_utf8_lossy(&stdout);
    let Some(list) = between(&stdout) else {
        let stderr = String::from_utf8_lossy(&stderr);
        bail!(
            "an interactive {shell} printed no path list ({status}){}",
            stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| format!(": {}", line.trim()))
                .unwrap_or_default()
        );
    };
    Ok(list)
}

fn between(stdout: &str) -> Option<Vec<PathBuf>> {
    let mut lines = stdout.lines().skip_while(|line| line.trim() != BEGIN);
    lines.next()?;
    let mut dirs = Vec::new();
    for line in lines {
        if line.trim() == END {
            return Some(dirs);
        }
        if !line.is_empty() {
            dirs.push(PathBuf::from(line));
        }
    }
    None
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || crate::source::real_path(a) == crate::source::real_path(b)
}

/// `doctor --check` findings for one shell.
pub fn findings(config: &Config, registry: &Registry, shell: &'static str) -> Vec<Finding> {
    if crate::source::resolve_binary(shell).is_err() {
        return vec![Finding::warn(
            shell,
            format!("{shell} is not on PATH; skipped the interactive probe"),
        )];
    }
    let dirs = match search_path(shell) {
        Ok(dirs) => dirs,
        Err(error) => {
            return vec![Finding::warn(
                shell,
                format!("could not ask an interactive {shell} for its path: {error}"),
            )];
        }
    };
    let Ok(ours) = shim_dir(config, shell) else {
        return Vec::new();
    };
    analyze(config, registry, shell, &ours, &dirs)
}

fn analyze(
    config: &Config,
    registry: &Registry,
    shell: &'static str,
    ours: &Path,
    dirs: &[PathBuf],
) -> Vec<Finding> {
    let (variable, line) = match shell {
        "zsh" => ("fpath", super::zsh::fpath_line(config).unwrap_or_default()),
        _ => (
            "fish_complete_path",
            super::fish::loading_line(config).unwrap_or_default(),
        ),
    };
    let position = dirs.iter().position(|dir| same_dir(dir, ours));
    let mut findings = Vec::new();

    let record = ProbeRecord {
        dir: ours.to_path_buf(),
        present: position.is_some(),
        at: crate::cache::unix_now(),
    };
    if let (Ok(path), Ok(bytes)) = (record_path(config, shell), serde_json::to_vec(&record)) {
        let _ = super::write_atomic(&path, &bytes);
    }

    match position {
        Some(index) => findings.push(Finding::ok(
            shell,
            format!(
                "an interactive {shell} has {} on {variable} (entry {} of {})",
                ours.display(),
                index + 1,
                dirs.len()
            ),
        )),
        None => findings.push(Finding::warn(
            shell,
            match shell {
                "zsh" => format!(
                    "an interactive zsh does not have {} on fpath; add `{line}` before compinit",
                    ours.display()
                ),
                _ => format!(
                    "an interactive fish does not have {} on fish_complete_path; add `{line}` to ~/.config/fish/config.fish",
                    ours.display()
                ),
            },
        )),
    }

    // A file for the same command earlier on the path wins over the shim.
    let file_name = |command: &str| match shell {
        "zsh" => format!("_{command}"),
        _ => format!("{command}.fish"),
    };
    let expected = crate::shell::expected_commands(registry, config, shell).unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for (command, aliases) in &expected {
        names.push(command.clone());
        if shell == "fish" {
            names.extend(aliases.iter().cloned());
        }
    }
    if let Some(index) = position {
        for name in &names {
            for dir in &dirs[..index] {
                let path = dir.join(file_name(name));
                if path.is_file() && !is_stale_self(&path) {
                    findings.push(Finding::warn(
                        shell,
                        format!(
                            "{name}: {} comes before shcomp's shim on {variable} and shadows it",
                            path.display()
                        ),
                    ));
                }
            }
        }
    }

    // A static `shcomp completion` snapshot from v0.0.1 anywhere on the path.
    for dir in dirs.iter().filter(|dir| !same_dir(dir, ours)) {
        let path = dir.join(file_name("shcomp"));
        if is_stale_self(&path) {
            findings.push(Finding::warn(
                shell,
                format!(
                    "{} is a static shcomp completion left by an older install; shcomp installs its own now, so remove it: rm {}",
                    path.display(),
                    super::shell_quote(&path.to_string_lossy())
                ),
            ));
        }
    }
    findings
}

/// A completion for shcomp itself that shcomp's installer did not write: the
/// output of `shcomp completion <shell>` saved to a file by hand in v0.0.1.
fn is_stale_self(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let names_shcomp = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "_shcomp" || name == "shcomp.fish");
    names_shcomp && !text.contains("# @generated by shcomp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_list_is_read_between_the_sentinels() {
        let out = format!("motd\n{BEGIN}\n/a\n\n/b c\n{END}\nbye\n");
        assert_eq!(
            between(&out).unwrap(),
            [PathBuf::from("/a"), PathBuf::from("/b c")]
        );
        assert!(between("no sentinels").is_none());
        assert!(between(&format!("{BEGIN}\n/a\n")).is_none());
    }

    #[test]
    fn a_probe_finds_the_line_shadows_and_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let early = dir.path().join("early");
        let late = dir.path().join("late");
        let ours = dir.path().join("data/zsh/site-functions");
        for path in [&early, &late, &ours] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(early.join("_gh"), "#compdef gh\n").unwrap();
        std::fs::write(
            late.join("_shcomp"),
            "#compdef shcomp\n# @generated by usage-argv for `shcomp __complete_word__`\n",
        )
        .unwrap();
        let mut config: Config = toml::from_str(
            r#"
[tools]
"example/gh" = true

[[registry]]
repo = "example/gh"
main = "gh"

[[registry.tools]]
name = "gh"

[registry.tools.completions.zsh]
run = ["{{ bin }}"]
"#,
        )
        .unwrap();
        config.settings.data_dir = Some(dir.path().join("data"));
        config.settings.cache_dir = Some(dir.path().join("cache"));
        let registry = Registry::load_with(&config, Some(&dir.path().join("none"))).unwrap();

        let found = analyze(
            &config,
            &registry,
            "zsh",
            &ours,
            &[early.clone(), ours.clone(), late.clone()],
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert!(messages[0].contains("entry 2 of 3"), "{messages:?}");
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("gh: ") && m.contains("shadows it")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("static shcomp completion")),
            "{messages:?}"
        );
        assert!(loading_line_in_effect(&config, "zsh"));

        let found = analyze(&config, &registry, "zsh", &ours, &[early, late]);
        assert!(found[0].message.contains("before compinit"), "{found:?}");
        assert!(!loading_line_in_effect(&config, "zsh"));
    }
}
