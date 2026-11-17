//! `shcomp ls`: list selected tools and their completion sources.

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Ls {
    /// Machine-readable output
    #[usage(long)]
    pub json: bool,
}

impl Ls {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        if self.json {
            let rows: Vec<serde_json::Value> = selected
                .iter()
                .map(|tool| {
                    let command = &tool.resolved.tool.name;
                    let source = tool.resolved.tool.source("zsh");
                    let root = crate::cache::cache_root(&config).ok();
                    let cached = source.is_some_and(|source| {
                        crate::source::resolve_binary(command).is_ok_and(|bin| {
                            crate::cache::key_for(&bin, source, "zsh").is_ok_and(|key| {
                                root.as_ref().is_some_and(|root| {
                                    crate::cache::script_cache_path(root, "zsh", command, &key)
                                        .is_ok_and(|path| path.exists())
                                })
                            })
                        })
                    });
                    serde_json::json!({
                        "reference": tool.reference,
                        "repo": tool.resolved.repo_slug(),
                        "tool": command,
                        "aliases": tool.aliases,
                        "shells": tool.resolved.tool.completions.keys().collect::<Vec<_>>(),
                        "source": source.map_or("none", |source| source.kind_name()),
                        "cached": cached,
                        "mise_owned": crate::shell::zsh::mise_owned(command),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        let root = crate::cache::cache_root(&config)?;
        for tool in &selected {
            let command = &tool.resolved.tool.name;
            let source = tool.resolved.tool.source("zsh");
            let kind = source.map_or("none".to_string(), |source| source.kind_name().to_string());
            let cached = source.is_some_and(|source| {
                crate::source::resolve_binary(command).is_ok_and(|bin| {
                    crate::cache::key_for(&bin, source, "zsh").is_ok_and(|key| {
                        crate::cache::script_cache_path(&root, "zsh", command, &key)
                            .is_ok_and(|path| path.exists())
                    })
                })
            });
            let mise = crate::shell::zsh::mise_owned(command);
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                tool.reference,
                command,
                tool.resolved.repo_slug().unwrap_or_else(|| "-".to_string()),
                kind,
                if cached { "cached" } else { "cold" },
                if mise { "mise" } else { "-" }
            );
        }
        Ok(())
    }
}
