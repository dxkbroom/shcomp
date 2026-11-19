//! `shcomp search`: list tools the registry can provide.
//!
//! Ranking and completion candidates live in `crate::search`; this module owns
//! the flags, the table, the picker, and the hidden output endpoints.

use demand::{DemandOption, Select};
use eyre::{Result, bail};
use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::Registry;
use crate::search::{self, Hit, MatchType};

#[derive(Debug, usage_rs::Args)]
pub struct Search {
    /// Name, workspace, or reference to search for
    #[usage(arg, complete = crate::cli::search::complete_name)]
    pub name: Option<String>,

    /// Pick a result from a menu and add it to the config
    #[usage(long, short, conflicts = &["match_type", "no_header", "json"])]
    pub interactive: bool,

    /// Match type: equal, contains, or fuzzy
    #[usage(long, short, value_enum, default = "fuzzy")]
    pub match_type: MatchType,

    /// Do not print the table header
    #[usage(long, alias = "no-headers")]
    pub no_header: bool,

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,

    /// Print every candidate with its description, for shell completions
    #[usage(long, hide = true)]
    pub complete: bool,

    /// Print only candidate values, for shell completions
    #[usage(long, hide = true)]
    pub complete_ids: bool,
}

impl Search {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let query = self.name.clone().unwrap_or_default();
        let mut hits = search::search(&registry, &query, self.match_type);

        if self.complete || self.complete_ids {
            for candidate in search::into_candidates(hits, &query) {
                if self.complete {
                    println!(
                        "{}\t{}",
                        candidate.value,
                        candidate.description.as_deref().unwrap_or_default()
                    );
                } else {
                    println!("{}", candidate.value);
                }
            }
            return Ok(());
        }

        if self.interactive {
            return self.pick(&mut hits);
        }

        if hits.is_empty() {
            bail!("no registry item matches `{query}`");
        }

        if self.json {
            let rows: Vec<serde_json::Value> = hits
                .iter()
                .map(|hit| {
                    serde_json::json!({
                        "reference": hit.reference,
                        "tool": hit.tool,
                        "repo": hit.repo,
                        "aliases": hit.aliases,
                        "source": hit.source,
                        "description": hit.description,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }

        let mut table = crate::report::table(
            &["Reference", "Tool", "Repo", "Source", "Description"],
            self.no_header,
        );
        for hit in &hits {
            table.add_row([
                comfy_table::Cell::new(&hit.reference),
                comfy_table::Cell::new(hit.label()),
                comfy_table::Cell::new(&hit.repo),
                comfy_table::Cell::new(&hit.source),
                comfy_table::Cell::new(&hit.description),
            ]);
        }
        // Description, then Source, give way before a word is split.
        crate::report::print_dropping(&mut table, &[4, 3]);
        Ok(())
    }

    /// Put the results in a filterable list and add whatever is picked. `add`
    /// prints its own report, so this only runs it.
    fn pick(self, hits: &mut Vec<Hit>) -> Result<()> {
        let query = self.name.clone().unwrap_or_default();
        if hits.is_empty() {
            bail!("no registry item matches `{query}`");
        }
        let mut select = Select::new("Tool")
            .description("Pick a tool to add")
            .filtering(true)
            .filterable(true);
        for hit in hits.iter() {
            select = select.option(
                DemandOption::new(hit.reference.clone())
                    .label(hit.reference.as_str())
                    .description(&hit.description),
            );
        }
        let picked = match select.run() {
            Ok(picked) => picked,
            // Ctrl-C is a normal way to leave a picker.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        crate::cli::add::Add {
            reference: picked,
            ..Default::default()
        }
        .run()?;
        Ok(())
    }
}

/// The completer for `search <NAME>`.
fn complete_name(
    _search: &<Search as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    search::candidates(ctx.prefix)
}
