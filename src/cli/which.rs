//! `shcomp which`: explain how one command is completed.
//!
//! Everything here is read from the config, the registry, the cache, and the
//! shim directories. The vendor is never run: the version comes from mise's
//! install records or the path shape, never from `--version`, and the only
//! process started is `mise completion`, to report a stub.

use std::process::ExitCode;

use eyre::Result;
use usage_rs::complete::{Candidate, CompleteCtx};

use crate::config::Config;
use crate::registry::{Registry, SelectedTool, Source};

#[derive(Debug, usage_rs::Args)]
pub struct Which {
    /// Only this shell; defaults to every installed shell
    #[usage(long, short = 's', choices("zsh", "fish"))]
    pub shell: Option<String>,

    /// Machine-readable output
    #[usage(long)]
    pub json: bool,

    /// The command as you type it; an alias works too
    #[usage(arg, complete = crate::cli::which::complete_command)]
    pub command: String,
}

/// The completer for `which <COMMAND>`: the selected commands.
fn complete_command(
    _which: &<Which as usage_rs::spec::CommandArgs>::Partial,
    ctx: &CompleteCtx<'_>,
) -> Vec<Candidate<'static>> {
    crate::search::selected_candidates(ctx.prefix)
}

impl Which {
    pub fn run(self) -> Result<ExitCode> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selections = registry.selections(&config)?;
        let Some(tool) = selections.iter().find(|tool| {
            tool.resolved.tool.name == self.command || tool.aliases.contains(&self.command)
        }) else {
            return not_selected(&registry, &self.command, self.json);
        };
        let shells: Vec<String> = match &self.shell {
            Some(shell) => vec![shell.clone()],
            None => config
                .settings
                .shells
                .iter()
                .filter(|shell| crate::shell::managed(shell))
                .cloned()
                .collect(),
        };
        let report = explain(&config, tool, &shells)?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_report(&report);
        }
        Ok(ExitCode::SUCCESS)
    }
}

fn not_selected(registry: &Registry, command: &str, json: bool) -> Result<ExitCode> {
    let (message, suggestion) = match registry.resolve_str(command) {
        Ok(resolved) => (
            format!(
                "`{command}` is not selected; the registry has it as {}",
                resolved
                    .repo_slug()
                    .unwrap_or_else(|| resolved.tool.name.clone())
            ),
            format!("shcomp add {command}"),
        ),
        Err(_) => (
            format!("`{command}` is not selected and no registry knows it"),
            format!(
                "shcomp add {command} --run '{command} <args that print a completion for {{shell}}>'"
            ),
        ),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "command": command,
                "selected": false,
                "message": message,
                "suggestion": suggestion,
            }))?
        );
    } else {
        println!("{message}");
        println!("to add it: {suggestion}");
    }
    Ok(ExitCode::FAILURE)
}

/// The registry layer an item came from, as the user would name it.
fn layer(config: &Config, tool: &SelectedTool) -> (String, Option<String>) {
    match &tool.resolved.source_path {
        None => ("embedded".to_string(), None),
        Some(path) if Some(path) == config.source.as_ref() => (
            "inline registry".to_string(),
            Some(path.display().to_string()),
        ),
        Some(path) => (
            "user registry".to_string(),
            Some(path.display().to_string()),
        ),
    }
}

fn describe_source(source: &Source) -> String {
    let what = if let Some(run) = &source.run {
        run.join(" ")
    } else if let Some(url) = &source.url {
        url.clone()
    } else if let Some(path) = &source.path {
        path.clone()
    } else {
        String::new()
    };
    format!("{}: {what}", source.kind_name())
}

fn explain(config: &Config, tool: &SelectedTool, shells: &[String]) -> Result<serde_json::Value> {
    let command = &tool.resolved.tool.name;
    let (layer, layer_path) = layer(config, tool);
    let bin = crate::source::resolve_binary(command).ok();
    let real = bin.as_deref().map(crate::source::real_path);
    let stamp = bin
        .as_deref()
        .and_then(|bin| crate::cache::BinStamp::of(bin).ok());
    let root = crate::cache::cache_root(config)?;
    let records: Vec<_> = shells
        .iter()
        .flat_map(|shell| crate::state::records(config, shell))
        .filter(|record| record.command == *command || tool.aliases.contains(&record.command))
        .collect();
    let now = crate::cache::unix_now();

    let mut per_shell = serde_json::Map::new();
    for shell in shells {
        let Some(source) = tool.resolved.tool.source(shell) else {
            per_shell.insert(
                shell.clone(),
                serde_json::json!({ "source": null, "message": format!("no {shell} source") }),
            );
            continue;
        };
        let (state, key, script) = match &stamp {
            None => ("missing", None, None),
            Some(stamp) => {
                let key = crate::cache::key_with(stamp, &source, shell)?;
                let script = crate::cache::script_cache_path(&root, shell, command, &key)?;
                let state = if script.exists() { "warm" } else { "cold" };
                (state, Some(key), Some(script))
            }
        };
        let shim = match shell.as_str() {
            "zsh" => crate::shell::zsh::shim_path(config, command)?,
            _ => crate::shell::fish::shim_path(config, command)?,
        };
        let shim_state = match std::fs::read_to_string(&shim) {
            Ok(text) if text.contains("# @generated by shcomp") => "installed",
            Ok(_) => "foreign file",
            Err(_) => "missing",
        };
        let last = records
            .iter()
            .find(|record| record.shell == *shell)
            .map(|record| {
                let outcome = if record.ok {
                    "ok".to_string()
                } else {
                    let mut failed = format!(
                        "failed: {}",
                        record.error.as_deref().unwrap_or("unknown error")
                    );
                    if let Some(stderr) = &record.stderr {
                        failed.push_str(&format!(" — {stderr}"));
                    }
                    failed
                };
                serde_json::json!({
                    "ok": record.ok,
                    "at": record.at,
                    "summary": format!("{outcome} {}", crate::state::ago(record.at, now)),
                })
            });
        per_shell.insert(
            shell.clone(),
            serde_json::json!({
                "source": describe_source(&source),
                "cache": {
                    "state": state,
                    "key": key,
                    "script": script.map(|path| path.display().to_string()),
                },
                "shim": { "path": shim.display().to_string(), "state": shim_state },
                "last_load": last,
            }),
        );
    }

    let selection = if tool.implicit {
        "implicit (shcomp completes itself)".to_string()
    } else {
        let mut how = format!("`{}` in [tools]", tool.reference);
        if !tool.aliases.is_empty() {
            how.push_str(&format!(", aliases {}", tool.aliases.join(", ")));
        }
        if let Some(shells) = &tool.shells {
            how.push_str(&format!(", shells {}", shells.join(", ")));
        }
        if tool.disabled {
            how.push_str(", disabled");
        }
        how
    };

    Ok(serde_json::json!({
        "command": command,
        "selected": true,
        "selection": selection,
        "reference": tool.reference,
        "aliases": tool.aliases,
        "disabled": tool.disabled,
        "implicit": tool.implicit,
        "config": config.source.as_ref().map(|path| path.display().to_string()),
        "registry": {
            "item": tool.resolved.repo_slug().unwrap_or_else(|| format!("tool:{command}")),
            "layer": layer,
            "path": layer_path,
        },
        "binary": {
            "path": bin.as_ref().map(|path| path.display().to_string()),
            "real": real.as_ref().map(|path| path.display().to_string()),
            "version": real.as_deref().and_then(crate::source::version_for),
        },
        "mise_stub": bin.is_some() && crate::shell::zsh::mise_owned(command),
        "shells": per_shell,
    }))
}

fn print_report(report: &serde_json::Value) {
    let text = |value: &serde_json::Value| value.as_str().unwrap_or("-").to_string();
    let line = |label: &str, value: String| println!("  {label:<10} {value}");
    println!("{}", text(&report["command"]));
    let mut selection = text(&report["selection"]);
    if let Some(config) = report["config"].as_str()
        && report["implicit"] != true
    {
        selection.push_str(&format!(" ({config})"));
    }
    line("selected", selection);
    let registry = &report["registry"];
    let mut layer = format!("{} — {}", text(&registry["item"]), text(&registry["layer"]));
    if let Some(path) = registry["path"].as_str() {
        layer.push_str(&format!(" ({path})"));
    }
    line("registry", layer);
    let binary = &report["binary"];
    match binary["path"].as_str() {
        None => line("binary", "not on PATH".to_string()),
        Some(path) => {
            let mut value = path.to_string();
            if let Some(real) = binary["real"].as_str()
                && real != path
            {
                value.push_str(&format!(" → {real}"));
            }
            if let Some(version) = binary["version"].as_str() {
                value.push_str(&format!(" ({version})"));
            }
            line("binary", value);
        }
    }
    line(
        "mise",
        if report["mise_stub"] == true {
            "also completes it (a packslip stub); shcomp's shim wins".to_string()
        } else {
            "no stub".to_string()
        },
    );
    if let Some(shells) = report["shells"].as_object() {
        for (shell, info) in shells {
            if info["source"].is_null() {
                line(shell, text(&info["message"]));
                continue;
            }
            line(shell, text(&info["source"]));
            let cache = &info["cache"];
            let mut value = format!("cache {}", text(&cache["state"]));
            if let Some(key) = cache["key"].as_str() {
                value.push_str(&format!(", key {key}"));
            }
            line("", value);
            if let Some(script) = cache["script"].as_str() {
                line("", format!("script {script}"));
            }
            line(
                "",
                format!(
                    "shim {} ({})",
                    text(&info["shim"]["path"]),
                    text(&info["shim"]["state"])
                ),
            );
            match info["last_load"]["summary"].as_str() {
                Some(summary) => line("", format!("last load {summary}")),
                None => line("", "last load never".to_string()),
            }
        }
    }
}
