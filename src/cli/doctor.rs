//! `shcomp doctor`: check the configuration and installed shims.

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Doctor {
    /// Run each generator and validate its output
    #[usage(long)]
    pub check: bool,
}

impl Doctor {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        println!(
            "config: {}",
            config.source.as_deref().map_or_else(
                || "not found".to_string(),
                |path| path.display().to_string()
            )
        );
        println!("registry items: {}", registry.entries().len());
        for message in crate::shell::zsh::doctor(&config, &registry, self.check)? {
            println!("{message}");
        }
        println!("fpath: {}", crate::shell::zsh::fpath_line(&config)?);
        Ok(())
    }
}
