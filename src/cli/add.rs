//! `shcomp add`: add tool selections and finish the job.
//!
//! A reference the registry knows becomes a `[tools]` key. With a source flag
//! (`--run`, `--run-zsh`, `--run-fish`, `--url`, `--usage`) the one reference is
//! a command the registry does not know, and `add` writes a `[[registry]]`
//! item for it too — after checking that the command is on `PATH` and that the
//! source produces a completion in every shell it declares.

use std::path::Path;
use std::process::ExitCode;

use eyre::{Result, WrapErr, bail};

use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::inline::InlineSpec;
use crate::registry::Registry;
use crate::shell::ShimStatus;

#[derive(Debug, Default, usage_rs::Args)]
pub struct Add {
    /// Tool references to add; with a source flag, the one command to describe
    #[usage(arg, complete = crate::cli::add::complete_reference)]
    pub reference: Vec<String>,

    /// Extra command name to register; repeat for several
    #[usage(long, var)]
    pub alias: Vec<String>,

    /// Command printing the completion, for every shell; `{shell}` and `{bin}`
    /// are filled in
    #[usage(long)]
    pub run: Option<String>,

    /// Command printing the zsh completion
    #[usage(long)]
    pub run_zsh: Option<String>,

    /// Command printing the fish completion
    #[usage(long)]
    pub run_fish: Option<String>,

    /// Completion file to fetch, for the one `--shell`
    #[usage(long)]
    pub url: Option<String>,

    /// Command printing a usage KDL spec, rendered by the `usage` CLI
    #[usage(long)]
    pub usage: Option<String>,

    /// Shells the described command completes in; defaults to the installed ones
    #[usage(long, short = 's', var, choices("zsh", "fish"))]
    pub shell: Vec<String>,

    /// Description for the described command
    #[usage(long)]
    pub description: Option<String>,

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
    fn inline_spec(&self) -> InlineSpec {
        InlineSpec {
            name: self.reference.first().cloned().unwrap_or_default(),
            description: self.description.clone(),
            run: self.run.clone(),
            usage: self.usage.clone(),
            run_zsh: self.run_zsh.clone(),
            run_fish: self.run_fish.clone(),
            url: self.url.clone(),
            shells: self.shell.clone(),
        }
    }

    pub fn run(self) -> Result<ExitCode> {
        let spec = self.inline_spec();
        let inline = spec.is_inline();
        if self.reference.is_empty() {
            bail!("name at least one tool to add");
        }
        if inline && self.reference.len() > 1 {
            bail!("a source flag describes one command; add the others separately");
        }
        if !inline && (!self.shell.is_empty() || self.description.is_some()) {
            bail!("`--shell` and `--description` describe a new command; pass a source flag too");
        }
        if self.reference.len() > 1 && !self.alias.is_empty() {
            bail!("`--alias` needs exactly one reference");
        }

        let config_path = crate::dirs::config_file()?;
        let config = if config_path.exists() {
            Config::load(&config_path)?
        } else {
            Config::default()
        };
        let registry = Registry::load(&config)?;
        let original = if config_path.exists() {
            std::fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let path = config_path.display().to_string();
        let (verb_add, verb_update) = if self.dry_run {
            ("would add", "would update")
        } else {
            ("added", "updated")
        };

        let mut text = original.clone();
        let mut commands: Vec<String> = Vec::new();
        let mut messages: Vec<String> = Vec::new();
        let mut checks_failed = false;

        if inline {
            let name = spec.name.clone();
            let replacing = inline_item_exists(&config, &name);
            if !replacing {
                match registry.resolve_str(&name) {
                    Ok(resolved) => bail!(
                        "`{name}` is already in the registry ({}); drop the source flags to select it",
                        resolved
                            .repo_slug()
                            .unwrap_or_else(|| resolved.reference.clone())
                    ),
                    Err(error) if error.to_string().contains("ambiguous") => {
                        bail!("`{name}` is already in the registry: {error}")
                    }
                    Err(_) => {}
                }
            }
            let installed = crate::cli::install::shells(&config, &[])?;
            let (item_toml, item) = spec.render(&installed)?;
            let selected = crate::config::add_selection(&text, &name, &self.alias, self.force)?;
            text = crate::inline::splice(&selected, &name, &item_toml)?;

            // Show the item, then prove it works before anything is written.
            println!("{item_toml}");
            let would_be: Config =
                toml::from_str(&text).wrap_err("cannot parse the updated config")?;
            would_be.validate()?;
            for (shell, outcome) in trial(&would_be, &name, &item)? {
                match outcome {
                    Ok(lines) => println!("{shell}: ok ({lines} line(s))"),
                    Err(error) => {
                        checks_failed = true;
                        println!("{shell}: failed: {error}");
                    }
                }
            }
            if checks_failed {
                bail!(
                    "not writing `{name}`: a source failed; fix the command, or describe each shell with --run-zsh / --run-fish"
                );
            }
            let verb = if replacing { verb_update } else { verb_add };
            messages.push(format!("{verb} {name} (inline registry item) in {path}"));
            commands.push(name);
        } else {
            for reference in &self.reference {
                let resolved = registry
                    .resolve_str(reference)
                    .wrap_err_with(|| format!("cannot add `{reference}`"))?;
                let command = resolved.tool.name.clone();
                // `tv` and `alexpasmantier/television/tv` are one tool: an
                // existing key for it is where any new aliases go, and no second
                // key is written.
                let existing = registry.selection_key(&config, &resolved);
                let key = existing.clone().unwrap_or_else(|| reference.clone());
                let before = text.clone();
                text = crate::config::add_selection(&text, &key, &self.alias, self.force)?;
                messages.push(match (&existing, text != before) {
                    (Some(_), true) => format!("{verb_update} {key} ({command}) in {path}"),
                    (None, true) => format!("{verb_add} {reference} ({command}) to {path}"),
                    (Some(existing), false) if existing != reference => format!(
                        "{reference} ({command}) is already selected as {existing} in {path}"
                    ),
                    _ => format!("{reference} ({command}) is already selected in {path}"),
                });
                if !commands.contains(&command) {
                    commands.push(command);
                }
            }
        }
        let changed = text != original;

        // Every preview and every write below works from the config as it would
        // be, so `--dry-run` reports the shim the real run would write.
        let would_be: Config = toml::from_str(&text).wrap_err("cannot parse the updated config")?;

        for message in &messages {
            println!("{message}");
        }

        if self.dry_run {
            let mut work = changed;
            if !self.no_install {
                let registry = Registry::load(&would_be)?;
                let shells = crate::cli::install::shells(&would_be, &[])?;
                let reports = crate::cli::install::reconcile(&would_be, &registry, &shells, true)?;
                for report in &reports {
                    for command in &commands {
                        if let Some(row) = report.row(command) {
                            println!(
                                "{}: {} {}",
                                report.shell,
                                row.status.as_str(),
                                row.shim.display()
                            );
                            work |= row.status == ShimStatus::WouldWrite;
                        }
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
            std::fs::write(&config_path, &text)?;
        }

        if self.no_install {
            return Ok(ExitCode::SUCCESS);
        }

        let registry = Registry::load(&would_be)?;
        let shells = crate::cli::install::shells(&would_be, &[])?;
        let prewarm = crate::cli::install::prewarm_enabled(&would_be, false, self.no_prewarm)
            .then_some(crate::cache::Prewarm {
                shells: &shells,
                only: Some(&commands),
            });
        let notes = crate::cache::prefetch(&would_be, &registry, prewarm, true)?;
        let reports = crate::cli::install::reconcile(&would_be, &registry, &shells, false)?;
        for report in &reports {
            for command in &commands {
                if let Some(row) = report.row(command) {
                    println!(
                        "{}: {} {}",
                        report.shell,
                        row.status.as_str(),
                        row.shim.display()
                    );
                }
            }
        }
        for note in notes {
            println!("{note}");
        }
        let instructions: Vec<&String> = reports
            .iter()
            .filter(|report| !crate::shell::probe::loading_line_in_effect(&would_be, report.shell))
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

/// Whether the config already has the repo-less inline item for `name`, which
/// a new `add --run` replaces rather than refuses.
fn inline_item_exists(config: &Config, name: &str) -> bool {
    config
        .registry
        .iter()
        .any(|item| item.repo.is_none() && item.tools.iter().any(|tool| tool.name == name))
}

/// Run the described source once per declared shell, into a scratch cache, so
/// a command that does not work is reported before it is written. Returns the
/// number of lines each shell produced, or the first line of its error.
fn trial(
    would_be: &Config,
    name: &str,
    item: &crate::registry::RegistryItem,
) -> Result<Vec<(String, std::result::Result<usize, String>)>> {
    let bin = crate::source::resolve_binary(name).wrap_err_with(|| {
        format!("`{name}` is not on PATH; install it first, so its completion can be checked")
    })?;
    let scratch = std::env::temp_dir().join(format!("shcomp-add-{}", std::process::id()));
    let mut config = would_be.clone();
    config.settings.cache_dir = Some(scratch.clone());
    let registry = Registry::load(&config)?;
    let mut outcomes = Vec::new();
    for shell in &item.tools[0].shells {
        let outcome = try_shell(&config, &registry, name, shell, &bin, &scratch)
            .map_err(|error| first_lines(&error));
        outcomes.push((shell.clone(), outcome));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(outcomes)
}

fn try_shell(
    config: &Config,
    registry: &Registry,
    name: &str,
    shell: &str,
    bin: &Path,
    scratch: &Path,
) -> Result<usize> {
    let tool = &registry.resolve_str(name)?.tool;
    let source = tool
        .source(shell)
        .ok_or_else(|| eyre::eyre!("no {shell} source"))?;
    if let Some(url) = &source.url {
        let timeout = match &source.timeout {
            Some(timeout) => crate::config::parse_duration(timeout)?,
            None => config.settings.timeout()?,
        };
        crate::source::fetch_url(scratch, url, source.sha256.as_deref(), timeout)?;
    }
    let script = crate::cache::load_or_generate(config, registry, name, shell, bin)?;
    Ok(std::fs::read_to_string(script)?.lines().count())
}

/// The error's first line, plus the first line the command wrote to stderr.
fn first_lines(error: &eyre::Report) -> String {
    let text = format!("{error:#}");
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or("failed").to_string();
    match lines.next() {
        Some(stderr) => format!("{first} — {stderr}"),
        None => first,
    }
}
