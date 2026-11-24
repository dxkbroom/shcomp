//! Resolve an executable, render a source, and run it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eyre::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::registry::{Source, SourceKind};
use crate::template::{TemplateContext, render};

/// Find an executable on `PATH`, or accept a path that contains a separator.
pub fn resolve_binary(command: &str) -> Result<PathBuf> {
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        let path = PathBuf::from(command);
        if path.is_file() {
            return Ok(path);
        }
        bail!("`{command}` is not a file");
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    bail!("`{command}` is not on PATH")
}

pub fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The version of the executable, from mise when it knows the install, or
/// from a conservative `<version>/bin/<command>` path shape.
pub fn version_for(bin: &Path) -> Option<String> {
    let bin = real_path(bin);
    if let Some(version) = mise_version_for(&bin) {
        return Some(version);
    }
    let parent = bin.parent()?;
    if parent.file_name()?.to_str()? != "bin" {
        return None;
    }
    let version = parent.parent()?.file_name()?.to_str()?;
    if version.contains('.') && version.chars().any(|c| c.is_ascii_digit()) {
        Some(version.to_string())
    } else {
        None
    }
}

static MISE_INSTALLS: OnceLock<Vec<(String, String, PathBuf)>> = OnceLock::new();

/// Every installed version mise reports, as `(tool, version, install_path)`.
///
/// The process asks mise once and reuses the answer: `version_for` and
/// `root_for` both need it, and a cold load should not start mise twice.
pub fn mise_installs() -> &'static [(String, String, PathBuf)] {
    MISE_INSTALLS.get_or_init(|| {
        let output = Command::new("mise")
            .args(["ls", "--json"])
            .output()
            .ok()
            .filter(|output| output.status.success());
        let Some(output) = output else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
            return Vec::new();
        };
        let Some(tools) = json.as_object() else {
            return Vec::new();
        };
        let mut installs = Vec::new();
        for (tool, versions) in tools {
            let Some(versions) = versions.as_array() else {
                continue;
            };
            for entry in versions {
                let installed = entry
                    .get("installed")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if !installed {
                    continue;
                }
                let Some(version) = entry.get("version").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(path) = entry
                    .get("install_path")
                    .and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                installs.push((tool.clone(), version.to_string(), PathBuf::from(path)));
            }
        }
        installs
    })
}

fn mise_version_for(bin: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for (_, version, install) in mise_installs() {
        let install = real_path(install);
        if bin.starts_with(&install) {
            let depth = install.components().count();
            if best
                .as_ref()
                .is_none_or(|(best_depth, _)| depth > *best_depth)
            {
                best = Some((depth, version.clone()));
            }
        }
    }
    best.map(|(_, version)| version)
}

/// The version of a tool, using the install path first and a registry-declared
/// version command second.
pub fn version_for_tool(tool: &crate::registry::RegistryTool, bin: &Path) -> Option<String> {
    if let Some(version) = version_for(bin) {
        return Some(version);
    }
    let spec = tool.version.as_ref()?;
    let context = TemplateContext::new(&tool.name, "zsh", bin);
    let argv = spec
        .run
        .iter()
        .map(|arg| render(arg, &context))
        .collect::<Result<Vec<_>>>();
    let argv = argv.ok()?;
    let (program, rest) = argv.split_first()?;
    let mut command = Command::new(program);
    command.args(rest);
    let (status, stdout, _) =
        run_captured(command, Duration::from_secs(3), "version command").ok()?;
    if !status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&stdout);
    if let Some(pattern) = &spec.regex {
        let regex = regex::Regex::new(pattern).ok()?;
        let captures = regex.captures(&text)?;
        return captures
            .get(1)
            .or_else(|| captures.get(0))
            .map(|matched| matched.as_str().to_string());
    }
    text.split_whitespace()
        .find(|word| word.contains('.') && word.chars().any(|c| c.is_ascii_digit()))
        .map(|word| word.trim_start_matches('v').to_string())
}

/// The install root mise reports for this executable, or the path heuristic.
pub fn root_for(bin: &Path) -> PathBuf {
    let bin = real_path(bin);
    let mut best: Option<(usize, PathBuf)> = None;
    for (_, _, install) in mise_installs() {
        let install = real_path(install);
        if bin.starts_with(&install) {
            let depth = install.components().count();
            if best
                .as_ref()
                .is_none_or(|(best_depth, _)| depth > *best_depth)
            {
                best = Some((depth, install));
            }
        }
    }
    best.map(|(_, path)| path)
        .unwrap_or_else(|| crate::template::install_root(&bin))
}

/// Render and obtain the completion text for one source.
///
/// `url` sources are read from the cache. They are fetched by `install`, never
/// here.
pub fn generate(
    source: &Source,
    context: &TemplateContext,
    timeout: Duration,
    cache_root: &Path,
    cache_key: &str,
    usage_bin: &str,
) -> Result<String> {
    if source.kind == crate::registry::SourceKind::Usage {
        return generate_usage(source, context, timeout, cache_root, cache_key, usage_bin);
    }
    if let Some(run) = &source.run {
        tracing::debug!(argv = ?run, env = ?source.env, "running generator");
        return run_command(run, source, context, timeout);
    }
    if let Some(path) = &source.path {
        let path = render(path, context).wrap_err("cannot render `path`")?;
        return std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("cannot read completion `{path}`"));
    }
    if let Some(url) = &source.url {
        let url = render(url, context).wrap_err("cannot render `url`")?;
        let path = crate::cache::url_cache_path(cache_root, &url);
        return std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("completion `{url}` is not cached; run `shcomp install`"));
    }
    bail!("source has no `run`, `path`, or `url`")
}

/// Ask the `usage` CLI to render the completion from a KDL spec (ADR-0008).
///
/// The spec-producing command is not run here: `usage` runs it once per
/// `--cache-key`, so a Tab pays for it only when the binary changed. `usage`
/// must be on PATH wherever the completion runs.
fn generate_usage(
    source: &Source,
    context: &TemplateContext,
    timeout: Duration,
    cache_root: &Path,
    cache_key: &str,
    usage_bin: &str,
) -> Result<String> {
    let mut command = Command::new(usage_bin);
    command.args(["generate", "completion", &context.shell, &context.command]);

    if let Some(run) = &source.run {
        let argv = run
            .iter()
            .map(|arg| render(arg, context))
            .collect::<Result<Vec<_>>>()
            .wrap_err("cannot render the spec command")?;
        // `usage` runs this through a shell later, so the argv becomes one
        // quoted string; the env goes with it for the same reason.
        let mut line = String::new();
        if !source.env.is_empty() {
            line.push_str("env");
            for (key, value) in &source.env {
                let value = render(value, context)
                    .wrap_err_with(|| format!("cannot render environment variable `{key}`"))?;
                line.push(' ');
                line.push_str(&quote(key));
                line.push('=');
                line.push_str(&quote(&value));
            }
            line.push(' ');
        }
        line.push_str(
            &argv
                .iter()
                .map(|arg| quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        );
        command.args(["--usage-cmd", &line]);
        command.args(["--cache-key", cache_key]);
    } else if let Some(path) = &source.path {
        let path = render(path, context).wrap_err("cannot render `path`")?;
        if !Path::new(&path).is_file() {
            bail!("usage spec `{path}` does not exist");
        }
        command.args(["--file", &path]);
    } else if let Some(url) = &source.url {
        let url = render(url, context).wrap_err("cannot render `url`")?;
        let cached = crate::cache::url_cache_path(cache_root, &url);
        if !cached.is_file() {
            bail!("usage spec `{url}` is not cached; run `shcomp install`");
        }
        command.args(["--file", &cached.display().to_string()]);
    } else {
        bail!("source has no `run`, `path`, or `url`");
    }
    command.args(["--usage-bin", usage_bin]);

    tracing::debug!(args = ?command, "usage generate completion");
    let (status, stdout, stderr) = run_captured(command, timeout, "usage completion generator")?;
    if !status.success() {
        bail!(
            "`{usage_bin} generate completion` failed\n{}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    let script = String::from_utf8_lossy(&stdout).into_owned();
    if script.trim().is_empty() {
        bail!("`{usage_bin} generate completion` produced no output");
    }
    Ok(script)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_command(
    argv: &[String],
    source: &Source,
    context: &TemplateContext,
    timeout: Duration,
) -> Result<String> {
    let argv = argv
        .iter()
        .map(|arg| render(arg, context))
        .collect::<Result<Vec<_>>>()
        .wrap_err("cannot render `run`")?;
    let Some(program) = argv.first() else {
        bail!("`run` must not be empty");
    };
    let mut command = Command::new(program);
    command.args(&argv[1..]);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in &source.env {
        let value = render(value, context)
            .wrap_err_with(|| format!("cannot render environment variable `{key}`"))?;
        command.env(key, value);
    }

    let (status, stdout, stderr) = run_captured(command, timeout, &argv.join(" "))?;
    let stdout = String::from_utf8(stdout).wrap_err("completion output is not UTF-8")?;
    let stderr = String::from_utf8_lossy(&stderr);

    if !status.success() {
        bail!(
            "`{}` exited with {status}\n{}",
            argv.join(" "),
            stderr.trim()
        );
    }
    Ok(stdout)
}

/// Run a command with a timeout, capturing stdout and stderr.
pub fn run_captured(
    mut command: Command,
    timeout: Duration,
    description: &str,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .wrap_err_with(|| format!("cannot run `{description}`"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre::eyre!("process has no stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre::eyre!("process has no stderr"))?;
    let out_thread = std::thread::spawn(move || read_all(stdout));
    let err_thread = std::thread::spawn(move || read_all(stderr));

    let status = wait_timeout(&mut child, timeout, description)?;
    let stdout = out_thread
        .join()
        .map_err(|_| eyre::eyre!("stdout reader panicked"))??;
    let stderr = err_thread
        .join()
        .map_err(|_| eyre::eyre!("stderr reader panicked"))??;
    Ok((status, stdout, stderr))
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn wait_timeout(
    child: &mut Child,
    timeout: Duration,
    description: &str,
) -> Result<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("`{description}` timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A completion script must look like one for its shell.
pub fn validate_script(script: &str, shell: &str, kind: SourceKind) -> Result<()> {
    if script.trim().is_empty() {
        bail!("the generator produced no output");
    }
    let marker = match shell {
        "zsh" => {
            script.contains("#compdef")
                || script.contains("compdef")
                || (kind == SourceKind::Bashcomp && script.contains("complete"))
        }
        "bash" => script.contains("complete"),
        // Fish accepts both short and long options, and generated scripts
        // commonly put options such as `-f` before the command selector.
        // Check tokens on each `complete` command rather than relying on one
        // literal spelling and ordering.
        "fish" => script.lines().any(|line| {
            let mut words = line.split_whitespace();
            if words.next() != Some("complete") {
                return false;
            }
            let words: Vec<_> = words.collect();
            words.windows(2).any(|pair| {
                matches!(pair[0], "-c" | "--command") && !pair[1].starts_with('-')
            })
        }),
        "nu" | "nushell" => script.contains("complete"),
        "powershell" | "pwsh" => script.contains("Register-ArgumentCompleter"),
        other => bail!("unsupported shell `{other}`"),
    };
    if !marker {
        let first = script.lines().next().unwrap_or_default();
        bail!(
            "the generator output does not look like a {shell} completion script (first line: {first:?})"
        );
    }
    Ok(())
}

/// Fetch a URL into the cache, verifying `sha256` when one is declared.
pub fn fetch_url(
    cache_root: &Path,
    url: &str,
    sha256: Option<&str>,
    timeout: Duration,
) -> Result<PathBuf> {
    let path = crate::cache::url_cache_path(cache_root, url);
    if path.exists() {
        let bytes = std::fs::read(&path)?;
        verify_sha256(&bytes, sha256, url)?;
        return Ok(path);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .wrap_err("cannot build the HTTP client")?;
    let response = client
        .get(url)
        .send()
        .wrap_err_with(|| format!("cannot fetch `{url}`"))?;
    if !response.status().is_success() {
        bail!("cannot fetch `{url}`: HTTP {}", response.status());
    }
    let bytes = response
        .bytes()
        .wrap_err_with(|| format!("cannot read `{url}`"))?;
    verify_sha256(&bytes, sha256, url)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&temporary, &bytes)?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

fn verify_sha256(bytes: &[u8], expected: Option<&str>, url: &str) -> Result<()> {
    if let Some(expected) = expected {
        let actual = hex::encode(Sha256::digest(bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("checksum mismatch for `{url}`: expected {expected}, got {actual}");
        }
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_signatures_are_required() {
        assert!(validate_script("#compdef gh\n_gh() {}\n", "zsh", SourceKind::Native).is_ok());
        assert!(validate_script("compdef _gh gh\n", "zsh", SourceKind::Native).is_ok());
        assert!(validate_script("complete -F _gh gh\n", "zsh", SourceKind::Bashcomp).is_ok());
        assert!(validate_script("Usage: gh [OPTIONS]\n", "zsh", SourceKind::Native).is_err());
        assert!(validate_script("", "zsh", SourceKind::Native).is_err());
    }

    #[test]
    fn version_is_none_for_system_bins() {
        assert_eq!(version_for(Path::new("/usr/bin/gh")), None);
    }

    #[test]
    fn renders_environment() {
        let source: Source = toml::from_str(
            r#"
run = ["sh", "-c", "printf %s \"$TOKEN\""]
env = { TOKEN = "{{ shell }}" }
"#,
        )
        .unwrap();
        let context = TemplateContext::new("x", "zsh", Path::new("/bin/true"));
        let output = generate(
            &source,
            &context,
            Duration::from_secs(5),
            Path::new("/tmp"),
            "test-key",
            "usage",
        )
        .unwrap();
        assert_eq!(output, "zsh");
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    fn tool(spec: &str) -> crate::registry::RegistryTool {
        toml::from_str(&format!("name = \"fake\"\n[version]\n{spec}\n")).unwrap()
    }

    fn script(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, path)
    }

    #[test]
    fn reads_the_first_version_like_word() {
        let (_dir, bin) = script("#!/bin/sh\nprintf 'v1.2.3 [+git]\\n'\n");
        let tool = tool("run = [\"{{ bin }}\", \"--version\"]");
        assert_eq!(version_for_tool(&tool, &bin).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn reads_a_capture_group() {
        let (_dir, bin) = script("#!/bin/sh\nprintf 'Gradle 9.8.0\\nBuild time: 2026\\n'\n");
        let tool = tool(
            "run = [\"{{ bin }}\", \"--version\"]\nregex = 'Gradle ([0-9]+\\.[0-9]+(?:\\.[0-9]+)?)'",
        );
        assert_eq!(version_for_tool(&tool, &bin).as_deref(), Some("9.8.0"));
    }
}
