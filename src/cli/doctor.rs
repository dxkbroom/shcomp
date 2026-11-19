//! `shcomp doctor`: check the configuration and installed shims.

use std::path::Path;
use std::process::ExitCode;

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;
use crate::report::{Finding, Severity, print, severity_cell, table};

#[derive(Debug, usage_rs::Args)]
pub struct Doctor {
    /// Run each generator and validate its output
    #[usage(long)]
    pub check: bool,

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,
}

impl Doctor {
    pub fn run(self) -> Result<ExitCode> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let findings = findings(&config, &registry, self.check)?;
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json(&config, &findings))?
            );
        } else {
            render(&findings);
        }
        let failed = findings.iter().any(|finding| finding.severity.is_error());
        Ok(if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    }
}

fn findings(config: &Config, registry: &Registry, check: bool) -> Result<Vec<Finding>> {
    let mut findings = vec![Finding::ok(
        "version",
        format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    )];

    for (label, path) in [
        ("data", crate::dirs::data_dir()?),
        ("cache", crate::dirs::cache_dir()?),
        ("zsh", crate::shell::zsh::site_functions(config)?),
    ] {
        findings.push(Finding::ok("dirs", format!("{label}: {}", path.display())));
    }

    match config.source.as_deref() {
        Some(path) => findings.push(Finding::ok("config", path.display().to_string())),
        None => findings.push(Finding::warn(
            "config",
            "not found; nothing is selected, so nothing will be installed",
        )),
    }

    findings.push(Finding::ok(
        "registry",
        format!("{} item(s)", registry.entries().len()),
    ));

    findings.extend(crate::shell::zsh::doctor(config, registry, check)?);

    let mise = crate::shell::zsh::mise_owned_commands(config, registry)?;
    if mise.is_empty() {
        findings.push(Finding::ok(
            "mise",
            "no selected command is installed from a packslip",
        ));
    } else {
        findings.push(Finding::warn(
            "mise",
            format!(
                "mise also completes {}; shcomp writes its own shim and wins on fpath, but a mise stub may duplicate candidates",
                mise.join(", ")
            ),
        ));
    }

    let cache = crate::cache::cache_root(config)?;
    let scripts = count_scripts(&cache.join("scripts"));
    findings.push(Finding::ok(
        "cache",
        format!("{} ({scripts} script(s))", cache.display()),
    ));

    findings.extend(load_findings(config, registry)?);

    Ok(findings)
}

/// What the cold loads recorded (ADR-0011): failures the prompt never showed,
/// and shells whose shims have never loaded at all.
fn load_findings(config: &Config, registry: &Registry) -> Result<Vec<Finding>> {
    let selected: std::collections::BTreeSet<String> = registry
        .selected(config)?
        .iter()
        .flat_map(|tool| {
            std::iter::once(tool.resolved.tool.name.clone()).chain(tool.aliases.clone())
        })
        .collect();
    let now = crate::cache::unix_now();
    let mut findings = Vec::new();
    for shell in &config.settings.shells {
        let shims = match shell.as_str() {
            "zsh" => crate::shell::zsh::uninstall(config, true)?.len(),
            "fish" => crate::shell::fish::uninstall(config, true)?.len(),
            _ => continue,
        };
        let records: Vec<_> = crate::state::records(config, shell)
            .into_iter()
            .filter(|record| selected.contains(&record.command))
            .collect();
        if records.is_empty() {
            if shims > 0 {
                findings.push(Finding::warn(
                    "loads",
                    format!(
                        "{shell}: {shims} shim(s) installed but none has loaded yet; if you have pressed Tab since installing, the loading line is not in effect (`shcomp doctor --check`)"
                    ),
                ));
            }
            continue;
        }
        let failed: Vec<_> = records.iter().filter(|record| !record.ok).collect();
        for record in &failed {
            let mut message = format!(
                "{} ({shell}): last load failed {}: {}",
                record.command,
                crate::state::ago(record.at, now),
                record.error.as_deref().unwrap_or("unknown error")
            );
            if let Some(stderr) = &record.stderr {
                message.push_str(&format!(" — {stderr}"));
            }
            findings.push(Finding::warn("loads", message));
        }
        if failed.is_empty() {
            findings.push(Finding::ok(
                "loads",
                format!("{shell}: {} command(s) loaded, no failures", records.len()),
            ));
        }
    }
    Ok(findings)
}

fn render(findings: &[Finding]) {
    let mut table = table(&["Section", "Status", "Message"], false);
    for finding in findings {
        table.add_row([
            comfy_table::Cell::new(finding.section),
            severity_cell(finding.severity),
            comfy_table::Cell::new(&finding.message),
        ]);
    }
    print(&mut table);

    let count = |severity: Severity| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    println!(
        "\n{} ok, {} warn, {} error",
        count(Severity::Ok),
        count(Severity::Warn),
        count(Severity::Error)
    );
}

fn json(config: &Config, findings: &[Finding]) -> serde_json::Value {
    let count = |severity: Severity| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "config": config.source.as_deref().map(|path| path.display().to_string()),
        "counts": {
            "ok": count(Severity::Ok),
            "warn": count(Severity::Warn),
            "error": count(Severity::Error),
        },
        "ok": count(Severity::Error) == 0,
        "findings": findings
            .iter()
            .map(|finding| serde_json::json!({
                "section": finding.section,
                "status": finding.severity.as_str(),
                "message": finding.message,
            }))
            .collect::<Vec<_>>(),
    })
}

fn count_scripts(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_scripts(&path)
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("zsh") {
                1
            } else {
                0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_decide_the_exit_code() {
        let findings = [Finding::ok("zsh", "fine"), Finding::warn("zsh", "stale")];
        assert!(!findings.iter().any(|finding| finding.severity.is_error()));
        let findings = [Finding::error("zsh", "missing shim")];
        assert!(findings.iter().any(|finding| finding.severity.is_error()));
    }
}
