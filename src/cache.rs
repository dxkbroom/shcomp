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
    let real = source::real_path(bin);
    let metadata =
        std::fs::metadata(&real).wrap_err_with(|| format!("cannot stat `{}`", real.display()))?;
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let source = serde_json::to_string(source).wrap_err("cannot describe the source")?;

    let mut hasher = Sha256::new();
    hasher.update(real.to_string_lossy().as_bytes());
    hasher.update(modified.to_le_bytes());
    hasher.update(metadata.len().to_le_bytes());
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
    let key = key_for(&bin, source, shell)?;
    let root = cache_root(config)?;
    let cache = script_cache_path(&root, shell, &tool.resolved.tool.name, &key)?;
    if cache.exists() {
        return Ok(cache);
    }

    let timeout = match &source.timeout {
        Some(timeout) => crate::config::parse_duration(timeout)?,
        None => config.settings.timeout()?,
    };
    let context = context_for(config, tool, shell, &bin, source)?;
    let script = source::generate(source, &context, timeout, &root)?;
    source::validate_script(&script, shell, source.kind)?;
    write_atomic(&cache, script.as_bytes())?;
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

/// Fetch URL sources for the active versions and, when `prewarm`, generate the
/// script for each active command.
pub fn prefetch(
    config: &Config,
    registry: &Registry,
    prewarm: bool,
    fetch: bool,
) -> Result<Vec<String>> {
    let mut messages = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let root = cache_root(config)?;
    for tool in registry.selected(config)? {
        let command = tool.resolved.tool.name.clone();
        if !seen.insert(command.clone()) {
            continue;
        }
        let Ok(bin) = source::resolve_binary(&command) else {
            messages.push(format!("skipped {command}: not on PATH"));
            continue;
        };
        for (shell, source) in &tool.resolved.tool.completions {
            if shell != "zsh" {
                continue;
            }
            if let Some(url) = &source.url {
                if !fetch {
                    messages.push(format!(
                        "skipped {command} {shell}: URL fetching is disabled"
                    ));
                    continue;
                }
                let fetched = (|| -> Result<()> {
                    let context = context_for(config, &tool, shell, &bin, source)?;
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
            if prewarm {
                match load_or_generate(config, registry, &command, shell, &bin) {
                    Ok(_) => messages.push(format!("generated {command} {shell}")),
                    Err(error) => messages.push(format!("skipped {command} {shell}: {error}")),
                }
            }
        }
    }
    Ok(messages)
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
