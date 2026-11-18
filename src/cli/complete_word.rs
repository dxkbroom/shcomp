//! `shcomp complete-word`: probe candidates through the target shell.

use eyre::Result;

use usage_rs::complete::{Candidate, CompleteCtx};

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
    #[usage(arg, complete = crate::cli::complete_word::complete_tool)]
    pub tool: String,

    /// Words typed so far, starting with the command
    #[usage(arg, var_max = 4294967296)]
    pub words: Vec<String>,
}

/// The completer for `complete-word <TOOL>`.
fn complete_tool(
    _complete_word: &<CompleteWord as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::cli::search::candidates(ctx.prefix)
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
