//! Cache generated and fetched completions.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::registry::{Registry, Source};
use crate::source;
use crate::template::TemplateContext;

pub fn cache_root(config: &Config) -> Result<PathBuf> {
    match &config.settings.cache_dir {
        Some(path) => Ok(path.clone()),
        None => crate::dirs::cache_dir(),
    }
}

pub fn url_cache_path(root: &Path, url: &str) -> PathBuf {
    root.join("url").join(source::sha256_hex(url.as_bytes()))
}

pub fn script_cache_path(root: &Path, shell: &str, command: &str, key: &str) -> Result<PathBuf> {
    let extension = match shell {
        "zsh" => "zsh",
        "bash" => "bash",
        "fish" => "fish",
        "nu" | "nushell" => "nu",
        "powershell" | "pwsh" => "ps1",
        other => bail!("unsupported shell `{other}`"),
    };
    Ok(root
        .join("scripts")
        .join(shell)
        .join(command)
        .join(format!("{key}.{extension}")))
}

pub fn key_for(bin: &Path, source: &Source, shell: &str) -> Result<String> {
    key_with(&BinStamp::of(bin)?, source, shell)
}

/// The parts of a binary the cache key depends on (ADR-0002), read once so a
/// caller keying several shells for one binary stats it once.
#[derive(Debug, Clone)]
pub struct BinStamp {
    real: PathBuf,
    modified: u128,
    len: u64,
}

impl BinStamp {
    pub fn of(bin: &Path) -> Result<Self> {
        let real = source::real_path(bin);
        let metadata = std::fs::metadata(&real)
            .wrap_err_with(|| format!("cannot stat `{}`", real.display()))?;
        let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(Self {
            real,
            modified,
            len: metadata.len(),
        })
    }
}

pub fn key_with(stamp: &BinStamp, source: &Source, shell: &str) -> Result<String> {
    let source = serde_json::to_string(source).wrap_err("cannot describe the source")?;
    let mut hasher = Sha256::new();
    hasher.update(stamp.real.to_string_lossy().as_bytes());
    hasher.update(stamp.modified.to_le_bytes());
    hasher.update(stamp.len.to_le_bytes());
    hasher.update(source.as_bytes());
    hasher.update(shell.as_bytes());
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    Ok(hex::encode(hasher.finalize())[..32].to_string())
}

/// Resolve the selected tool for `command` and return its cached script path,
/// generating the script on a miss.
pub fn load_or_generate(
    config: &Config,
    registry: &Registry,
    command: &str,
    shell: &str,
    bin: &Path,
) -> Result<PathBuf> {
    let selected = registry.selected(config)?;
    let tool = selected
        .iter()
        .find(|tool| {
            tool.resolved.tool.name == command || tool.aliases.iter().any(|alias| alias == command)
        })
        .ok_or_else(|| eyre::eyre!("`{command}` is not selected in the config"))?;
    let source = tool
        .resolved
        .tool
        .source(shell)
        .ok_or_else(|| eyre::eyre!("`{command}` has no {shell} completion source"))?;

    let bin = source::real_path(bin);
    let key = key_for(&bin, &source, shell)?;
    let root = cache_root(config)?;
    let cache = script_cache_path(&root, shell, &tool.resolved.tool.name, &key)?;
    tracing::debug!(
        command = %command,
        shell,
        bin = %bin.display(),
        key = %key,
        cached = cache.exists(),
        "load"
    );
    if cache.exists() {
        return Ok(cache);
    }

    let timeout = match &source.timeout {
        Some(timeout) => crate::config::parse_duration(timeout)?,
        None => config.settings.timeout()?,
    };
    let context = context_for(config, tool, shell, &bin, &source)?;
    let script = source::generate(
        &source,
        &context,
        timeout,
        &root,
        &key,
        &config.settings.usage_bin,
    )?;
    source::validate_script(&script, shell, source.kind)?;
    write_atomic(&cache, script.as_bytes())?;
    let meta = ScriptMeta {
        command: tool.resolved.tool.name.clone(),
        shell: shell.to_string(),
        bin: bin.clone(),
        kind: source.kind_name().to_string(),
        created: unix_now(),
        shcomp: env!("CARGO_PKG_VERSION").to_string(),
    };
    write_atomic(&meta_path(&cache), &serde_json::to_vec_pretty(&meta)?)?;
    tracing::info!(command = %command, script = %cache.display(), "generated");
    Ok(cache)
}

fn context_for(
    config: &Config,
    tool: &crate::registry::SelectedTool,
    shell: &str,
    bin: &Path,
    source: &Source,
) -> Result<TemplateContext> {
    let version = source::version_for_tool(&tool.resolved.tool, bin);
    if version.is_none() && wants_version(source)? {
        bail!(
            "cannot determine the version of `{}`; a manager that records install paths (mise) is needed for `{{{{ version }}}}`",
            bin.display()
        );
    }
    let root = source::root_for(bin);
    let (config_root, config_source) = match &tool.resolved.source_path {
        Some(path) => (
            path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
            Some(path.as_path()),
        ),
        None => (config.config_root(), config.source.as_deref()),
    };
    let mut context =
        TemplateContext::new(&tool.resolved.tool.name, shell, bin).with_version(version);
    context.root = root.to_string_lossy().into_owned();
    context.config_root = config_root.to_string_lossy().into_owned();
    context.config_source = config_source.map(|path| path.to_string_lossy().into_owned());
    Ok(context)
}

fn wants_version(source: &Source) -> Result<bool> {
    Ok(serde_json::to_string(source)?.contains("{{ version"))
}

/// Which scripts `prefetch` generates ahead of the first Tab (ADR-0010).
#[derive(Debug, Clone, Copy)]
pub struct Prewarm<'a> {
    /// The shells being installed for.
    pub shells: &'a [String],
    /// Only these commands; every selected command when `None`.
    pub only: Option<&'a [String]>,
}

/// Fetch URL sources for the active versions and, when `prewarm` is given,
/// generate the script for each selected command in each of its shells.
///
/// A generator failure is a note, not an error: the shim is still written and
/// the cold path still generates at Tab time.
pub fn prefetch(
    config: &Config,
    registry: &Registry,
    prewarm: Option<Prewarm<'_>>,
    fetch: bool,
) -> Result<Vec<String>> {
    let mut messages = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut generated = 0;
    let root = cache_root(config)?;
    for tool in registry.selected(config)? {
        let command = tool.resolved.tool.name.clone();
        if !seen.insert(command.clone()) {
            continue;
        }
        let wanted =
            prewarm.is_some_and(|prewarm| prewarm.only.is_none_or(|only| only.contains(&command)));
        let has_url = tool.resolved.tool.completed_shells().iter().any(|shell| {
            tool.resolved
                .tool
                .source(shell)
                .is_some_and(|source| source.url.is_some())
        });
        if !wanted && !has_url {
            continue;
        }
        let Ok(bin) = source::resolve_binary(&command) else {
            messages.push(format!("skipped {command}: not on PATH"));
            continue;
        };
        for shell in tool.resolved.tool.completed_shells() {
            if !crate::shell::managed(&shell) {
                continue;
            }
            let Some(source) = tool.resolved.tool.source(&shell) else {
                continue;
            };
            if let Some(url) = &source.url {
                if !fetch {
                    messages.push(format!(
                        "skipped {command} {shell}: URL fetching is disabled"
                    ));
                    continue;
                }
                let fetched = (|| -> Result<()> {
                    let context = context_for(config, &tool, &shell, &bin, &source)?;
                    let url = crate::template::render(url, &context)?;
                    let timeout = match &source.timeout {
                        Some(timeout) => crate::config::parse_duration(timeout)?,
                        None => config.settings.timeout()?,
                    };
                    source::fetch_url(&root, &url, source.sha256.as_deref(), timeout)?;
                    Ok(())
                })();
                match fetched {
                    Ok(()) => messages.push(format!("fetched {command} {shell}")),
                    Err(error) => messages.push(format!("skipped {command} {shell}: {error}")),
                }
            }
            let Some(prewarm) = prewarm.filter(|_| wanted) else {
                continue;
            };
            let shell_wanted = prewarm.shells.contains(&shell)
                && tool
                    .shells
                    .as_ref()
                    .is_none_or(|shells| shells.contains(&shell));
            if !shell_wanted {
                continue;
            }
            match load_or_generate(config, registry, &command, &shell, &bin) {
                Ok(_) => generated += 1,
                Err(error) => messages.push(format!(
                    "prewarm failed for {command} {shell}: {}",
                    format!("{error:#}").lines().next().unwrap_or_default()
                )),
            }
        }
    }
    if prewarm.is_some() {
        messages.push(format!("prewarmed {generated} script(s)"));
    }
    Ok(messages)
}

/// What produced a cached script (ADR-0011). Written beside the script, as
/// `<key>.json`, by the call that writes the script.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ScriptMeta {
    pub command: String,
    pub shell: String,
    /// The real path of the binary the key was computed from.
    pub bin: PathBuf,
    pub kind: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub shcomp: String,
}

pub fn meta_path(script: &Path) -> PathBuf {
    script.with_extension("json")
}

pub fn read_meta(script: &Path) -> Option<ScriptMeta> {
    let text = std::fs::read_to_string(meta_path(script)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// One cached script on disk.
#[derive(Debug, Clone)]
pub struct CachedScript {
    pub path: PathBuf,
    pub shell: String,
    pub command: String,
    pub key: String,
}

/// Every cached script under the cache root, sorted by path.
pub fn scripts(root: &Path) -> Vec<CachedScript> {
    let mut found = Vec::new();
    let Ok(shells) = std::fs::read_dir(root.join("scripts")) else {
        return found;
    };
    for shell in shells.flatten() {
        let shell_name = shell.file_name().to_string_lossy().into_owned();
        let Ok(commands) = std::fs::read_dir(shell.path()) else {
            continue;
        };
        for command in commands.flatten() {
            let command_name = command.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(command.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some((key, extension)) = name.split_once('.') else {
                    continue;
                };
                // Sidecars and interrupted temporaries are not scripts.
                if extension == "json" || extension.starts_with("tmp") {
                    continue;
                }
                found.push(CachedScript {
                    path: path.clone(),
                    shell: shell_name.clone(),
                    command: command_name.clone(),
                    key: key.to_string(),
                });
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Why a cached script is no longer reachable from any `shcomp load`, or
/// `None` when it still is.
pub fn stale_reason(
    script: &CachedScript,
    selected: &[crate::registry::SelectedTool],
) -> Option<String> {
    let Some(meta) = read_meta(&script.path) else {
        return Some("no metadata (written by an older shcomp)".to_string());
    };
    if !meta.bin.exists() {
        return Some(format!("{} is gone", meta.bin.display()));
    }
    let Some(tool) = selected
        .iter()
        .find(|tool| tool.resolved.tool.name == script.command)
    else {
        return Some(format!("{} is not selected", script.command));
    };
    let Some(source) = tool.resolved.tool.source(&script.shell) else {
        return Some(format!("{} has no {} source", script.command, script.shell));
    };
    match key_for(&meta.bin, &source, &script.shell) {
        Ok(key) if key == script.key => None,
        Ok(_) => Some("superseded (the binary or the source changed)".to_string()),
        Err(error) => Some(error.to_string()),
    }
}

/// Remove a script, its sidecar, and the directories it leaves empty.
pub fn remove_script(root: &Path, script: &Path) -> Result<()> {
    match std::fs::remove_file(script) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("cannot remove `{}`", script.display()));
        }
    }
    let _ = std::fs::remove_file(meta_path(script));
    let stop = root.join("scripts");
    let mut dir = script.parent();
    while let Some(current) = dir {
        if current == stop || !current.starts_with(&stop) || std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
    Ok(())
}

/// Remove every stale script quietly; returns how many went.
pub fn prune(config: &Config, registry: &Registry) -> Result<usize> {
    let root = cache_root(config)?;
    let selected = registry.selected(config)?;
    let mut pruned = 0;
    for script in scripts(&root) {
        if let Some(reason) = stale_reason(&script, &selected) {
            tracing::info!(script = %script.path.display(), %reason, "pruned");
            remove_script(&root, &script.path)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn source(run: &str) -> Source {
        toml::from_str(&format!("run = [\"{run}\"]")).unwrap()
    }

    fn binary() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"#!/bin/sh\n").unwrap();
        file
    }

    #[test]
    fn different_sources_get_different_keys() {
        let file = binary();
        let first = key_for(file.path(), &source("one"), "zsh").unwrap();
        let second = key_for(file.path(), &source("two"), "zsh").unwrap();
        assert_ne!(first, second);
        let another_shell = key_for(file.path(), &source("one"), "bash").unwrap();
        assert_ne!(first, another_shell);
    }

    #[test]
    fn script_paths_use_the_shell_extension() {
        let root = std::path::Path::new("/tmp/cache");
        let zsh = script_cache_path(root, "zsh", "gh", "abc").unwrap();
        assert!(zsh.ends_with("abc.zsh"));
        let fish = script_cache_path(root, "fish", "gh", "abc").unwrap();
        assert!(fish.ends_with("abc.fish"));
        assert!(script_cache_path(root, "elvish", "gh", "abc").is_err());
    }

    #[test]
    fn url_cache_paths_are_deterministic() {
        let root = std::path::Path::new("/tmp/cache");
        let first = url_cache_path(root, "https://example.com/_tool");
        let second = url_cache_path(root, "https://example.com/_tool");
        assert_eq!(first, second);
        assert_ne!(first, url_cache_path(root, "https://example.com/_other"));
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;
    use std::io::Write;

    fn setup(script_body: &str) -> (tempfile::TempDir, Config, Registry, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake");
        let mut file = std::fs::File::create(&bin).unwrap();
        file.write_all(script_body.as_bytes()).unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut config: Config = toml::from_str(
            r#"
[tools]
"example/fake" = true

[[registry]]
repo = "example/fake"
main = "fake"

[[registry.tools]]
name = "fake"

[registry.tools.completions.zsh]
run = ["{{ bin }}"]
"#,
        )
        .unwrap();
        config.settings.cache_dir = Some(dir.path().join("cache"));
        let registry = Registry::load_with(&config, Some(&dir.path().join("empty"))).unwrap();
        (dir, config, registry, bin)
    }

    #[test]
    fn generates_and_reuses_a_script() {
        let (_dir, config, registry, bin) =
            setup("#!/bin/sh\nprintf '#compdef fake\\n_fake() {}\\n'\n");
        let first = load_or_generate(&config, &registry, "fake", "zsh", &bin).unwrap();
        assert!(first.exists());
        assert!(
            std::fs::read_to_string(&first)
                .unwrap()
                .contains("#compdef")
        );
        let second = load_or_generate(&config, &registry, "fake", "zsh", &bin).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_generated_script_carries_its_origin() {
        let (_dir, config, registry, bin) =
            setup("#!/bin/sh\nprintf '#compdef fake\\n_fake() {}\\n'\n");
        let script = load_or_generate(&config, &registry, "fake", "zsh", &bin).unwrap();
        let meta = read_meta(&script).unwrap();
        assert_eq!(meta.command, "fake");
        assert_eq!(meta.shell, "zsh");
        assert_eq!(meta.bin, source::real_path(&bin));
        assert_eq!(meta.kind, "run");
        let root = cache_root(&config).unwrap();
        let found = scripts(&root);
        assert_eq!(found.len(), 1, "the sidecar is not a script: {found:?}");
        let selected = registry.selected(&config).unwrap();
        assert_eq!(stale_reason(&found[0], &selected), None);
    }

    #[test]
    fn stale_scripts_are_named_and_pruned() {
        let (_dir, config, registry, bin) =
            setup("#!/bin/sh\nprintf '#compdef fake\\n_fake() {}\\n'\n");
        let script = load_or_generate(&config, &registry, "fake", "zsh", &bin).unwrap();
        let root = cache_root(&config).unwrap();
        let selected = registry.selected(&config).unwrap();

        // A script from an older shcomp has no sidecar.
        let legacy = script.with_file_name("0123.zsh");
        std::fs::write(&legacy, "#compdef fake\n").unwrap();
        let found = scripts(&root);
        let legacy_entry = found.iter().find(|entry| entry.path == legacy).unwrap();
        assert!(
            stale_reason(legacy_entry, &selected)
                .unwrap()
                .contains("no metadata")
        );

        // The binary going away makes the real script stale too.
        std::fs::remove_file(&bin).unwrap();
        let entry = scripts(&root)
            .into_iter()
            .find(|entry| entry.path == script)
            .unwrap();
        assert!(stale_reason(&entry, &selected).unwrap().contains("is gone"));

        assert_eq!(prune(&config, &registry).unwrap(), 2);
        assert!(!script.exists());
        assert!(!meta_path(&script).exists());
        assert!(scripts(&root).is_empty());
        assert!(
            !root.join("scripts").join("zsh").exists(),
            "empty dirs go too"
        );
    }

    #[test]
    fn a_help_page_is_not_a_completion() {
        let (_dir, config, registry, bin) = setup("#!/bin/sh\nprintf 'Usage: fake [OPTIONS]\\n'\n");
        let error = load_or_generate(&config, &registry, "fake", "zsh", &bin).unwrap_err();
        assert!(
            format!("{error:?}").contains("does not look like"),
            "{error:?}"
        );
    }

    #[test]
    fn a_missing_url_points_at_install() {
        let dir = tempfile::tempdir().unwrap();
        let mut config: Config = toml::from_str(
            r#"
[tools]
"example/url" = true

[[registry]]
repo = "example/url"
main = "url"

[[registry.tools]]
name = "url"

[registry.tools.completions.zsh]
url = "https://example.invalid/_url"
"#,
        )
        .unwrap();
        config.settings.cache_dir = Some(dir.path().join("cache"));
        let registry = Registry::load_with(&config, Some(&dir.path().join("empty"))).unwrap();
        let bin = dir.path().join("url");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        let error = load_or_generate(&config, &registry, "url", "zsh", &bin).unwrap_err();
        assert!(format!("{error:?}").contains("shcomp install"), "{error:?}");
    }
}
