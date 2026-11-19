//! Registry items described on the `add` command line.
//!
//! `shcomp add leaf --run 'leaf --auto-complete {shell}:dump'` describes a
//! tool no registry knows. The flags become one `[[registry]]` item in the
//! user's config (ADR-0003 allows inline items), written as TOML a person
//! would have written by hand.

use eyre::{Result, bail};

use crate::registry::RegistryItem;

/// The source flags `add` accepts for one tool.
#[derive(Debug, Default, Clone)]
pub struct InlineSpec {
    pub name: String,
    pub description: Option<String>,
    /// `--run`: the shared default for every declared shell.
    pub run: Option<String>,
    /// `--usage`: a command printing a usage KDL spec, shared like `--run`.
    pub usage: Option<String>,
    pub run_zsh: Option<String>,
    pub run_fish: Option<String>,
    /// `--url` with the one `--shell` it is for.
    pub url: Option<String>,
    /// `--shell`: the shells to declare; every installed shell when empty.
    pub shells: Vec<String>,
}

impl InlineSpec {
    pub fn is_inline(&self) -> bool {
        self.run.is_some()
            || self.usage.is_some()
            || self.run_zsh.is_some()
            || self.run_fish.is_some()
            || self.url.is_some()
    }

    /// The `[[registry]]` item as TOML, and the item it parses to.
    ///
    /// `default_shells` are the shells a shared source covers when `--shell`
    /// is not given: the installed ones.
    pub fn render(&self, default_shells: &[String]) -> Result<(String, RegistryItem)> {
        crate::config::validate_command_name(&self.name)?;
        if self.run.is_some() && self.usage.is_some() {
            bail!("`--run` and `--usage` both describe the shared source; pick one");
        }
        for shell in &self.shells {
            if !crate::shell::managed(shell) {
                bail!("`{shell}` is not a shell shcomp can install for");
            }
        }
        if self.url.is_some() && self.shells.len() != 1 {
            bail!("`--url` needs exactly one `--shell`: a completion file is for one shell");
        }

        let shared = match (&self.run, &self.usage) {
            (Some(run), _) => Some((argv(&self.name, run)?, false)),
            (_, Some(usage)) => Some((argv(&self.name, usage)?, true)),
            _ => None,
        };
        let mut overrides: Vec<(&str, String)> = Vec::new();
        if let Some(run) = &self.run_zsh {
            overrides.push(("zsh", format!("run = {}", array(&argv(&self.name, run)?))));
        }
        if let Some(run) = &self.run_fish {
            overrides.push(("fish", format!("run = {}", array(&argv(&self.name, run)?))));
        }
        if let Some(url) = &self.url {
            let shell = self.shells[0].as_str();
            if overrides.iter().any(|(existing, _)| *existing == shell) {
                bail!("`--url` and `--run-{shell}` both describe the {shell} source");
            }
            overrides.push((shell, format!("url = {}", string(url))));
        }

        // The shells the item declares: `--shell` when given, otherwise the
        // installed shells for a shared source plus each per-shell flag's.
        let mut shells: Vec<String> = Vec::new();
        let mut declare = |shell: &str| {
            if !shells.iter().any(|existing| existing == shell) {
                shells.push(shell.to_string());
            }
        };
        if !self.shells.is_empty() && self.url.is_none() {
            self.shells.iter().for_each(|shell| declare(shell));
        } else {
            if shared.is_some() {
                default_shells
                    .iter()
                    .filter(|shell| crate::shell::managed(shell))
                    .for_each(|shell| declare(shell));
            }
            overrides.iter().for_each(|(shell, _)| declare(shell));
        }
        shells.sort_by_key(|shell| ["zsh", "fish"].iter().position(|known| known == shell));
        if shells.is_empty() {
            bail!("no shell to complete `{}` in", self.name);
        }

        let mut text = String::from("[[registry]]\n");
        if let Some(description) = &self.description {
            text.push_str(&format!("description = {}\n", string(description)));
        }
        text.push_str(&format!(
            "\n[[registry.tools]]\nname = {}\nshells = {}\n",
            string(&self.name),
            array(&shells)
        ));
        if let Some((argv, usage)) = &shared {
            text.push_str(&format!(
                "\n[registry.tools.completions]\nrun = {}\n",
                array(argv)
            ));
            if *usage {
                text.push_str("kind = \"usage\"\n");
            }
        }
        for (shell, line) in &overrides {
            text.push_str(&format!("\n[registry.tools.completions.{shell}]\n{line}\n"));
        }

        let item: RegistryItem = toml::from_str(
            &text
                .replacen("[[registry]]\n", "", 1)
                .replace("[[registry.tools]]", "[[tools]]")
                .replace("[registry.tools.completions", "[tools.completions"),
        )
        .map_err(|error| eyre::eyre!("cannot describe `{}`: {error}", self.name))?;
        crate::registry::validate_item(&item, false)?;
        Ok((text, item))
    }
}

/// Split a flag value like a shell would, then turn the two placeholders into
/// templates. A leading word naming the tool becomes `{{ bin }}`, so the
/// command runs the executable the shell resolved (ADR-0002), not whichever
/// one is first on `PATH` at load time.
fn argv(name: &str, line: &str) -> Result<Vec<String>> {
    let words = split_words(line)?;
    if words.is_empty() {
        bail!("an empty command describes no source");
    }
    Ok(words
        .into_iter()
        .enumerate()
        .map(|(index, word)| {
            if index == 0 && word == name {
                "{{ bin }}".to_string()
            } else {
                word.replace("{shell}", "{{ shell }}")
                    .replace("{bin}", "{{ bin }}")
            }
        })
        .collect())
}

/// POSIX-shell word splitting for one flag value: whitespace separates words,
/// single quotes are literal, double quotes allow `\"` and `\\`, and a
/// backslash outside quotes escapes the next character. No expansion.
pub fn split_words(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => bail!("unterminated single quote in `{line}`"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => bail!("unterminated double quote in `{line}`"),
                        },
                        Some(c) => word.push(c),
                        None => bail!("unterminated double quote in `{line}`"),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => bail!("trailing backslash in `{line}`"),
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

fn string(value: &str) -> String {
    toml_edit::Value::from(value).to_string().trim().to_string()
}

fn array(values: &[String]) -> String {
    let parts: Vec<String> = values.iter().map(|value| string(value)).collect();
    format!("[{}]", parts.join(", "))
}

/// Add or replace the inline item for `name` in the config text.
///
/// Only a repo-less inline item whose tool is `name` is replaced: that is the
/// one an earlier `add --run` wrote. Everything else in the file is kept as it
/// was, comments included.
pub fn splice(text: &str, name: &str, item_toml: &str) -> Result<String> {
    let mut document: toml_edit::DocumentMut = if text.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        text.parse()
            .map_err(|error| eyre::eyre!("cannot parse the config as TOML: {error}"))?
    };
    if let Some(items) = document
        .get_mut("registry")
        .and_then(|item| item.as_array_of_tables_mut())
    {
        items.retain(|table| !inline_item_for(table, name));
        if items.is_empty() {
            document.remove("registry");
        }
    }
    let mut out = document.to_string();
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(item_toml);
    Ok(out)
}

/// Whether a `[[registry]]` table is the repo-less inline item for `name`.
pub fn inline_item_for(table: &toml_edit::Table, name: &str) -> bool {
    !table.contains_key("repo")
        && table
            .get("tools")
            .and_then(|tools| tools.as_array_of_tables())
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|tool| tool.get("name").and_then(|value| value.as_str()) == Some(name))
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shells() -> Vec<String> {
        vec!["zsh".to_string(), "fish".to_string()]
    }

    #[test]
    fn words_split_like_a_shell() {
        assert_eq!(
            split_words(r#"leaf --auto-complete {shell}:dump"#).unwrap(),
            ["leaf", "--auto-complete", "{shell}:dump"]
        );
        assert_eq!(
            split_words(r#"a 'b c' "d \"e\"" f\ g"#).unwrap(),
            ["a", "b c", "d \"e\"", "f g"]
        );
        assert_eq!(split_words("  ").unwrap(), Vec::<String>::new());
        assert_eq!(split_words("''").unwrap(), [""]);
        assert!(split_words("'open").is_err());
    }

    #[test]
    fn a_run_flag_becomes_a_shared_source() {
        let spec = InlineSpec {
            name: "leaf".to_string(),
            run: Some("leaf --auto-complete {shell}:dump".to_string()),
            ..InlineSpec::default()
        };
        let (text, item) = spec.render(&shells()).unwrap();
        assert_eq!(
            text,
            r#"[[registry]]

[[registry.tools]]
name = "leaf"
shells = ["zsh", "fish"]

[registry.tools.completions]
run = ["{{ bin }}", "--auto-complete", "{{ shell }}:dump"]
"#
        );
        let tool = &item.tools[0];
        assert_eq!(
            tool.source("fish").unwrap().run.unwrap(),
            ["{{ bin }}", "--auto-complete", "{{ shell }}:dump"]
        );
    }

    #[test]
    fn per_shell_flags_declare_only_their_shells() {
        let spec = InlineSpec {
            name: "leaf".to_string(),
            description: Some("Leaf \"CLI\"".to_string()),
            run_fish: Some("leaf --auto-complete fish:dump".to_string()),
            ..InlineSpec::default()
        };
        let (text, item) = spec.render(&shells()).unwrap();
        assert!(
            text.contains("description = 'Leaf \"CLI\"'")
                || text.contains(r#"description = "Leaf \"CLI\"""#),
            "{text}"
        );
        assert!(text.contains("shells = [\"fish\"]"), "{text}");
        assert!(text.contains("[registry.tools.completions.fish]"), "{text}");
        assert!(item.tools[0].source("zsh").is_none());
    }

    #[test]
    fn a_url_needs_its_shell_and_usage_sets_the_kind() {
        let url = InlineSpec {
            name: "leaf".to_string(),
            url: Some("https://example.com/_leaf".to_string()),
            ..InlineSpec::default()
        };
        assert!(url.render(&shells()).is_err());
        let url = InlineSpec {
            shells: vec!["zsh".to_string()],
            ..url
        };
        let (_, item) = url.render(&shells()).unwrap();
        assert_eq!(
            item.tools[0].source("zsh").unwrap().url.as_deref(),
            Some("https://example.com/_leaf")
        );

        let usage = InlineSpec {
            name: "leaf".to_string(),
            usage: Some("leaf usage".to_string()),
            shells: vec!["zsh".to_string()],
            ..InlineSpec::default()
        };
        let (text, item) = usage.render(&shells()).unwrap();
        assert!(text.contains("kind = \"usage\""), "{text}");
        assert_eq!(item.tools[0].shells, ["zsh"]);
    }

    #[test]
    fn splice_replaces_only_the_earlier_inline_item() {
        let config = r#"# mine
[tools]
leaf = true

[[registry]]
repo = "example/other"

[[registry.tools]]
name = "leaf"

[[registry]]

[[registry.tools]]
name = "leaf"
shells = ["zsh"]
"#;
        let out = splice(
            config,
            "leaf",
            "[[registry]]\n\n[[registry.tools]]\nname = \"leaf\"\nshells = [\"fish\"]\n",
        )
        .unwrap();
        assert!(out.starts_with("# mine\n[tools]\nleaf = true\n"), "{out}");
        assert!(out.contains("repo = \"example/other\""), "{out}");
        assert!(!out.contains("shells = [\"zsh\"]"), "{out}");
        assert!(out.ends_with("shells = [\"fish\"]\n"), "{out}");
        let parsed: crate::config::Config = toml::from_str(&out).unwrap();
        assert_eq!(parsed.registry.len(), 2);
    }
}
