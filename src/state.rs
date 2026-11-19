//! What the last cold load of each command did (ADR-0011).
//!
//! `shcomp load` is the cold path: a shim forks it only when the shell has no
//! captured completion for the current binary. It records its outcome here so
//! `doctor` can name a generator that failed silently — the shims discard
//! `load`'s stderr so a broken generator never prints into the prompt — and a
//! shell whose shims have never loaded at all. The warm paths never call
//! `shcomp`, so they never touch these files.

use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// The outcome of the last cold load of one command in one shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadRecord {
    pub shell: String,
    pub command: String,
    pub bin: PathBuf,
    /// The cache key that was served; absent when the load failed.
    pub key: Option<String>,
    /// Seconds since the Unix epoch when this outcome was first seen.
    pub at: u64,
    pub ok: bool,
    /// The first line of the error, when the load failed.
    pub error: Option<String>,
    /// The first line the generator wrote to stderr, when there was one.
    pub stderr: Option<String>,
}

pub fn state_dir(config: &Config) -> Result<PathBuf> {
    Ok(crate::cache::cache_root(config)?.join("state"))
}

fn record_path(config: &Config, shell: &str, command: &str) -> Result<PathBuf> {
    Ok(state_dir(config)?
        .join(shell)
        .join(format!("{command}.json")))
}

impl LoadRecord {
    pub fn success(shell: &str, command: &str, bin: &Path, script: &Path) -> Self {
        Self {
            shell: shell.to_string(),
            command: command.to_string(),
            bin: bin.to_path_buf(),
            key: script
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned()),
            at: crate::cache::unix_now(),
            ok: true,
            error: None,
            stderr: None,
        }
    }

    pub fn failure(shell: &str, command: &str, bin: &Path, error: &eyre::Report) -> Self {
        let text = format!("{error:#}");
        let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
        let first = lines.next().unwrap_or("load failed").to_string();
        Self {
            shell: shell.to_string(),
            command: command.to_string(),
            bin: bin.to_path_buf(),
            key: None,
            at: crate::cache::unix_now(),
            ok: false,
            error: Some(first),
            stderr: lines.next().map(str::to_string),
        }
    }

    /// Two records describe the same outcome when only the time differs.
    fn same_outcome(&self, other: &Self) -> bool {
        self.ok == other.ok
            && self.key == other.key
            && self.bin == other.bin
            && self.error == other.error
            && self.stderr == other.stderr
    }
}

/// Record a load outcome, rewriting the file only when the outcome changed.
///
/// A new shell session loading an already-cached script reads one small file
/// and writes nothing. Recording never fails a load: a cache directory that
/// cannot be written is the cache's problem, reported elsewhere.
pub fn record(config: &Config, record: &LoadRecord) {
    if crate::config::validate_command_name(&record.command).is_err()
        || !crate::shell::managed(&record.shell)
    {
        return;
    }
    let Ok(path) = record_path(config, &record.shell, &record.command) else {
        return;
    };
    if read(&path).is_some_and(|previous| previous.same_outcome(record)) {
        return;
    }
    let Ok(bytes) = serde_json::to_vec_pretty(record) else {
        return;
    };
    if let Err(error) = crate::shell::write_atomic(&path, &bytes) {
        tracing::debug!(%error, path = %path.display(), "cannot record the load");
    }
}

fn read(path: &Path) -> Option<LoadRecord> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Every recorded load for a shell, sorted by command.
pub fn records(config: &Config, shell: &str) -> Vec<LoadRecord> {
    let Ok(dir) = state_dir(config) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir.join(shell)) else {
        return Vec::new();
    };
    let mut records: Vec<LoadRecord> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| read(&entry.path()))
        .collect();
    records.sort_by(|a, b| a.command.cmp(&b.command));
    records
}

/// Whether any shim has loaded through `shcomp load` in this shell: proof
/// that the shell searches shcomp's directory.
pub fn has_loaded(config: &Config, shell: &str) -> bool {
    !records(config, shell).is_empty()
}

/// `just now`, `5m ago`, `3h ago`, `2d ago`.
pub fn ago(at: u64, now: u64) -> String {
    let seconds = now.saturating_sub(at);
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86_400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &Path) -> Config {
        let mut config = Config::default();
        config.settings.cache_dir = Some(dir.join("cache"));
        config
    }

    #[test]
    fn a_failure_keeps_the_first_error_and_stderr_lines() {
        let error = eyre::eyre!(
            "`leaf --auto-complete zsh:dump` exited with exit status: 2\n\nleaf: unknown shell\nmore"
        );
        let record = LoadRecord::failure("zsh", "leaf", Path::new("/bin/leaf"), &error);
        assert!(!record.ok);
        assert_eq!(
            record.error.as_deref(),
            Some("`leaf --auto-complete zsh:dump` exited with exit status: 2")
        );
        assert_eq!(record.stderr.as_deref(), Some("leaf: unknown shell"));
    }

    #[test]
    fn an_unchanged_outcome_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let script = Path::new("/cache/abc.zsh");
        let mut first = LoadRecord::success("zsh", "gh", Path::new("/bin/gh"), script);
        first.at = 100;
        record(&config, &first);
        let mut again = first.clone();
        again.at = 200;
        record(&config, &again);
        assert_eq!(records(&config, "zsh"), vec![first.clone()]);
        assert!(has_loaded(&config, "zsh"));
        assert!(!has_loaded(&config, "fish"));

        let error = eyre::eyre!("boom");
        let failed = LoadRecord::failure("zsh", "gh", Path::new("/bin/gh"), &error);
        record(&config, &failed);
        assert_eq!(records(&config, "zsh"), vec![failed]);
    }

    #[test]
    fn unsafe_names_are_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let record_ = LoadRecord::success("zsh", "../x", Path::new("/bin/x"), Path::new("/k.zsh"));
        record(&config, &record_);
        assert!(!state_dir(&config).unwrap().exists());
    }

    #[test]
    fn ages_read_like_a_person_would_say_them() {
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 300), "5m ago");
        assert_eq!(ago(0, 3 * 3600), "3h ago");
        assert_eq!(ago(0, 2 * 86_400 + 5), "2d ago");
    }
}
