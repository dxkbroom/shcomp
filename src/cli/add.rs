//! `shcomp add`: add a tool selection and finish the job.

use std::process::ExitCode;

use eyre::{Result, WrapErr};

use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::Registry;
use crate::shell::zsh::{self, ShimStatus};

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
}

/// The completer for `add <REFERENCE>`: every reference the registry can add.
fn complete_reference(
    _add: &<Add as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::cli::search::candidates(ctx.prefix)
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
        let original = if config_path.exists() {
            std::fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let updated =
            crate::config::add_selection(&original, &self.reference, &self.alias, self.force)?;
        let changed = updated != original;

        // Every preview and every write below works from the config as it would
        // be, so `--dry-run` reports the shim the real run would write.
        let would_be: Config =
            toml::from_str(&updated).wrap_err("cannot parse the updated config")?;

        if self.dry_run {
            if changed {
                println!(
                    "would add {} ({command}) to {}",
                    self.reference,
                    config_path.display()
                );
            } else {
                println!(
                    "{} ({command}) is already selected in {}",
                    self.reference,
                    config_path.display()
                );
            }
            let mut work = changed;
            if !self.no_install {
                let report = zsh::install(&would_be, &Registry::load(&would_be)?, true)?;
                if let Some(row) = report.row(&command) {
                    println!("{} {}", row.status.as_str(), row.shim.display());
                    work |= row.status == ShimStatus::WouldWrite;
                }
                println!("{}", zsh::fpath_line(&would_be)?);
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
            println!(
                "added {} ({command}) to {}",
                self.reference,
                config_path.display()
            );
        } else {
            println!(
                "{} ({command}) is already selected in {}",
                self.reference,
                config_path.display()
            );
        }

        if self.no_install {
            return Ok(ExitCode::SUCCESS);
        }

        let registry = Registry::load(&would_be)?;
        let notes = crate::cache::prefetch(&would_be, &registry, false, true)?;
        let report = zsh::install(&would_be, &registry, false)?;
        if let Some(row) = report.row(&command) {
            println!("{} {}", row.status.as_str(), row.shim.display());
        }
        for note in notes {
            println!("{note}");
        }
        println!(
            "\nadd this before `compinit`:\n\n{}\n",
            zsh::fpath_line(&would_be)?
        );
        Ok(ExitCode::SUCCESS)
    }
}
