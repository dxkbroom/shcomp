//! `shcomp uninstall`: remove shcomp-owned shims.

use eyre::Result;

use crate::config::Config;

#[derive(Debug, usage_rs::Args)]
pub struct Uninstall {
    /// Print what would be removed without removing it
    #[usage(long)]
    pub dry_run: bool,
}

impl Uninstall {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let removed = crate::shell::zsh::uninstall(&config, self.dry_run)?;
        for path in removed {
            if self.dry_run {
                println!("would remove {}", path.display());
            } else {
                println!("removed {}", path.display());
            }
        }
        Ok(())
    }
}
