//! `shcomp doctor`: check the configuration and installed shims.

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
        ("fish", crate::shell::fish::site_completions(config)?),
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

    for (name, references) in registry.duplicates(config) {
        let times = match references.len() {
            2 => "twice".to_string(),
            count => format!("{count} times"),
        };
        findings.push(Finding::warn(
            "config",
            format!("{name} is selected {times} ({})", references.join(", ")),
        ));
    }

    findings.push(Finding::ok(
        "registry",
        format!("{} item(s)", registry.entries().len()),
    ));

    let shells = &config.settings.shells;
    if shells.iter().any(|shell| shell == "zsh") {
        findings.extend(crate::shell::zsh::doctor(config, registry, check)?);
        if check {
            findings.extend(crate::shell::probe::findings(config, registry, "zsh"));
        }
    }
    if shells.iter().any(|shell| shell == "fish") {
        findings.extend(crate::shell::fish::doctor(config, registry, check)?);
        if check {
            findings.extend(crate::shell::probe::findings(config, registry, "fish"));
        }
    }

    findings.extend(mise_findings(crate::shell::zsh::mise_owned_commands(
        config, registry,
    )?));

    let cache = crate::cache::cache_root(config)?;
    let scripts = crate::cache::scripts(&cache).len();
    findings.push(Finding::ok(
        "cache",
        format!("{} ({scripts} script(s))", cache.display()),
    ));

    findings.extend(load_findings(config, registry)?);

    Ok(findings)
}

/// mise's packslip loader completing a selected command too.
///
/// shcomp's own entry always overlaps when shcomp itself was installed through
/// packslip, and its shim wins, so that one is expected. Any other overlap is a
/// stub that may duplicate candidates, and stays a warning.
fn mise_findings(mut commands: Vec<String>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(index) = commands.iter().position(|command| command == "shcomp") {
        commands.remove(index);
        findings.push(Finding::ok(
            "mise",
            "mise also completes shcomp, which it installed; shcomp's own shim wins",
        ));
    }
    if !commands.is_empty() {
        findings.push(Finding::warn(
            "mise",
            format!(
                "mise also completes {}; shcomp writes its own shim and wins on fpath, but a mise stub may duplicate candidates",
                commands.join(", ")
            ),
        ));
    } else if findings.is_empty() {
        findings.push(Finding::ok(
            "mise",
            "no selected command is installed from a packslip",
        ));
    }
    findings
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

    #[test]
    fn shcomp_completing_itself_through_mise_is_expected() {
        let findings = mise_findings(vec!["shcomp".to_string()]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Ok);

        let findings = mise_findings(vec!["shcomp".to_string(), "gh".to_string()]);
        assert_eq!(findings[1].severity, Severity::Warn);
        assert!(findings[1].message.contains("mise also completes gh;"));
        assert!(!findings[1].message.contains("shcomp,"));

        let findings = mise_findings(Vec::new());
        assert_eq!(findings[0].severity, Severity::Ok);
        assert!(findings[0].message.contains("no selected command"));
    }
}
