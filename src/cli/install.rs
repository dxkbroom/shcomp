//! `shcomp install`: reconcile shell shims for the selected tools.

use std::collections::BTreeMap;

use eyre::{Result, bail, eyre};

use crate::config::Config;
use crate::registry::Registry;
use crate::shell::{ShellReport, ShimStatus};

#[derive(Debug, Default, usage_rs::Args)]
pub struct Install {
    /// Do not fetch URL sources
    #[usage(long)]
    pub no_fetch: bool,

    /// Fail instead of fetching an uncached URL source
    #[usage(long)]
    pub offline: bool,

    /// Generate scripts for the versions on PATH even when `[settings].prewarm`
    /// is false
    #[usage(long)]
    pub prewarm: bool,

    /// Do not generate scripts now; the first Tab generates them
    #[usage(long)]
    pub no_prewarm: bool,

    /// Print what would change without changing it
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// Shells to reconcile; defaults to the `shells` setting
    #[usage(long, short = 's', var)]
    pub shell: Vec<String>,

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,
}

impl Install {
    pub fn run(self) -> Result<()> {
        self.run_with(&Config::load_default()?)
    }

    /// Install for a config that is already loaded; `edit` passes the one it
    /// just wrote.
    pub fn run_with(self, config: &Config) -> Result<()> {
        let registry = Registry::load(config)?;
        let shells = shells(config, &self.shell)?;
        let fetch = !self.no_fetch && !self.offline && !self.dry_run;
        let prewarm = (!self.dry_run && prewarm_enabled(config, self.prewarm, self.no_prewarm))
            .then_some(crate::cache::Prewarm {
                shells: &shells,
                only: None,
            });
        let notes = crate::cache::prefetch(config, &registry, prewarm, fetch)?;
        let reports = reconcile(config, &registry, &shells, self.dry_run)?;
        // Scripts no load can reach go quietly; `-v` names them.
        let pruned = if self.dry_run {
            0
        } else {
            crate::cache::prune(config, &registry)?
        };

        // One row per command, one cell per shell. A command with no source for
        // a shell simply has no cell there.
        let mut order: Vec<String> = Vec::new();
        let mut cells: BTreeMap<String, BTreeMap<&'static str, ShimStatus>> = BTreeMap::new();
        let mut shims: BTreeMap<String, BTreeMap<&'static str, String>> = BTreeMap::new();
        for report in &reports {
            for row in &report.rows {
                if !cells.contains_key(&row.command) {
                    order.push(row.command.clone());
                }
                cells
                    .entry(row.command.clone())
                    .or_default()
                    .insert(report.shell, row.status);
                shims
                    .entry(row.command.clone())
                    .or_default()
                    .insert(report.shell, row.shim.display().to_string());
            }
        }

        // The loading line is printed only for a shell shcomp has no evidence
        // of: a shim that loaded there, or a probe that found the directory.
        let instructions: Vec<String> = reports
            .iter()
            .filter(|report| !crate::shell::probe::loading_line_in_effect(config, report.shell))
            .filter_map(|report| report.loading.clone())
            .collect();

        if self.json {
            let tools: Vec<serde_json::Value> = order
                .iter()
                .map(|command| {
                    let mut value = serde_json::Map::new();
                    value.insert("tool".into(), serde_json::json!(command));
                    for shell in &shells {
                        let status = cells
                            .get(command)
                            .and_then(|cell| cell.get(shell.as_str()))
                            .map_or("-", |status| status.as_str());
                        value.insert(shell.clone(), serde_json::json!(status));
                    }
                    value.insert("shims".into(), serde_json::json!(shims.get(command)));
                    serde_json::Value::Object(value)
                })
                .collect();
            let value = serde_json::json!({
                "dry_run": self.dry_run,
                "shells": shells,
                "tools": tools,
                "notes": notes,
                "pruned": pruned,
                "instructions": instructions,
            });
            println!("{}", serde_json::to_string_pretty(&value)?);
            return Ok(());
        }

        let mut headers: Vec<String> = vec!["Tool".to_string()];
        headers.extend(shells.iter().cloned());
        let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
        let mut table = crate::report::table(&headers, false);
        for command in &order {
            let mut row = vec![comfy_table::Cell::new(command)];
            for shell in &shells {
                let status = cells
                    .get(command)
                    .and_then(|cell| cell.get(shell.as_str()))
                    .map_or("-", |status| status.as_str());
                row.push(comfy_table::Cell::new(status));
            }
            table.add_row(row);
        }
        crate::report::print(&mut table);

        for note in &notes {
            println!("{note}");
        }

        let count = |wanted: fn(ShimStatus) -> bool| {
            reports
                .iter()
                .flat_map(|report| &report.rows)
                .filter(|row| wanted(row.status))
                .count()
        };
        let wrote = count(ShimStatus::wrote);
        let unchanged = count(|status| status == ShimStatus::UpToDate);
        let removed =
            count(|status| matches!(status, ShimStatus::Removed | ShimStatus::WouldRemove));
        let (wrote, removed) = if self.dry_run {
            (format!("{wrote} to write"), format!("{removed} to remove"))
        } else {
            (format!("{wrote} written"), format!("{removed} removed"))
        };
        println!(
            "\n{} command(s): {wrote}, {unchanged} up to date, {removed}",
            reports
                .iter()
                .map(|report| report.rows.len())
                .sum::<usize>()
        );

        if !instructions.is_empty() {
            println!("\nadd this to your shell setup:\n");
            for instruction in &instructions {
                println!("{instruction}");
            }
            println!();
        }
        Ok(())
    }
}

/// Whether to prewarm: `--no-prewarm` wins, then `--prewarm`, then the
/// setting, which defaults to true (ADR-0010).
pub fn prewarm_enabled(config: &Config, prewarm: bool, no_prewarm: bool) -> bool {
    if no_prewarm {
        return false;
    }
    prewarm || config.settings.prewarm
}

/// The shells to reconcile: the flag if given, the setting otherwise.
pub fn shells(config: &Config, requested: &[String]) -> Result<Vec<String>> {
    let shells = if requested.is_empty() {
        config.settings.shells.clone()
    } else {
        requested.to_vec()
    };
    for shell in &shells {
        crate::config::validate_shell(shell)?;
        if !crate::shell::managed(shell) {
            bail!("`{shell}` is not a shell shcomp can install for");
        }
    }
    Ok(shells)
}

/// Reconcile each requested shell, in order.
pub fn reconcile(
    config: &Config,
    registry: &Registry,
    shells: &[String],
    dry_run: bool,
) -> Result<Vec<ShellReport>> {
    let mut reports = Vec::new();
    for shell in shells {
        let report = match shell.as_str() {
            "zsh" => crate::shell::zsh::install(config, registry, dry_run)?,
            "fish" => crate::shell::fish::install(config, registry, dry_run)?,
            other => return Err(eyre!("`{other}` is not a shell shcomp can install for")),
        };
        reports.push(report);
    }
    Ok(reports)
}
