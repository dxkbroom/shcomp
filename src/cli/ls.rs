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

    /// Describe one shell: its source and cache state, and only tools that
    /// complete in it
    #[usage(long, short = 's', choices("zsh", "fish"))]
    pub shell: Option<String>,
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

/// One shell's cache entry for one tool.
#[derive(Debug, Clone)]
struct Cache {
    state: CacheState,
    script: Option<String>,
    key: Option<String>,
}

impl Cache {
    const fn bare(state: CacheState) -> Self {
        Self {
            state,
            script: None,
            key: None,
        }
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "state": self.state.as_str(),
            "script": self.script,
            "key": self.key,
        })
    }
}

struct Row {
    reference: String,
    tool: String,
    aliases: Vec<String>,
    repo: String,
    source: String,
    shells: Vec<String>,
    /// The described shell's entry: the `--shell` one, or the preferred one.
    cache: Cache,
    /// Every shown shell's entry, in the order of the `Cache` column.
    caches: Vec<(String, Cache)>,
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

    /// `warm/cold` across the shown shells; `-` where a shell has no source,
    /// and a single `missing` when the executable is not on PATH.
    fn cache_cell(&self) -> String {
        if self
            .caches
            .iter()
            .all(|(_, cache)| cache.state == CacheState::Missing)
        {
            return CacheState::Missing.as_str().to_string();
        }
        self.caches
            .iter()
            .map(|(_, cache)| match cache.state {
                CacheState::NoSource => "-",
                state => state.as_str(),
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

impl Ls {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        // JSON always carries every selection, disabled ones included: a machine
        // consumer can filter, and hiding a row it cannot see is worse than an
        // extra field.
        let shells: Vec<String> = match &self.shell {
            Some(shell) => vec![shell.clone()],
            None => config
                .settings
                .shells
                .iter()
                .filter(|shell| crate::shell::managed(shell))
                .cloned()
                .collect(),
        };
        let rows = rows(
            &config,
            &registry,
            self.all || self.json,
            &shells,
            self.shell.as_deref(),
        )?;
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
                        "cache": row.cache.json(),
                        "caches": row
                            .caches
                            .iter()
                            .map(|(shell, cache)| (shell.clone(), cache.json()))
                            .collect::<serde_json::Map<_, _>>(),
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

        // The header names the shells when the cell holds more than one.
        let cache_header = if shells.len() > 1 {
            format!("Cache ({})", shells.join("/"))
        } else {
            "Cache".to_string()
        };
        let mut table = crate::report::table(
            &[
                "Tool",
                "Repo",
                "Source",
                "Shells",
                &cache_header,
                "Owner",
                "Description",
            ],
            self.no_header,
        );
        for row in &rows {
            let cells = [
                Cell::new(row.label()),
                Cell::new(&row.repo),
                Cell::new(&row.source),
                Cell::new(row.shells.join(", ")),
                Cell::new(row.cache_cell()),
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
        // Description, then Source, give way before a word is split.
        crate::report::print_dropping(&mut table, &[6, 2]);
        Ok(())
    }
}

fn rows(
    config: &Config,
    registry: &Registry,
    all: bool,
    shells: &[String],
    only: Option<&str>,
) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    let root = crate::cache::cache_root(config).ok();
    for tool in registry.selections(config)? {
        if tool.disabled && !all {
            continue;
        }
        if let Some(shell) = only
            && tool.resolved.tool.source(shell).is_none()
        {
            continue;
        }
        let command = &tool.resolved.tool.name;
        // One PATH lookup and one stat per tool, shared by every shell's key.
        let bin = crate::source::resolve_binary(command)
            .ok()
            .and_then(|bin| crate::cache::BinStamp::of(&bin).ok());
        let caches: Vec<(String, Cache)> = shells
            .iter()
            .map(|shell| {
                (
                    shell.clone(),
                    cache_state(root.as_deref(), &tool, shell, bin.as_ref()),
                )
            })
            .collect();
        let described = only.or_else(|| preferred_shell(&tool));
        let cache = match described {
            Some(shell) => caches
                .iter()
                .find(|(name, _)| name == shell)
                .map(|(_, cache)| cache.clone())
                .unwrap_or_else(|| cache_state(root.as_deref(), &tool, shell, bin.as_ref())),
            None => Cache::bare(CacheState::NoSource),
        };
        rows.push(Row {
            reference: tool.reference.clone(),
            tool: command.clone(),
            aliases: tool.aliases.clone(),
            repo: tool.resolved.repo_slug().unwrap_or_else(|| "-".to_string()),
            source: described
                .and_then(|shell| tool.resolved.tool.source(shell))
                .map_or("none".to_string(), |source| source.kind_name().to_string()),
            shells: tool.resolved.tool.completed_shells().into_iter().collect(),
            cache,
            caches,
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

/// The shell whose source the table describes: zsh when the tool has one,
/// otherwise the first managed shell it does have.
fn preferred_shell(tool: &SelectedTool) -> Option<&'static str> {
    ["zsh", "fish"]
        .into_iter()
        .find(|shell| tool.resolved.tool.source(shell).is_some())
}

fn cache_state(
    root: Option<&std::path::Path>,
    tool: &SelectedTool,
    shell: &str,
    bin: Option<&crate::cache::BinStamp>,
) -> Cache {
    let command = &tool.resolved.tool.name;
    let Some(source) = tool.resolved.tool.source(shell) else {
        return Cache::bare(CacheState::NoSource);
    };
    let (Some(root), Some(bin)) = (root, bin) else {
        return Cache::bare(CacheState::Missing);
    };
    let Ok(key) = crate::cache::key_with(bin, &source, shell) else {
        return Cache::bare(CacheState::Missing);
    };
    let Ok(path) = crate::cache::script_cache_path(root, shell, command, &key) else {
        return Cache::bare(CacheState::Missing);
    };
    let state = if path.exists() {
        CacheState::Warm
    } else {
        CacheState::Cold
    };
    Cache {
        state,
        script: Some(path.display().to_string()),
        key: Some(key),
    }
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
            cache: Cache::bare(CacheState::Cold),
            caches: vec![
                ("zsh".to_string(), Cache::bare(CacheState::Warm)),
                ("fish".to_string(), Cache::bare(CacheState::NoSource)),
            ],
            owner: "shcomp",
            description: String::new(),
            disabled: true,
            implicit: false,
        };
        assert_eq!(row.label(), "gh (github) (disabled)");
        assert_eq!(row.cache_cell(), "warm/-");
    }

    #[test]
    fn a_missing_binary_reads_once() {
        let missing = || Cache::bare(CacheState::Missing);
        let row = Row {
            reference: "gh".to_string(),
            tool: "gh".to_string(),
            aliases: Vec::new(),
            repo: "-".to_string(),
            source: "run".to_string(),
            shells: Vec::new(),
            cache: missing(),
            caches: vec![
                ("zsh".to_string(), missing()),
                ("fish".to_string(), missing()),
            ],
            owner: "shcomp",
            description: String::new(),
            disabled: false,
            implicit: false,
        };
        assert_eq!(row.cache_cell(), "missing");
    }
}
