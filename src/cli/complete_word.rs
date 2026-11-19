//! `shcomp complete-word`: probe candidates through the target shell.

use comfy_table::Cell;
use eyre::Result;

use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct CompleteWord {
    /// Target shell; defaults to `$SHELL` when that is zsh or fish, otherwise zsh
    #[usage(long, choices("zsh", "fish"))]
    pub shell: Option<String>,

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
/// The shared candidate list inserts the tool name, which is what the command
/// accepts; an ambiguous name falls back to the full reference.
fn complete_tool(
    _complete_word: &<CompleteWord as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::search::candidates(ctx.prefix)
}

/// The shell a user means when they do not say: their login shell when shcomp
/// can drive it, zsh otherwise.
fn default_shell() -> String {
    shell_from(std::env::var_os("SHELL").as_deref())
}

fn shell_from(shell: Option<&std::ffi::OsStr>) -> String {
    let name = shell
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_name)
        .and_then(|name| name.to_str());
    match name {
        Some(name @ ("zsh" | "fish")) => name.to_string(),
        _ => "zsh".to_string(),
    }
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
            shell: self.shell.unwrap_or_else(default_shell),
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
            crate::report::print(&mut table);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_shell_follows_shell_when_shcomp_drives_it() {
        let shell = |value: &str| shell_from(Some(std::ffi::OsStr::new(value)));
        assert_eq!(shell("/usr/bin/fish"), "fish");
        assert_eq!(shell("/bin/zsh"), "zsh");
        assert_eq!(shell("/bin/bash"), "zsh");
        assert_eq!(shell("/opt/zshrs"), "zsh");
        assert_eq!(shell_from(None), "zsh");
    }
}
