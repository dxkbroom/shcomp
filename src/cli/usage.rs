//! `shcomp usage`: print the usage KDL for shcomp itself.

use eyre::Result;

#[derive(Debug, usage_rs::Args)]
pub struct Usage;

impl Usage {
    pub fn run(self) -> Result<()> {
        println!("{}", crate::cli::Cli::to_kdl());
        Ok(())
    }
}
