//! `shcomp clean`: remove the completion cache.

use eyre::Result;

#[derive(Debug, usage_rs::Args)]
pub struct Clean;

impl Clean {
    pub fn run(self) -> Result<()> {
        let cache = crate::dirs::cache_dir()?;
        if cache.exists() {
            std::fs::remove_dir_all(&cache)?;
            println!("removed {}", cache.display());
        }
        Ok(())
    }
}
