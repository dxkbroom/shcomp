//! `shcomp complete-word`: probe candidates through the target shell.

use comfy_table::Cell;
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

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,

    /// Tool reference, optionally with `@version`
    #[usage(arg, complete = crate::cli::complete_word::complete_tool)]
    pub tool: String,

    /// Words typed so far, starting with the command
    #[usage(arg, var_max = 4294967296)]
    pub words: Vec<String>,
}

/// The completer for `complete-word <TOOL>`.
///
/// Returns only the tool name (not the full reference) so the completion list
/// is shorter and matches what the user types.
fn complete_tool(
    _complete_word: &<CompleteWord as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    let candidates = crate::cli::search::candidates(ctx.prefix);
    candidates
        .into_iter()
        .map(|c| {
            // The value from search::candidates is the full reference
            // (e.g., `cli/cli/gh`).  Extract just the tool name (the last
            // path component) for a cleaner completion list.
            let value = c.value;
            Candidate {
                value,
                description: c.description,
                ..Candidate::new(String::new())
            }
        })
        .collect()
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
        let result = crate::complete::complete(&config, &registry, &request)?;
        if self.json {
            let items: Vec<serde_json::Value> = result
                .candidates
                .iter()
                .map(|candidate| {
                    let mut parts = candidate.splitn(2, '\t');
                    let value = parts.next().unwrap_or_default();
                    let description = parts.next().unwrap_or_default();
                    if description.is_empty() {
                        serde_json::json!({ "value": value })
                    } else {
                        serde_json::json!({ "value": value, "description": description })
                    }
                })
                .collect();
            let output = serde_json::json!({
                "tool": result.command,
                "reference": result.reference,
                "description": result.description,
                "line": result.line,
                "items": items,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !result.candidates.is_empty() {
            let mut table = crate::report::table(&["Completion", "Description"], false);
            for candidate in result.candidates {
                let mut parts = candidate.splitn(2, '\t');
                let value = parts.next().unwrap_or_default();
                let description = parts.next().unwrap_or_default();
                if description.is_empty() {
                    table.add_row([Cell::new(value)]);
                } else {
                    table.add_row([Cell::new(value), Cell::new(description)]);
                }
            }
            crate::report::print(&table);
        }
        Ok(())
    }
}
