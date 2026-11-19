//! `shcomp add`: add a tool selection and finish the job.

use std::process::ExitCode;

use eyre::{Result, WrapErr};

use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::Registry;
use crate::shell::ShimStatus;

#[derive(Debug, Default, usage_rs::Args)]
pub struct Add {
    /// Tool reference to add
    #[usage(arg, complete = crate::cli::add::complete_reference)]
    pub reference: String,

    /// Extra command name to register; repeat for several
    #[usage(long, var)]
    pub alias: Vec<String>,

    /// Print what would change without changing anything
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// With --dry-run, exit 1 when there is something to do
    #[usage(long, requires = "dry_run")]
    pub dry_run_code: bool,

    /// Write the selection but install nothing
    #[usage(long)]
    pub no_install: bool,

    /// Replace the selection's aliases instead of adding to them
    #[usage(long, short)]
    pub force: bool,

    /// Do not generate the script now; the first Tab generates it
    #[usage(long)]
    pub no_prewarm: bool,
}

/// The completer for `add <REFERENCE>`: every tool the registry can add.
fn complete_reference(
    _add: &<Add as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::search::candidates(ctx.prefix)
}

impl Add {
    pub fn run(self) -> Result<ExitCode> {
        let config_path = crate::dirs::config_file()?;
        let config = if config_path.exists() {
            Config::load(&config_path)?
        } else {
            Config::default()
        };
        let registry = Registry::load(&config)?;
        let resolved = registry
            .resolve_str(&self.reference)
            .wrap_err_with(|| format!("cannot add `{}`", self.reference))?;
        let command = resolved.tool.name.clone();
        // `tv` and `alexpasmantier/television/tv` are one tool: an existing key
        // for it is where any new aliases go, and no second key is written.
        let existing = registry.selection_key(&config, &resolved);
        let key = existing.clone().unwrap_or_else(|| self.reference.clone());
        let already = match &existing {
            Some(existing) if *existing != self.reference => format!(
                "{} ({command}) is already selected as {existing} in {}",
                self.reference,
                config_path.display()
            ),
            _ => format!(
                "{} ({command}) is already selected in {}",
                self.reference,
                config_path.display()
            ),
        };
        let original = if config_path.exists() {
            std::fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let updated = crate::config::add_selection(&original, &key, &self.alias, self.force)?;
        let changed = updated != original;

        // Every preview and every write below works from the config as it would
        // be, so `--dry-run` reports the shim the real run would write.
        let would_be: Config =
            toml::from_str(&updated).wrap_err("cannot parse the updated config")?;

        if self.dry_run {
            if changed && existing.is_some() {
                println!(
                    "would update {key} ({command}) in {}",
                    config_path.display()
                );
            } else if changed {
                println!(
                    "would add {} ({command}) to {}",
                    self.reference,
                    config_path.display()
                );
            } else {
                println!("{already}");
            }
            let mut work = changed;
            if !self.no_install {
                let registry = Registry::load(&would_be)?;
                let shells = crate::cli::install::shells(&would_be, &[])?;
                let reports = crate::cli::install::reconcile(&would_be, &registry, &shells, true)?;
                for report in &reports {
                    if let Some(row) = report.row(&command) {
                        println!(
                            "{}: {} {}",
                            report.shell,
                            row.status.as_str(),
                            row.shim.display()
                        );
                        work |= row.status == ShimStatus::WouldWrite;
                    }
                    if let Some(line) = &report.loading {
                        println!("{line}");
                    }
                }
            }
            return Ok(if self.dry_run_code && work {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            });
        }

        if changed {
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&config_path, &updated)?;
            if existing.is_some() {
                println!("updated {key} ({command}) in {}", config_path.display());
            } else {
                println!(
                    "added {} ({command}) to {}",
                    self.reference,
                    config_path.display()
                );
            }
        } else {
            println!("{already}");
        }

        if self.no_install {
            return Ok(ExitCode::SUCCESS);
        }

        let registry = Registry::load(&would_be)?;
        let shells = crate::cli::install::shells(&would_be, &[])?;
        let only = [command.clone()];
        let prewarm = crate::cli::install::prewarm_enabled(&would_be, false, self.no_prewarm)
            .then_some(crate::cache::Prewarm {
                shells: &shells,
                only: Some(&only),
            });
        let notes = crate::cache::prefetch(&would_be, &registry, prewarm, true)?;
        let reports = crate::cli::install::reconcile(&would_be, &registry, &shells, false)?;
        for report in &reports {
            if let Some(row) = report.row(&command) {
                println!(
                    "{}: {} {}",
                    report.shell,
                    row.status.as_str(),
                    row.shim.display()
                );
            }
        }
        for note in notes {
            println!("{note}");
        }
        let instructions: Vec<&String> = reports
            .iter()
            .filter_map(|report| report.loading.as_ref())
            .collect();
        if !instructions.is_empty() {
            println!("\nadd this to your shell setup:\n");
            for line in instructions {
                println!("{line}");
            }
            println!();
        }
        Ok(ExitCode::SUCCESS)
    }
}
