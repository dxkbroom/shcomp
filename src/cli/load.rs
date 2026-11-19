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
        let bin = std::path::Path::new(&self.bin);
        let result =
            crate::cache::load_or_generate(&config, &registry, &self.command, &self.shell, bin);
        // Only this command records: it is the cold path, and the shims
        // discard its stderr, so the record is how a failure surfaces.
        let record = match &result {
            Ok(path) => crate::state::LoadRecord::success(&self.shell, &self.command, bin, path),
            Err(error) => crate::state::LoadRecord::failure(&self.shell, &self.command, bin, error),
        };
        crate::state::record(&config, &record);
        println!("{}", result?.display());
        Ok(())
    }
}
