//! Registry items: workspaces, their tools, and their completion sources.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{Config, Selection, validate_command_name};
use crate::reference::{Host, Reference, Repo, normalize_repo};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryItem {
    /// The workspace this item describes, such as `cli/cli`, a GitHub URL, or
    /// an SSH URL. Absent for a private item that is addressable only by tool
    /// name.
    #[serde(default)]
    pub repo: Option<String>,

    /// The tool `owner/repo` resolves to. Required when the item has more
    /// than one tool.
    #[serde(default)]
    pub main: Option<String>,

    #[serde(default)]
    pub description: Option<String>,

    pub tools: Vec<RegistryTool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryTool {
    /// The executable name.
    pub name: String,

    /// Other names typed at the prompt that should receive this completion.
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Completion sources, keyed by shell.
    #[serde(default)]
    pub completions: IndexMap<String, Source>,
}

impl RegistryTool {
    pub fn source(&self, shell: &str) -> Option<&Source> {
        self.completions.get(shell)
    }

    pub fn command_names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Native,
    /// The script is zsh output that relies on `bashcompinit`.
    Bashcomp,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// argv to run. Executed without a shell.
    #[serde(default)]
    pub run: Option<Vec<String>>,

    /// A completion file to read, usually inside the tool's install root.
    #[serde(default)]
    pub path: Option<String>,

    /// A completion file to fetch at install time.
    #[serde(default)]
    pub url: Option<String>,

    /// Hex SHA-256 of the fetched file. Optional for a version-pinned URL,
    /// whose cache records the hash it fetched; required for a static URL in
    /// the embedded registry.
    #[serde(default)]
    pub sha256: Option<String>,

    /// Environment variables for `run`, rendered as templates.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    #[serde(default)]
    pub kind: SourceKind,

    /// Per-source timeout, overriding `[settings].timeout`.
    #[serde(default)]
    pub timeout: Option<String>,
}

impl Source {
    pub fn validate(&self, embedded: bool) -> Result<()> {
        let count = [self.run.is_some(), self.path.is_some(), self.url.is_some()]
            .into_iter()
            .filter(|present| *present)
            .count();
        if count != 1 {
            bail!("a completion source must have exactly one of `run`, `path`, or `url`");
        }
        if let Some(run) = &self.run
            && run.is_empty()
        {
            bail!("`run` must not be empty");
        }
        if let Some(path) = &self.path
            && path.trim().is_empty()
        {
            bail!("`path` must not be empty");
        }
        if let Some(url) = &self.url {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                bail!("`url` must be an HTTP(S) URL");
            }
            match self.sha256.as_deref() {
                Some(checksum) => validate_sha256(checksum)?,
                None if embedded && !url.contains("{{ version }}") => {
                    bail!(
                        "an embedded `url` source needs `sha256` or a `{{{{ version }}}}` template"
                    )
                }
                None => {}
            }
        } else if self.sha256.is_some() {
            bail!("`sha256` only applies to `url` sources");
        }
        if let Some(timeout) = &self.timeout {
            crate::config::parse_duration(timeout)?;
        }
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("`sha256` must be 64 hexadecimal characters");
    }
    Ok(())
}

pub fn validate_item(item: &RegistryItem, embedded: bool) -> Result<()> {
    if item.tools.is_empty() {
        bail!("a registry item needs at least one tool");
    }
    let mut names = BTreeSet::new();
    for tool in &item.tools {
        validate_command_name(&tool.name)?;
        let mut command_names = BTreeSet::new();
        for name in tool.command_names() {
            validate_command_name(name)?;
            if !command_names.insert(name.to_string()) {
                bail!("tool `{}` lists `{name}` more than once", tool.name);
            }
        }
        for name in tool.command_names() {
            if !names.insert(name.to_string()) {
                bail!("`{name}` is declared more than once in this item");
            }
        }
        for (shell, source) in &tool.completions {
            crate::config::validate_shell(shell)?;
            source
                .validate(embedded)
                .wrap_err_with(|| format!("invalid `{shell}` source for `{}`", tool.name))?;
        }
    }
    if let Some(main) = &item.main
        && !item.tools.iter().any(|tool| tool.name == *main)
    {
        bail!("`main = \"{main}\"` does not name a tool in this item");
    }
    if item.repo.is_none() && item.main.is_some() {
        bail!("`main` requires a `repo`");
    }
    if let Some(repo) = &item.repo {
        normalize_repo(repo)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub item: RegistryItem,
    pub repo: Option<Repo>,
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub reference: String,
    pub repo: Option<Repo>,
    pub item: RegistryItem,
    pub tool: RegistryTool,
    pub source_path: Option<PathBuf>,
}

impl ResolvedTool {
    pub fn repo_slug(&self) -> Option<String> {
        self.repo.as_ref().map(Repo::slug)
    }
}

type ItemWithSource = (RegistryItem, Option<PathBuf>);
type Layer<'a> = (&'a str, Vec<ItemWithSource>);

#[derive(Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
    by_tool: BTreeMap<String, Vec<usize>>,
    by_repo: BTreeMap<String, usize>,
    main: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct SelectedTool {
    pub reference: String,
    pub resolved: ResolvedTool,
    pub aliases: Vec<String>,
    pub shells: Option<Vec<String>>,
}

impl Registry {
    pub fn load(config: &Config) -> Result<Self> {
        Self::load_with(config, None)
    }

    pub fn load_with(config: &Config, registry_dir: Option<&Path>) -> Result<Self> {
        let mut layers: Vec<Layer> = Vec::new();

        let mut embedded = Vec::new();
        for (name, text) in embedded::EMBEDDED {
            let item: RegistryItem = toml::from_str(text)
                .wrap_err_with(|| format!("invalid embedded registry item `{name}`"))?;
            validate_item(&item, true)
                .wrap_err_with(|| format!("invalid embedded registry item `{name}`"))?;
            embedded.push((item, None));
        }
        layers.push(("embedded", embedded));

        let mut user = Vec::new();
        let registry_dir = match registry_dir {
            Some(path) => path.to_path_buf(),
            None => crate::dirs::registry_dir()?,
        };
        if registry_dir.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&registry_dir)
                .wrap_err_with(|| format!("cannot read registry `{}`", registry_dir.display()))?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<_>>()?;
            files.retain(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"));
            files.sort();
            for path in files {
                let text = std::fs::read_to_string(&path)
                    .wrap_err_with(|| format!("cannot read registry item `{}`", path.display()))?;
                let item: RegistryItem = toml::from_str(&text)
                    .wrap_err_with(|| format!("invalid registry item `{}`", path.display()))?;
                validate_item(&item, false)
                    .wrap_err_with(|| format!("invalid registry item `{}`", path.display()))?;
                user.push((item, Some(path)));
            }
        }
        layers.push(("user registry", user));

        let inline = config
            .registry
            .iter()
            .cloned()
            .map(|item| (item, config.source.clone()))
            .collect::<Vec<_>>();
        layers.push(("inline registry", inline));

        let mut merged: IndexMap<String, Entry> = IndexMap::new();
        for (layer_name, items) in layers {
            let mut seen = BTreeSet::new();
            for (item, source_path) in items {
                validate_item(&item, layer_name == "embedded")?;
                let repo = match &item.repo {
                    Some(repo) => Some(normalize_repo(repo)?),
                    None => None,
                };
                let identity = repo
                    .as_ref()
                    .map(Repo::slug)
                    .unwrap_or_else(|| format!("tool:{}", item.tools[0].name));
                if !seen.insert(identity.clone()) {
                    bail!("duplicate registry item `{identity}` in {layer_name}");
                }
                if merged.contains_key(&identity) {
                    eprintln!("shcomp: warning: {layer_name} overrides registry item `{identity}`");
                }
                merged.insert(
                    identity,
                    Entry {
                        item,
                        repo,
                        source_path,
                    },
                );
            }
        }

        let entries: Vec<Entry> = merged.into_values().collect();
        let mut built = Self {
            entries,
            ..Default::default()
        };
        built.index()?;
        Ok(built)
    }

    fn index(&mut self) -> Result<()> {
        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(repo) = &entry.repo {
                if self.by_repo.insert(repo.slug(), index).is_some() {
                    bail!("duplicate workspace `{}`", repo.slug());
                }
                if let Some(main) = &entry.item.main {
                    let tool_index = entry
                        .item
                        .tools
                        .iter()
                        .position(|tool| tool.name == *main)
                        .expect("validated main");
                    self.main.insert(repo.slug(), tool_index);
                }
            }
            for tool in &entry.item.tools {
                for name in tool.command_names() {
                    self.by_tool
                        .entry(name.to_string())
                        .or_default()
                        .push(index);
                }
            }
        }
        Ok(())
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn resolve(&self, reference: &Reference) -> Result<ResolvedTool> {
        if let Some(Host::Other(host)) = &reference.host {
            bail!("unsupported host `{host}`; only GitHub workspaces are recognized");
        }

        if let Some(repo) = &reference.repo {
            let slug = repo.slug();
            let Some(&item_index) = self.by_repo.get(&slug) else {
                bail!("no registry item for `{slug}`; add one or choose another reference");
            };
            let entry = &self.entries[item_index];
            let tool_index = match &reference.tool {
                Some(name) => entry
                    .item
                    .tools
                    .iter()
                    .position(|tool| tool.command_names().any(|candidate| candidate == name))
                    .ok_or_else(|| {
                        eyre::eyre!(
                            "`{slug}` has no tool `{name}`; known tools: {}",
                            entry
                                .item
                                .tools
                                .iter()
                                .map(|tool| tool.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?,
                None => {
                    if let Some(&main) = self.main.get(&slug) {
                        main
                    } else if entry.item.tools.len() == 1 {
                        0
                    } else {
                        bail!(
                            "`{slug}` has several tools and no `main`; use one of: {}",
                            entry
                                .item
                                .tools
                                .iter()
                                .map(|tool| format!("{slug}/{}", tool.name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            };
            return Ok(ResolvedTool {
                reference: format!("{slug}/{}", entry.item.tools[tool_index].name),
                repo: Some(repo.clone()),
                item: entry.item.clone(),
                tool: entry.item.tools[tool_index].clone(),
                source_path: entry.source_path.clone(),
            });
        }

        let Some(name) = &reference.tool else {
            bail!("`{}` does not identify a tool", reference.raw);
        };
        let Some(matches) = self.by_tool.get(name) else {
            bail!("no registry item provides `{name}`");
        };
        let unique: BTreeSet<usize> = matches.iter().copied().collect();
        if unique.len() > 1 {
            let candidates = unique
                .iter()
                .map(|index| {
                    let entry = &self.entries[*index];
                    match &entry.repo {
                        Some(repo) => format!("{}/{}", repo.slug(), name),
                        None => name.clone(),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            bail!("`{name}` is ambiguous; use one of: {candidates}");
        }
        let item_index = *unique.first().expect("non-empty matches");
        let entry = &self.entries[item_index];
        let tool_index = entry
            .item
            .tools
            .iter()
            .position(|tool| tool.command_names().any(|candidate| candidate == name))
            .expect("indexed tool");
        Ok(ResolvedTool {
            reference: match &entry.repo {
                Some(repo) => format!("{}/{}", repo.slug(), entry.item.tools[tool_index].name),
                None => entry.item.tools[tool_index].name.clone(),
            },
            repo: entry.repo.clone(),
            item: entry.item.clone(),
            tool: entry.item.tools[tool_index].clone(),
            source_path: entry.source_path.clone(),
        })
    }

    pub fn resolve_str(&self, reference: &str) -> Result<ResolvedTool> {
        self.resolve(&Reference::parse(reference)?)
    }

    pub fn selected(&self, config: &Config) -> Result<Vec<SelectedTool>> {
        let mut selected = Vec::new();
        for (reference, selection) in &config.tools {
            if selection.disabled() {
                continue;
            }
            let resolved = self
                .resolve_str(reference)
                .wrap_err_with(|| format!("cannot resolve `{reference}`"))?;
            let mut aliases: Vec<String> = resolved.tool.aliases.clone();
            for alias in selection.aliases() {
                if !aliases.contains(alias) {
                    aliases.push(alias.clone());
                }
            }
            let shells = match selection {
                Selection::Options(options) => options.shells.clone(),
                Selection::Enabled(_) => None,
            };
            selected.push(SelectedTool {
                reference: reference.clone(),
                resolved,
                aliases,
                shells,
            });
        }
        Ok(selected)
    }
}

/// Read one registry item from a file. Useful for tests and for
/// `shcomp registry check`.
pub fn read_item(path: &Path) -> Result<RegistryItem> {
    let text = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("cannot read `{}`", path.display()))?;
    let item: RegistryItem =
        toml::from_str(&text).wrap_err_with(|| format!("invalid `{}`", path.display()))?;
    validate_item(&item, false)?;
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(items: &[&str]) -> Registry {
        let config = Config {
            registry: items
                .iter()
                .map(|item| toml::from_str(item).unwrap())
                .collect(),
            ..Config::default()
        };
        Registry::load(&config).unwrap()
    }

    const CLI_CLI: &str = r#"
repo = "cli/cli"
main = "gh"
description = "GitHub CLI"

[[tools]]
name = "gh"
aliases = ["github"]

[tools.completions.zsh]
run = ["{{ bin }}", "completion", "-s", "{{ shell }}"]
"#;

    #[test]
    fn resolves_every_reference_form() {
        let registry = registry(&[CLI_CLI]);
        for input in [
            "gh",
            "github",
            "cli/cli",
            "cli/cli/gh",
            "https://github.com/cli/cli",
            "git@github.com:cli/cli.git",
            "gh:cli/cli/gh",
        ] {
            let resolved = registry.resolve_str(input).unwrap();
            assert_eq!(resolved.tool.name, "gh", "input: {input}");
        }
    }

    #[test]
    fn rejects_ambiguous_bare_names() {
        let other = r#"
repo = "example/gh"
[[tools]]
name = "gh"
[tools.completions.zsh]
run = ["gh", "completion"]
"#;
        let registry = registry(&[CLI_CLI, other]);
        let error = registry.resolve_str("gh").unwrap_err().to_string();
        assert!(error.contains("ambiguous"), "{error}");
        assert!(registry.resolve_str("example/gh").is_ok());
    }

    #[test]
    fn workspace_without_main_needs_a_tool() {
        let item = r#"
repo = "example/tools"
[[tools]]
name = "one"
[tools.completions.zsh]
run = ["one"]
[[tools]]
name = "two"
[tools.completions.zsh]
run = ["two"]
"#;
        let registry = registry(&[item]);
        assert!(registry.resolve_str("example/tools").is_err());
        assert_eq!(
            registry.resolve_str("example/tools/two").unwrap().tool.name,
            "two"
        );
    }

    #[test]
    fn url_sources_require_checksums_when_embedded() {
        let item = r#"
repo = "example/tool"
[[tools]]
name = "tool"
[tools.completions.zsh]
url = "https://example.com/_tool"
"#;
        let item: RegistryItem = toml::from_str(item).unwrap();
        let error = format!("{:?}", validate_item(&item, true).unwrap_err());
        assert!(error.contains("sha256"), "{error}");
    }

    #[test]
    fn selected_applies_config_aliases() {
        let registry = registry(&[CLI_CLI]);
        let config: Config = toml::from_str(
            r#"
[tools]
"cli/cli" = { aliases = ["ghx"] }
"#,
        )
        .unwrap();
        let selected = registry.selected(&config).unwrap();
        assert_eq!(selected[0].aliases, ["github", "ghx"]);
    }
}

#[cfg(test)]
mod embedded_tests {
    use super::*;

    #[test]
    fn every_embedded_item_validates_and_has_zsh() {
        assert!(!embedded::EMBEDDED.is_empty());
        let mut entries = Vec::new();
        for (name, text) in embedded::EMBEDDED {
            let item: RegistryItem =
                toml::from_str(text).unwrap_or_else(|error| panic!("{name}: {error}"));
            validate_item(&item, true).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            for tool in &item.tools {
                assert!(
                    tool.completions.contains_key("zsh"),
                    "{name}/{} has no zsh source",
                    tool.name
                );
            }
            let repo = item
                .repo
                .as_deref()
                .map(normalize_repo)
                .transpose()
                .unwrap();
            entries.push(Entry {
                item,
                repo,
                source_path: None,
            });
        }
        let mut registry = Registry {
            entries,
            ..Default::default()
        };
        registry.index().unwrap();

        for reference in [
            "gh",
            "cli/cli",
            "cli/cli/gh",
            "chezmoi",
            "uv",
            "cargo",
            "android",
            "tv",
            "zellij",
            "herdr",
            "bun",
            "eza",
            "gradle",
            "just",
        ] {
            registry
                .resolve_str(reference)
                .unwrap_or_else(|error| panic!("{reference}: {error}"));
        }
    }
}
