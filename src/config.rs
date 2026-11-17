//! The user config: which tools are wanted, never how to complete them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{RegistryItem, SourceKind};

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Tools to complete, keyed by any accepted reference (`gh`,
    /// `cli/cli`, `cli/cli/gh`, a repository URL, or `gh:owner/repo/tool`).
    #[serde(default)]
    pub tools: IndexMap<String, Selection>,

    #[serde(default)]
    pub settings: Settings,

    /// Registry items declared inline, using the registry-item schema.
    #[serde(default, rename = "registry")]
    pub registry: Vec<RegistryItem>,

    /// The file this config was loaded from, when it was loaded from disk.
    #[serde(skip)]
    #[schemars(skip)]
    pub source: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Selection {
    Enabled(bool),
    Options(SelectionOptions),
}

impl Selection {
    pub fn disabled(&self) -> bool {
        match self {
            Self::Enabled(enabled) => !*enabled,
            Self::Options(options) => options.disabled,
        }
    }

    pub fn aliases(&self) -> &[String] {
        match self {
            Self::Enabled(_) => &[],
            Self::Options(options) => &options.aliases,
        }
    }

    pub fn shells(&self) -> Option<&[String]> {
        match self {
            Self::Enabled(_) => None,
            Self::Options(options) => options.shells.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionOptions {
    /// Extra command names that should receive this tool's completion.
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Keep the selection but install nothing for it.
    #[serde(default)]
    pub disabled: bool,

    /// Restrict this selection to these shells. Absent means every shell the
    /// tool supports.
    #[serde(default)]
    pub shells: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Timeout for one generator run. Accepts `ms`, `s`, or `m` suffixes.
    #[serde(default = "Settings::default_timeout")]
    pub timeout: String,

    /// Whether `install` may fetch `url` sources.
    #[serde(default = "Settings::default_fetch")]
    pub fetch: bool,

    /// Override the cache directory.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    /// Override the data directory (where shell shims are installed).
    #[serde(default)]
    pub data_dir: Option<PathBuf>,

    /// Environment passed to generators. The process environment is used when
    /// this is absent.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// The `usage` executable that renders `kind = "usage"` sources.
    #[serde(default = "Settings::default_usage_bin")]
    pub usage_bin: String,

    /// Shells `install` manages. Defaults to every shell shcomp supports.
    #[serde(default = "Settings::default_shells")]
    pub shells: Vec<String>,

    /// Tracing directives, as `SHCOMP_LOG` takes them. `SHCOMP_LOG` wins.
    #[serde(default)]
    pub log: Option<String>,

    /// Append the same events, without color, to this file. `SHCOMP_LOG_FILE`
    /// wins.
    #[serde(default)]
    pub log_file: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timeout: Self::default_timeout(),
            fetch: Self::default_fetch(),
            cache_dir: None,
            data_dir: None,
            env: BTreeMap::new(),
            usage_bin: Self::default_usage_bin(),
            shells: Self::default_shells(),
            log: None,
            log_file: None,
        }
    }
}

impl Settings {
    fn default_timeout() -> String {
        "5s".to_string()
    }

    fn default_fetch() -> bool {
        true
    }

    fn default_usage_bin() -> String {
        "usage".to_string()
    }

    fn default_shells() -> Vec<String> {
        vec!["zsh".to_string(), "fish".to_string()]
    }

    pub fn timeout(&self) -> Result<std::time::Duration> {
        parse_duration(&self.timeout)
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("cannot read config `{}`", path.display()))?;
        let mut config: Self = toml::from_str(&text)
            .wrap_err_with(|| format!("invalid config `{}`", path.display()))?;
        config.source = Some(path.to_path_buf());
        config.validate()?;
        Ok(config)
    }

    /// Load the requested config for a command.
    ///
    /// Memoized on success: `main` reads it once to configure logging, and the
    /// command that runs next must not pay a second parse — `shcomp load` is
    /// the hot path a shell forks. A failed load is not cached, so a later
    /// command still reports the error in full.
    pub fn load_default() -> Result<Self> {
        static CACHED: std::sync::OnceLock<Config> = std::sync::OnceLock::new();
        if let Some(config) = CACHED.get() {
            return Ok(config.clone());
        }
        let path = crate::dirs::config_file()?;
        let config = if path.exists() {
            Self::load(&path)?
        } else {
            Self::default()
        };
        let _ = CACHED.set(config.clone());
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        for (reference, selection) in &self.tools {
            if reference.trim().is_empty() {
                bail!("config contains an empty tool reference");
            }
            if let Selection::Options(options) = selection {
                for alias in &options.aliases {
                    validate_command_name(alias)
                        .wrap_err_with(|| format!("invalid alias `{alias}` for `{reference}`"))?;
                }
                for shell in options.shells.iter().flatten() {
                    validate_shell(shell)
                        .wrap_err_with(|| format!("invalid shell `{shell}` for `{reference}`"))?;
                }
            }
        }

        for (index, item) in self.registry.iter().enumerate() {
            crate::registry::validate_item(item, false)
                .wrap_err_with(|| format!("invalid inline registry item #{index}"))?;
        }

        if self.settings.usage_bin.trim().is_empty() {
            bail!("`settings.usage_bin` must not be empty");
        }
        for shell in &self.settings.shells {
            validate_shell(shell)
                .wrap_err_with(|| format!("invalid `settings.shells` entry `{shell}`"))?;
            if !crate::shell::managed(shell) {
                bail!("`{}` is not a shell shcomp can install for", shell);
            }
        }
        self.settings.timeout()?;
        Ok(())
    }

    /// The directory templates in this config resolve relative paths from.
    pub fn config_root(&self) -> PathBuf {
        self.source
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }
}

pub fn parse_duration(value: &str) -> Result<std::time::Duration> {
    let value = value.trim();
    let (number, unit) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1u64)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1_000)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60_000)
    } else {
        bail!("`{value}` is not a duration; use a suffix such as `5s` or `500ms`");
    };
    let number: u64 = number
        .trim()
        .parse()
        .wrap_err_with(|| format!("`{value}` is not a duration"))?;
    if number == 0 {
        bail!("timeout must be greater than zero");
    }
    Ok(std::time::Duration::from_millis(number * unit))
}

pub fn validate_command_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        bail!("command name is empty");
    };
    if !(first.is_ascii_alphanumeric() || first == '_') {
        bail!("command name must start with a letter, digit, or underscore");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
    {
        bail!("command name contains an unsupported character");
    }
    Ok(())
}

pub fn validate_shell(shell: &str) -> Result<()> {
    match shell {
        "zsh" | "bash" | "fish" | "nu" | "nushell" | "powershell" | "pwsh" => Ok(()),
        other => bail!("unsupported shell `{other}`"),
    }
}

pub fn validate_source_kind(kind: SourceKind) -> Result<()> {
    match kind {
        SourceKind::Native | SourceKind::Bashcomp | SourceKind::Usage => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boolean_and_table_selections() {
        let config: Config = toml::from_str(
            r#"
[tools]
gh = true
"cli/cli" = { aliases = ["github"] }
"#,
        )
        .unwrap();
        assert!(!config.tools["gh"].disabled());
        assert_eq!(config.tools["cli/cli"].aliases(), ["github"]);
    }

    #[test]
    fn parses_inline_registry_items() {
        let config: Config = toml::from_str(
            r#"
[tools]
acme = true

[[registry]]
repo = "acme/tool"
main = "acme"
description = "A private tool"

[[registry.tools]]
name = "acme"
aliases = ["at"]

[registry.tools.completions.zsh]
run = ["{{ bin }}", "completion", "{{ shell }}"]
"#,
        )
        .unwrap();
        assert_eq!(config.registry.len(), 1);
        assert_eq!(config.registry[0].tools[0].name, "acme");
    }

    #[test]
    fn duration_units() {
        assert_eq!(parse_duration("5s").unwrap().as_millis(), 5_000);
        assert_eq!(parse_duration("250ms").unwrap().as_millis(), 250);
        assert_eq!(parse_duration("1m").unwrap().as_millis(), 60_000);
        assert!(parse_duration("5").is_err());
        assert!(parse_duration("0s").is_err());
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("[settings]\nunknown = true\n").is_err());
    }
}

/// Add one selection to a config document, preserving comments and layout.
pub fn add_selection(
    text: &str,
    reference: &str,
    aliases: &[String],
    replace_aliases: bool,
) -> Result<String> {
    for alias in aliases {
        validate_command_name(alias)?;
    }
    let is_new = text.trim().is_empty();
    let mut document: toml_edit::DocumentMut = if is_new {
        toml_edit::DocumentMut::new()
    } else {
        text.parse().wrap_err("cannot parse the config as TOML")?
    };

    if !document.contains_key("tools") {
        let mut tools = toml_edit::Table::new();
        tools.set_implicit(false);
        document.insert("tools", toml_edit::Item::Table(tools));
    }
    let tools = document["tools"]
        .as_table_mut()
        .ok_or_else(|| eyre::eyre!("`tools` must be a table"))?;

    let existing_aliases = tools
        .get(reference)
        .and_then(|item| item.as_table_like())
        .and_then(|table| table.get("aliases"))
        .and_then(|item| item.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut merged = if replace_aliases {
        Vec::new()
    } else {
        existing_aliases
    };
    for alias in aliases {
        if !merged.contains(alias) {
            merged.push(alias.clone());
        }
    }

    if merged.is_empty() {
        // Keep an existing table intact if it had other options.
        if tools
            .get(reference)
            .and_then(|item| item.as_table_like())
            .is_none()
        {
            tools.insert(
                reference,
                toml_edit::Item::Value(toml_edit::Value::from(true)),
            );
        }
    } else {
        let mut array = toml_edit::Array::new();
        for alias in &merged {
            array.push(alias.as_str());
        }
        if let Some(table) = tools
            .get_mut(reference)
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            table.insert(
                "aliases",
                toml_edit::Item::Value(toml_edit::Value::Array(array)),
            );
        } else {
            let mut table = toml_edit::InlineTable::new();
            table.insert("aliases", toml_edit::Value::Array(array));
            tools.insert(
                reference,
                toml_edit::Item::Value(toml_edit::Value::InlineTable(table)),
            );
        }
    }

    let mut output = document.to_string();
    if is_new {
        output.insert_str(0, "# shcomp: tools to complete. See `shcomp schema`.\n");
    }
    Ok(output)
}

#[cfg(test)]
mod add_tests {
    use super::*;

    #[test]
    fn adds_a_boolean_selection() {
        let text = add_selection("", "gh", &[], false).unwrap();
        assert!(text.contains("[tools]"), "{text}");
        assert!(text.contains("gh = true"), "{text}");
    }

    #[test]
    fn adds_aliases_without_losing_comments() {
        let original = "# keep me\n[tools]\nuv = true\n";
        let text = add_selection(original, "gh", &["github".to_string()], false).unwrap();
        assert!(text.contains("# keep me"), "{text}");
        assert!(text.contains("uv = true"), "{text}");
        assert!(text.contains("aliases = [\"github\"]"), "{text}");
    }

    #[test]
    fn merges_existing_aliases() {
        let original = "[tools]\ngh = { aliases = [\"github\"] }\n";
        let text = add_selection(original, "gh", &["ghx".to_string()], false).unwrap();
        assert!(text.contains("\"github\""), "{text}");
        assert!(text.contains("\"ghx\""), "{text}");
    }

    #[test]
    fn force_replaces_existing_aliases() {
        let original = "[tools]\ngh = { aliases = [\"github\"] }\n";
        let text = add_selection(original, "gh", &["ghx".to_string()], true).unwrap();
        assert!(!text.contains("\"github\""), "{text}");
        assert!(text.contains("\"ghx\""), "{text}");
    }
}
