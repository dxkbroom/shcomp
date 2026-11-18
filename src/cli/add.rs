//! `shcomp add`: add a tool selection to the config.

use eyre::{Result, WrapErr};

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Add {
    /// Tool reference to add
    #[usage(arg)]
    pub reference: String,

    /// Extra command name to register; repeat for several
    #[usage(long, var)]
    pub alias: Vec<String>,
}

impl Add {
    pub fn run(self) -> Result<()> {
        let config_path = crate::dirs::config_file()?;
        let config = if config_path.exists() {
            Config::load(&config_path)?
        } else {
            Config::default()
        };
        let registry = Registry::load(&config)?;
        let resolved = registry
            .resolve_str(&self.reference)
            .wrap_err_with(|| format!("cannot add `{}`", self.reference))?;
        let original = if config_path.exists() {
            std::fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let updated = crate::config::add_selection(&original, &self.reference, &self.alias)?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, updated)?;
        println!(
            "added {} ({}) to {}",
            self.reference,
            resolved.tool.name,
            config_path.display()
        );
        Ok(())
    }
}
