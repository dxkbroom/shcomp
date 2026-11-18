//! `shcomp search`: find tools the registry can provide.
//!
//! The corpus is every tool of every registry item — embedded, user, and inline
//! — so a tool that only exists in a user's registry is as findable as a
//! shipped one. A row is a tool, and its reference is the form `add` accepts.

use demand::{DemandOption, Select};
use eyre::{Result, bail};
use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, usage_rs::ValueEnum)]
pub enum MatchType {
    Equal,
    Contains,
    Fuzzy,
}

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

    /// Print every reference with its description, for shell completions
    #[usage(long, hide = true)]
    pub complete: bool,

    /// Print only references, for shell completions
    #[usage(long, hide = true)]
    pub complete_ids: bool,
}

/// One tool the registry can provide.
#[derive(Debug, Clone)]
struct Hit {
    reference: String,
    tool: String,
    repo: String,
    aliases: Vec<String>,
    source: String,
    description: String,
}

impl Hit {
    /// How well this hit answers `query`, lower being better, or `None`.
    fn score(&self, query: &str, kind: MatchType) -> Option<u32> {
        if query.is_empty() {
            return Some(0);
        }
        let mut best: Option<u32> = None;
        for candidate in self.candidates() {
            let score = match kind {
                MatchType::Equal => (candidate.eq_ignore_ascii_case(query)).then_some(0),
                MatchType::Contains => candidate
                    .to_lowercase()
                    .contains(&query.to_lowercase())
                    .then_some(0),
                MatchType::Fuzzy => fuzzy_score(query, &candidate),
            };
            if let Some(score) = score {
                best = Some(best.map_or(score, |current| current.min(score)));
            }
        }
        best
    }

    /// Everything a query may name: the reference, the command, the workspace,
    /// and the command's aliases.
    fn candidates(&self) -> Vec<String> {
        let mut candidates = vec![self.reference.clone(), self.tool.clone()];
        if self.repo != "-" {
            candidates.push(self.repo.clone());
        }
        candidates.extend(self.aliases.iter().cloned());
        candidates
    }

    /// `gh (github)`.
    fn label(&self) -> String {
        if self.aliases.is_empty() {
            self.tool.clone()
        } else {
            format!("{} ({})", self.tool, self.aliases.join(", "))
        }
    }
}

impl Search {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let query = self.name.clone().unwrap_or_default();
        let mut hits = search(&registry, &query, self.match_type);

        if self.complete || self.complete_ids {
            for hit in &hits {
                if self.complete {
                    println!("{}\t{}", hit.reference, hit.description);
                } else {
                    println!("{}", hit.reference);
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
        crate::report::print(&table);
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

/// Candidate references for a shell, filtered by the word being typed.
///
/// This is the one answer behind `add <TAB>`, `complete-word <TAB>`, and the
/// hidden `search --complete` endpoint, so a shell and a user see the same
/// corpus. A failure here yields no candidates: a Tab must never print an error.
pub fn candidates(prefix: &str) -> Vec<Candidate<'static>> {
    let Ok(config) = Config::load_default() else {
        return Vec::new();
    };
    let Ok(registry) = Registry::load(&config) else {
        return Vec::new();
    };
    search(&registry, prefix, MatchType::Fuzzy)
        .into_iter()
        .map(|hit| Candidate {
            value: hit.reference,
            description: (!hit.description.is_empty()).then(|| hit.description.into()),
            ..Candidate::new(String::new())
        })
        .collect()
}

/// The completer for `search <NAME>`.
fn complete_name(
    _search: &<Search as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    candidates(ctx.prefix)
}

/// Every tool of every item, filtered and ranked by `query`.
fn search(registry: &Registry, query: &str, kind: MatchType) -> Vec<Hit> {
    let mut scored: Vec<(u32, Hit)> = registry
        .entries()
        .iter()
        .flat_map(|entry| {
            let repo = entry.repo.as_ref().map(|repo| repo.slug());
            entry.item.tools.iter().map(move |tool| {
                let reference = match &repo {
                    Some(repo) => format!("{repo}/{}", tool.name),
                    None => tool.name.clone(),
                };
                Hit {
                    reference,
                    tool: tool.name.clone(),
                    repo: repo.clone().unwrap_or_else(|| "-".to_string()),
                    aliases: tool.aliases.clone(),
                    source: tool
                        .source("zsh")
                        .map_or("none".to_string(), |source| source.kind_name().to_string()),
                    description: entry.item.description.clone().unwrap_or_default(),
                }
            })
        })
        .filter_map(|hit| hit.score(query, kind).map(|score| (score, hit)))
        .collect();
    scored.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.reference.cmp(&right.1.reference))
    });
    scored.into_iter().map(|(_, hit)| hit).collect()
}

/// Subsequence match, scored by how much of the candidate the query skipped.
///
/// A lower score is a tighter match: `gh` against `cli/cli/gh` skips more than
/// against `gh`, and a query that never appears in order finds nothing.
fn fuzzy_score(query: &str, candidate: &str) -> Option<u32> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.is_empty() {
        return Some(0);
    }
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    let mut matched = 0;
    let mut skipped = 0u32;
    let mut last: Option<usize> = None;
    for (index, ch) in candidate.iter().enumerate() {
        if matched == query.len() {
            break;
        }
        if *ch == query[matched] {
            skipped += match last {
                Some(previous) => (index - previous - 1) as u32,
                None => index as u32,
            };
            last = Some(index);
            matched += 1;
        }
    }
    (matched == query.len()).then_some(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(reference: &str, tool: &str, repo: &str, aliases: &[&str]) -> Hit {
        Hit {
            reference: reference.to_string(),
            tool: tool.to_string(),
            repo: repo.to_string(),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
            source: "run".to_string(),
            description: String::new(),
        }
    }

    #[test]
    fn fuzzy_matching_prefers_the_tighter_match() {
        let close = fuzzy_score("gh", "gh").unwrap();
        let far = fuzzy_score("gh", "cli/cli/gh").unwrap();
        assert!(close < far, "{close} !< {far}");
        assert_eq!(fuzzy_score("zz", "gh"), None);
    }

    #[test]
    fn fuzzy_matching_is_ordered_not_contiguous() {
        assert!(fuzzy_score("clgh", "cli/cli/gh").is_some());
        assert_eq!(fuzzy_score("hg", "gh"), None);
    }

    #[test]
    fn every_reference_form_is_a_candidate() {
        let hit = hit("cli/cli/gh", "gh", "cli/cli", &["github"]);
        assert_eq!(hit.score("cli/cli", MatchType::Equal), Some(0));
        assert_eq!(hit.score("github", MatchType::Equal), Some(0));
        assert_eq!(hit.score("github", MatchType::Contains), Some(0));
        assert_eq!(hit.score("hub", MatchType::Equal), None);
        assert_eq!(hit.score("hub", MatchType::Contains), Some(0));
    }

    #[test]
    fn labels_name_the_aliases() {
        assert_eq!(
            hit("cli/cli/gh", "gh", "cli/cli", &["github"]).label(),
            "gh (github)"
        );
        assert_eq!(hit("cli/cli/gh", "gh", "cli/cli", &[]).label(), "gh");
    }
}
