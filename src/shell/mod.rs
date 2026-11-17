//! Shell-specific shim generation.

use std::path::PathBuf;

pub mod fish;
pub mod zsh;

/// What a reconcile did to one shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimStatus {
    Wrote,
    UpToDate,
    Removed,
    WouldWrite,
    WouldRemove,
}

impl ShimStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wrote => "wrote",
            Self::UpToDate => "up-to-date",
            Self::Removed => "removed",
            Self::WouldWrite => "would write",
            Self::WouldRemove => "would remove",
        }
    }

    /// Whether this status changed something (or would, in a dry run).
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Wrote | Self::Removed | Self::WouldWrite | Self::WouldRemove
        )
    }

    /// Whether this status is a write rather than a removal.
    pub fn wrote(self) -> bool {
        matches!(self, Self::Wrote | Self::WouldWrite)
    }
}

/// One command's shim after a reconcile.
#[derive(Debug, Clone)]
pub struct InstallRow {
    pub command: String,
    pub shim: PathBuf,
    pub status: ShimStatus,
}

/// What one shell's reconcile did.
#[derive(Debug, Clone)]
pub struct ShellReport {
    pub shell: &'static str,
    pub rows: Vec<InstallRow>,
    /// The one-time line the shell needs to load this directory, if any.
    pub loading: Option<String>,
}

impl ShellReport {
    pub fn row(&self, command: &str) -> Option<&InstallRow> {
        self.rows.iter().find(|row| row.command == command)
    }
}

/// The shells shcomp manages a directory for.
pub fn managed(shell: &str) -> bool {
    matches!(shell, "zsh" | "fish")
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

/// The command names a shim must exist for, with their aliases.
pub(crate) fn expected_commands(
    registry: &Registry,
    config: &Config,
    shell: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for selected in registry.selected(config)? {
        if selected.resolved.tool.source(shell).is_none() {
            continue;
        }
        let command = selected.resolved.tool.name.clone();
        let entry = expected.entry(command).or_default();
        for alias in &selected.aliases {
            if alias != &selected.resolved.tool.name {
                entry.insert(alias.clone());
            }
        }
    }
    Ok(expected)
}

/// A stable identifier for a command, safe in a function name.
pub(crate) fn ident(command: &str) -> String {
    let sanitized: String = command
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let hash = crate::source::sha256_hex(command.as_bytes());
    format!("{sanitized}_{}", &hash[..6])
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}
