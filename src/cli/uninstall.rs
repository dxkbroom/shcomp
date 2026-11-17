//! `shcomp uninstall`: remove shcomp-owned shims.

use eyre::Result;

use crate::config::Config;

#[derive(Debug, usage_rs::Args)]
pub struct Uninstall {
    /// Print what would be removed without removing it
    #[usage(long)]
    pub dry_run: bool,

    /// Shells to clean; defaults to the `shells` setting
    #[usage(long, short = 's', var)]
    pub shell: Vec<String>,
}

impl Uninstall {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let shells = crate::cli::install::shells(&config, &self.shell)?;
        for shell in &shells {
            let removed = match shell.as_str() {
                "zsh" => crate::shell::zsh::uninstall(&config, self.dry_run)?,
                "fish" => crate::shell::fish::uninstall(&config, self.dry_run)?,
                other => eyre::bail!("`{other}` is not a shell shcomp can install for"),
            };
            for path in removed {
                if self.dry_run {
                    println!("would remove {}", path.display());
                } else {
                    println!("removed {}", path.display());
                }
            }
        }
        Ok(())
    }
}
