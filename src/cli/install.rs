//! `shcomp install`: reconcile shell shims for the selected tools.

use eyre::{Result, bail};

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Install {
    /// Do not fetch URL sources
    #[usage(long)]
    pub no_fetch: bool,

    /// Fail instead of fetching an uncached URL source
    #[usage(long)]
    pub offline: bool,

    /// Generate scripts for the active versions now
    #[usage(long)]
    pub prewarm: bool,
}

impl Install {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        if selected.is_empty() {
            bail!("no tools are selected; add one with `shcomp add <tool>`");
        }
        let messages = crate::cache::prefetch(
            &config,
            &registry,
            self.prewarm,
            !self.no_fetch && !self.offline,
        )?;
        for message in messages {
            println!("{message}");
        }
        let report = crate::shell::zsh::install(&config, &registry)?;
        for path in &report.written {
            println!("wrote {}", path.display());
        }
        for path in &report.removed {
            println!("removed {}", path.display());
        }
        for command in &report.skipped_mise {
            println!("skipped {command}: mise's packslip loader owns it");
        }
        println!(
            "\nadd this before `compinit`:\n\n{}\n",
            crate::shell::zsh::fpath_line(&config)?
        );
        Ok(())
    }
}
