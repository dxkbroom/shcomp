//! `shcomp ls`: list selected tools and their completion sources.

use comfy_table::{Attribute, Cell};
use eyre::Result;

use crate::config::Config;
use crate::registry::{Registry, SelectedTool};

#[derive(Debug, usage_rs::Args)]
pub struct Ls {
    /// Machine-readable output
    #[usage(long)]
    pub json: bool,

    /// Include disabled selections
    #[usage(long)]
    pub all: bool,

    /// Do not print the table header
    #[usage(long, alias = "no-headers")]
    pub no_header: bool,
}

/// What the next Tab costs for one tool, which is the question the `Cache`
/// column answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheState {
    /// A cached script for the resolved binary.
    Warm,
    /// Resolvable, but nothing generated for this version yet.
    Cold,
    /// Selected, but the executable is not on PATH.
    Missing,
    /// No completion source for this shell.
    NoSource,
}

impl CacheState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "warm",
            Self::Cold => "cold",
            Self::Missing => "missing",
            Self::NoSource => "n/a",
        }
    }
}

struct Row {
    reference: String,
    tool: String,
    aliases: Vec<String>,
    repo: String,
    source: String,
    shells: Vec<String>,
    cache: CacheState,
    script: Option<String>,
    key: Option<String>,
    owner: &'static str,
    description: String,
    disabled: bool,
    implicit: bool,
}

impl Row {
    /// `gh (github, ghx) (disabled)`, or just `gh`.
    fn label(&self) -> String {
        let mut label = self.tool.clone();
        if !self.aliases.is_empty() {
            label.push_str(&format!(" ({})", self.aliases.join(", ")));
        }
        if self.disabled {
            label.push_str(" (disabled)");
        }
        if self.implicit {
            label.push_str(" (implicit)");
        }
        label
    }
}

impl Ls {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        // JSON always carries every selection, disabled ones included: a machine
        // consumer can filter, and hiding a row it cannot see is worse than an
        // extra field.
        let rows = rows(&config, &registry, self.all || self.json)?;
        if self.json {
            let rows: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "reference": row.reference,
                        "aliases": row.aliases,
                        "repo": row.repo,
                        "tool": row.tool,
                        "shells": row.shells.clone(),
                        "source": row.source,
                        "cache": {
                            "state": row.cache.as_str(),
                            "script": row.script,
                            "key": row.key,
                        },
                        "owner": row.owner,
                        "description": row.description,
                        "disabled": row.disabled,
                        "implicit": row.implicit,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }

        let mut table = crate::report::table(
            &["Tool", "Repo", "Source", "Cache", "Owner", "Description"],
            self.no_header,
        );
        for row in &rows {
            let cells = [
                Cell::new(row.label()),
                Cell::new(&row.repo),
                Cell::new(&row.source),
                Cell::new(row.cache.as_str()),
                Cell::new(row.owner),
                Cell::new(&row.description),
            ];
            let mut cells = cells.to_vec();
            if row.disabled {
                cells = cells
                    .into_iter()
                    .map(|cell| cell.add_attribute(Attribute::Dim))
                    .collect();
            }
            table.add_row(cells);
        }
        crate::report::print(&table);
        Ok(())
    }
}

fn rows(config: &Config, registry: &Registry, all: bool) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for tool in registry.selections(config)? {
        if tool.disabled && !all {
            continue;
        }
        let command = &tool.resolved.tool.name;
        let (cache, script, key) = cache_state(config, &tool);
        rows.push(Row {
            reference: tool.reference.clone(),
            tool: command.clone(),
            aliases: tool.aliases.clone(),
            repo: tool.resolved.repo_slug().unwrap_or_else(|| "-".to_string()),
            source: tool
                .resolved
                .tool
                .source("zsh")
                .map_or("none".to_string(), |source| source.kind_name().to_string()),
            shells: tool.resolved.tool.completions.keys().cloned().collect(),
            cache,
            script,
            key,
            owner: if crate::shell::zsh::mise_owned(command) {
                "shcomp+mise"
            } else {
                "shcomp"
            },
            description: tool.resolved.item.description.clone().unwrap_or_default(),
            disabled: tool.disabled,
            implicit: tool.implicit,
        });
    }
    Ok(rows)
}

fn cache_state(
    config: &Config,
    tool: &SelectedTool,
) -> (CacheState, Option<String>, Option<String>) {
    let command = &tool.resolved.tool.name;
    let Some(source) = tool.resolved.tool.source("zsh") else {
        return (CacheState::NoSource, None, None);
    };
    let Ok(root) = crate::cache::cache_root(config) else {
        return (CacheState::Missing, None, None);
    };
    let Ok(bin) = crate::source::resolve_binary(command) else {
        return (CacheState::Missing, None, None);
    };
    let Ok(key) = crate::cache::key_for(&bin, source, "zsh") else {
        return (CacheState::Missing, None, None);
    };
    let Ok(path) = crate::cache::script_cache_path(&root, "zsh", command, &key) else {
        return (CacheState::Missing, None, None);
    };
    let state = if path.exists() {
        CacheState::Warm
    } else {
        CacheState::Cold
    };
    (state, Some(path.display().to_string()), Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_states_read_the_way_a_user_asks() {
        assert_eq!(CacheState::Warm.as_str(), "warm");
        assert_eq!(CacheState::Cold.as_str(), "cold");
        assert_eq!(CacheState::Missing.as_str(), "missing");
        assert_eq!(CacheState::NoSource.as_str(), "n/a");
    }

    #[test]
    fn labels_suffix_aliases_and_disabled() {
        let row = Row {
            reference: "cli/cli/gh".to_string(),
            tool: "gh".to_string(),
            aliases: vec!["github".to_string()],
            repo: "cli/cli".to_string(),
            source: "run".to_string(),
            shells: vec!["zsh".to_string()],
            cache: CacheState::Cold,
            script: None,
            key: None,
            owner: "shcomp",
            description: String::new(),
            disabled: true,
            implicit: false,
        };
        assert_eq!(row.label(), "gh (github) (disabled)");
    }
}
