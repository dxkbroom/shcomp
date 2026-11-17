//! `shcomp install`: reconcile shell shims for the selected tools.

use eyre::Result;

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Args)]
pub struct Install {
    /// Do not fetch URL sources
    #[usage(long)]
    pub no_fetch: bool,

    /// Fail instead of fetching an uncached URL source
    #[usage(long)]
    pub offline: bool,

    /// Generate scripts for the active versions now
    #[usage(long)]
    pub prewarm: bool,

    /// Print what would change without changing it
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,
}

impl Install {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let fetch = !self.no_fetch && !self.offline && !self.dry_run;
        let notes = crate::cache::prefetch(&config, &registry, self.prewarm, fetch)?;
        let report = crate::shell::zsh::install(&config, &registry, self.dry_run)?;
        let instructions = vec![crate::shell::zsh::fpath_line(&config)?];

        if self.json {
            let rows: Vec<serde_json::Value> = report
                .rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "tool": row.command,
                        "status": row.status.as_str(),
                        "shim": row.shim.display().to_string(),
                    })
                })
                .collect();
            let value = serde_json::json!({
                "dry_run": self.dry_run,
                "tools": rows,
                "notes": notes,
                "instructions": instructions,
            });
            println!("{}", serde_json::to_string_pretty(&value)?);
            return Ok(());
        }

        let mut table = crate::report::table(&["Tool", "Status", "Shim"], false);
        for row in &report.rows {
            table.add_row([
                comfy_table::Cell::new(&row.command),
                comfy_table::Cell::new(row.status.as_str()),
                comfy_table::Cell::new(row.shim.display()),
            ]);
        }
        crate::report::print(&table);

        for note in &notes {
            println!("{note}");
        }

        use crate::shell::zsh::ShimStatus;
        let count = |statuses: &[ShimStatus]| {
            report
                .rows
                .iter()
                .filter(|row| statuses.contains(&row.status))
                .count()
        };
        let wrote = count(&[ShimStatus::Wrote, ShimStatus::WouldWrite]);
        let unchanged = count(&[ShimStatus::UpToDate]);
        let removed = count(&[ShimStatus::Removed, ShimStatus::WouldRemove]);
        let (wrote, removed) = if self.dry_run {
            (format!("{wrote} to write"), format!("{removed} to remove"))
        } else {
            (format!("{wrote} written"), format!("{removed} removed"))
        };
        println!(
            "\n{} tool(s): {wrote}, {unchanged} up to date, {removed}",
            report.rows.len()
        );

        println!("\nadd this before `compinit`:\n\n{}\n", instructions[0]);
        Ok(())
    }
}
