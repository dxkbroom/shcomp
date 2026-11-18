//! Search the registry corpus for tools a user can add or complete.
//!
//! The corpus is every tool of every registry item — embedded, user, and inline
//! — so a tool that only exists in a user's registry is as findable as a
//! shipped one. A hit is a tool, and its reference is the form `add` accepts.
//!
//! This module is the one search implementation behind `search`, `add <TAB>`,
//! `search <TAB>`, and `complete-word <TAB>`. The CLI owns presentation; this
//! owns ranking and candidate values.

use std::collections::HashMap;

use usage_rs::complete::Candidate;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, usage_rs::ValueEnum)]
pub enum MatchType {
    Equal,
    Contains,
    Fuzzy,
}

/// One tool the registry can provide.
#[derive(Debug, Clone)]
pub(crate) struct Hit {
    pub(crate) reference: String,
    pub(crate) tool: String,
    pub(crate) repo: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) source: String,
    pub(crate) description: String,
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
    pub(crate) fn label(&self) -> String {
        if self.aliases.is_empty() {
            self.tool.clone()
        } else {
            format!("{} ({})", self.tool, self.aliases.join(", "))
        }
    }
}

/// Every tool of every item, filtered and ranked by `query`.
pub(crate) fn search(registry: &Registry, query: &str, kind: MatchType) -> Vec<Hit> {
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

/// Candidate values for a shell, filtered by the word being typed.
///
/// A user knows a tool by its command name, so that is what the shell inserts.
/// When two registry entries share a name, the full reference is the only
/// unambiguous value and is offered instead.
pub(crate) fn candidates(prefix: &str) -> Vec<Candidate<'static>> {
    let Ok(config) = Config::load_default() else {
        return Vec::new();
    };
    let Ok(registry) = Registry::load(&config) else {
        return Vec::new();
    };
    into_candidates(search(&registry, prefix, MatchType::Fuzzy), prefix)
}

/// Turn ranked hits into completion candidates.
///
/// The shell filters candidates by the typed prefix, so the value has to start
/// with it. Prefer the tool name; fall back to the reference when the user is
/// typing a reference (or the name is ambiguous), and to an alias when the
/// query matches one.
pub(crate) fn into_candidates(hits: Vec<Hit>, prefix: &str) -> Vec<Candidate<'static>> {
    let mut names: HashMap<String, usize> = HashMap::new();
    for hit in &hits {
        *names.entry(hit.tool.clone()).or_insert(0) += 1;
    }
    hits.into_iter()
        .map(|hit| {
            let ambiguous = names.get(&hit.tool).copied().unwrap_or(0) > 1;
            let value = if prefix.is_empty() {
                if ambiguous {
                    hit.reference.clone()
                } else {
                    hit.tool.clone()
                }
            } else if hit.tool.starts_with(prefix) {
                hit.tool.clone()
            } else if hit.reference.starts_with(prefix) {
                hit.reference.clone()
            } else if let Some(alias) = hit.aliases.iter().find(|alias| alias.starts_with(prefix)) {
                alias.clone()
            } else {
                hit.tool.clone()
            };
            let mut candidate = Candidate::new(value);
            if !hit.description.is_empty() {
                candidate.description = Some(hit.description.into());
            }
            candidate
        })
        .collect()
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

    #[test]
    fn completion_values_are_tool_names() {
        let candidates = into_candidates(
            vec![
                hit("cli/cli/gh", "gh", "cli/cli", &[]),
                hit("eza-community/eza", "eza", "eza-community/eza", &[]),
            ],
            "",
        );
        assert_eq!(candidates[0].value, "gh");
        assert_eq!(candidates[1].value, "eza");
    }

    #[test]
    fn ambiguous_tool_names_fall_back_to_references() {
        let candidates = into_candidates(
            vec![
                hit("one/thing/tool", "tool", "one/thing", &[]),
                hit("two/thing/tool", "tool", "two/thing", &[]),
            ],
            "",
        );
        assert_eq!(candidates[0].value, "one/thing/tool");
        assert_eq!(candidates[1].value, "two/thing/tool");
    }

    #[test]
    fn a_reference_prefix_offers_the_reference() {
        let candidates = into_candidates(
            vec![
                hit("cli/cli/gh", "gh", "cli/cli", &[]),
                hit("eza-community/eza", "eza", "eza-community/eza", &[]),
            ],
            "cli/",
        );
        assert_eq!(candidates[0].value, "cli/cli/gh");
        assert_eq!(candidates[1].value, "eza");
    }

    #[test]
    fn an_alias_prefix_offers_the_alias() {
        let candidates =
            into_candidates(vec![hit("cli/cli/gh", "gh", "cli/cli", &["github"])], "git");
        assert_eq!(candidates[0].value, "github");
    }
}
