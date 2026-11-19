//! `shcomp clean`: remove cached completion scripts.

use eyre::{Result, bail};
use usage_rs::complete::{Candidate, CompleteCtx};

use crate::cache::{self, CachedScript};
use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, Default, usage_rs::Args)]
pub struct Clean {
    /// Remove only scripts no `shcomp load` can reach any more
    #[usage(long)]
    pub stale: bool,

    /// Only this shell's scripts
    #[usage(long, short = 's')]
    pub shell: Option<String>,

    /// Print what would be removed without removing it
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// Only these tools' scripts; defaults to every tool
    #[usage(arg, complete = crate::cli::clean::complete_tool)]
    pub tool: Vec<String>,
}

/// The completer for `clean <TOOL>`: the selected commands.
fn complete_tool(
    _clean: &<Clean as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::search::selected_candidates(ctx.prefix)
}

impl Clean {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let root = cache::cache_root(&config)?;
        if let Some(shell) = &self.shell {
            crate::config::validate_shell(shell)?;
        }

        // The whole cache, as before: URL downloads, scripts, and metadata.
        if !self.stale && self.shell.is_none() && self.tool.is_empty() {
            if root.exists() {
                if self.dry_run {
                    println!("would remove {}", root.display());
                } else {
                    std::fs::remove_dir_all(&root)?;
                    println!("removed {}", root.display());
                }
            }
            return Ok(());
        }

        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        let tools = self
            .tool
            .iter()
            .map(|name| command_for(&selected, name))
            .collect::<Result<Vec<_>>>()?;
        let wanted = |script: &CachedScript| {
            self.shell
                .as_ref()
                .is_none_or(|shell| &script.shell == shell)
                && (tools.is_empty() || tools.contains(&script.command))
        };

        let verb = if self.dry_run {
            "would remove"
        } else {
            "removed"
        };
        let mut removed = 0;
        for script in cache::scripts(&root).iter().filter(|script| wanted(script)) {
            let reason = if self.stale {
                match cache::stale_reason(script, &selected) {
                    Some(reason) => Some(reason),
                    None => continue,
                }
            } else {
                None
            };
            if !self.dry_run {
                cache::remove_script(&root, &script.path)?;
            }
            removed += 1;
            match reason {
                Some(reason) => println!("{verb} {} ({reason})", script.path.display()),
                None => println!("{verb} {}", script.path.display()),
            }
        }
        println!("{verb} {removed} script(s)");
        Ok(())
    }
}

/// The cache directory a tool's scripts live under: its command name, even
/// when the user typed an alias. A name the config does not select is taken
/// as-is, so scripts of a tool that was removed can still be cleaned.
fn command_for(selected: &[crate::registry::SelectedTool], name: &str) -> Result<String> {
    if name.is_empty() || name.contains('/') {
        bail!("`{name}` is not a command name");
    }
    Ok(selected
        .iter()
        .find(|tool| {
            tool.resolved.tool.name == name || tool.aliases.iter().any(|alias| alias == name)
        })
        .map_or_else(|| name.to_string(), |tool| tool.resolved.tool.name.clone()))
}
