//! `shcomp complete-word`: probe candidates through the target shell.

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct CompleteWord {
    /// Target shell
    #[usage(long, default = "zsh")]
    pub shell: String,

    /// Index of the word being completed; defaults to the last word
    #[usage(long)]
    pub cword: Option<usize>,

    /// Tool reference, optionally with `@version`
    #[usage(arg)]
    pub tool: String,

    /// Words typed so far, starting with the command
    #[usage(arg, var_max = 4294967296)]
    pub words: Vec<String>,
}

impl CompleteWord {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let (tool, version) = crate::complete::split_tool_version(&self.tool);
        let request = crate::complete::Request {
            tool,
            version,
            words: self.words,
            cword: self.cword,
            shell: self.shell,
        };
        for candidate in crate::complete::complete(&config, &registry, &request)? {
            println!("{candidate}");
        }
        Ok(())
    }
}
