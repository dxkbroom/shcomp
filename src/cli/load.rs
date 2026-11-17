//! `shcomp load`: print the cached script for one command and executable.
//!
//! This is the command a shell shim forks on a cold Tab.

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Load {
    /// Target shell
    #[usage(long, default = "zsh")]
    pub shell: String,

    /// Command name as typed
    #[usage(long)]
    pub command: String,

    /// Resolved executable path
    #[usage(long)]
    pub bin: String,
}

impl Load {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let path = crate::cache::load_or_generate(
            &config,
            &registry,
            &self.command,
            &self.shell,
            std::path::Path::new(&self.bin),
        )?;
        println!("{}", path.display());
        Ok(())
    }
}
