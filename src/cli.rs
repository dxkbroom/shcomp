//! The command line surface.

use eyre::{Result, bail};

use crate::config::Config;
use crate::registry::Registry;

#[derive(Debug, usage_rs::Cli)]
#[usage(
    name = "shcomp",
    bin = "shcomp",
    version = "0.0.1",
    about = "Manage shell completions that follow the active tool version",
    arg_required_else_help = true,
    completion = true
)]
pub struct Cli {
    #[usage(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, usage_rs::Subcommands)]
pub enum Commands {
    /// Install and reconcile shell shims for the selected tools
    Install(Install),

    /// Remove shcomp-owned shims
    Uninstall(Uninstall),

    /// Print the cached script path for one command and executable
    #[usage(hide = true)]
    Load(Load),

    /// Probe completion candidates through the target shell
    CompleteWord(CompleteWord),

    /// Check the configuration and installed shims
    Doctor(Doctor),

    /// List selected tools and their completion sources
    Ls(Ls),

    /// Add a tool selection to the config
    Add(Add),

    /// Remove the completion cache
    Clean(Clean),

    /// Print the JSON Schema for the config or a registry item
    Schema(Schema),

    /// Print the usage KDL for shcomp itself
    Usage(Usage),

    /// Print a completion script for shcomp itself
    Completion(Completion),
}

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
}

#[derive(Debug, usage_rs::Args)]
pub struct Uninstall {
    /// Print what would be removed without removing it
    #[usage(long)]
    pub dry_run: bool,
}

#[derive(Debug, usage_rs::Args)]
pub struct Load {
    /// Target shell
    #[usage(long, default = "zsh")]
    pub shell: String,

    /// Command name as typed
    #[usage(long)]
    pub command: String,

    /// Resolved executable path
    #[usage(long)]
    pub bin: String,
}

#[derive(Debug, usage_rs::Args)]
pub struct CompleteWord {
    /// Target shell
    #[usage(long, default = "zsh")]
    pub shell: String,

    /// Index of the word being completed; defaults to the last word
    #[usage(long)]
    pub cword: Option<usize>,

    /// Tool reference, optionally with `@version`
    #[usage(arg)]
    pub tool: String,

    /// Words typed so far, starting with the command
    #[usage(arg, var_max = 4294967296)]
    pub words: Vec<String>,
}

#[derive(Debug, usage_rs::Args)]
pub struct Doctor {
    /// Run each generator and validate its output
    #[usage(long)]
    pub check: bool,
}

#[derive(Debug, usage_rs::Args)]
pub struct Ls {
    /// Machine-readable output
    #[usage(long)]
    pub json: bool,
}

#[derive(Debug, usage_rs::Args)]
pub struct Add {
    /// Tool reference to add
    #[usage(arg)]
    pub reference: String,

    /// Extra command name to register; repeat for several
    #[usage(long, var)]
    pub alias: Vec<String>,
}

#[derive(Debug, usage_rs::Args)]
pub struct Clean;

#[derive(Debug, usage_rs::Args)]
pub struct Schema {
    /// Print the registry-item schema instead of the config schema
    #[usage(long)]
    pub registry: bool,
}

#[derive(Debug, usage_rs::Args)]
pub struct Usage;

#[derive(Debug, usage_rs::Args)]
pub struct Completion {
    /// Shell to generate completions for
    #[usage(arg, choices("bash", "fish", "nu", "powershell", "zsh"))]
    pub shell: String,

    /// Write the script where the shell looks for it
    #[usage(long)]
    pub install: bool,
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(command) => command.run(),
            None => {
                if let Some(help) = Self::render_help(Self::spec().root.cmd, false) {
                    println!("{help}");
                }
                Ok(())
            }
        }
    }
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Install(command) => command.run(),
            Self::Uninstall(command) => command.run(),
            Self::Load(command) => command.run(),
            Self::CompleteWord(command) => command.run(),
            Self::Doctor(command) => command.run(),
            Self::Ls(command) => command.run(),
            Self::Add(command) => command.run(),
            Self::Clean(command) => command.run(),
            Self::Schema(command) => command.run(),
            Self::Usage(command) => command.run(),
            Self::Completion(command) => command.run(),
        }
    }
}

impl Install {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        if selected.is_empty() {
            bail!("no tools are selected; add one with `shcomp add <tool>`");
        }
        // The zsh installer arrives in the shell layer; keep this command honest
        // until then rather than pretending a no-op installed anything.
        bail!("install is implemented by the shell layer");
    }
}

impl Uninstall {
    pub fn run(self) -> Result<()> {
        bail!("uninstall is implemented by the shell layer");
    }
}

impl Load {
    pub fn run(self) -> Result<()> {
        bail!("load is implemented by the source layer");
    }
}

impl CompleteWord {
    pub fn run(self) -> Result<()> {
        bail!("complete-word is implemented by the harness layer");
    }
}

impl Doctor {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        println!(
            "config: {}",
            config.source.as_deref().map_or_else(
                || "not found".to_string(),
                |path| path.display().to_string()
            )
        );
        println!("registry items: {}", registry.entries().len());
        println!("selected tools: {}", selected.len());
        for tool in &selected {
            println!(
                "  {} -> {} ({})",
                tool.reference,
                tool.resolved.tool.name,
                tool.resolved.repo_slug().unwrap_or_else(|| "-".to_string())
            );
        }
        Ok(())
    }
}

impl Ls {
    pub fn run(self) -> Result<()> {
        let config = Config::load_default()?;
        let registry = Registry::load(&config)?;
        let selected = registry.selected(&config)?;
        if self.json {
            let rows: Vec<serde_json::Value> = selected
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "reference": tool.reference,
                        "repo": tool.resolved.repo_slug(),
                        "tool": tool.resolved.tool.name,
                        "aliases": tool.aliases,
                        "shells": tool.resolved.tool.completions.keys().collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        for tool in &selected {
            let shells = tool
                .resolved
                .tool
                .completions
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{}\t{}\t{}\t{}",
                tool.reference,
                tool.resolved.tool.name,
                tool.resolved.repo_slug().unwrap_or_else(|| "-".to_string()),
                shells
            );
        }
        Ok(())
    }
}

impl Add {
    pub fn run(self) -> Result<()> {
        let _ = &self.reference;
        let _ = &self.alias;
        bail!("add is implemented by the registry layer");
    }
}

impl Clean {
    pub fn run(self) -> Result<()> {
        let cache = crate::dirs::cache_dir()?;
        if cache.exists() {
            std::fs::remove_dir_all(&cache)?;
            println!("removed {}", cache.display());
        }
        Ok(())
    }
}

impl Schema {
    pub fn run(self) -> Result<()> {
        let schema = if self.registry {
            crate::schema::registry_schema()?
        } else {
            crate::schema::config_schema()?
        };
        println!("{schema}");
        Ok(())
    }
}

impl Usage {
    pub fn run(self) -> Result<()> {
        println!("{}", Cli::to_kdl());
        Ok(())
    }
}

impl Completion {
    pub fn run(self) -> Result<()> {
        let shell = usage_rs::complete::Shell::from_name(&self.shell)
            .ok_or_else(|| eyre::eyre!("unsupported shell `{}`", self.shell))?;
        let script = usage_rs::script::script("shcomp", shell);
        if self.install {
            bail!("completion --install is not implemented yet; redirect the script instead");
        }
        println!("{script}");
        Ok(())
    }
}
