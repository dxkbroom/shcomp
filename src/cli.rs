//! The command line surface.
//!
//! This file owns the root `Cli`, the `Commands` enum, and dispatch. Each
//! command lives in its own module, with its flags and its work beside each
//! other; a command that grows subcommands becomes a directory.

use std::process::ExitCode;

use eyre::Result;

pub mod add;
pub mod clean;
pub mod complete_word;
pub mod completion;
pub mod doctor;
pub mod install;
pub mod load;
pub mod ls;
pub mod schema;
pub mod search;
pub mod uninstall;
pub mod usage;

pub use add::Add;
pub use clean::Clean;
pub use complete_word::CompleteWord;
pub use completion::Completion;
pub use doctor::Doctor;
pub use install::Install;
pub use load::Load;
pub use ls::Ls;
pub use schema::Schema;
pub use search::Search;
pub use uninstall::Uninstall;
pub use usage::Usage;

#[derive(Debug, usage_rs::Cli)]
#[usage(
    name = "shcomp",
    bin = "shcomp",
    version = "0.0.2",
    about = "Manage shell completions that follow the active tool version",
    arg_required_else_help = true,
    completion = true
)]
pub struct Cli {
    /// Print more detail; repeat for more
    #[usage(short = 'v', long, count, global)]
    pub verbose: u8,

    /// Report errors only
    #[usage(short = 'q', long, global)]
    pub quiet: bool,

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

    /// Search the registry for a tool to add
    Search(Search),

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

impl Cli {
    pub fn run(self) -> Result<ExitCode> {
        match self.command {
            Some(command) => command.run(),
            None => {
                if let Some(help) = Self::render_help(Self::spec().root.cmd, false) {
                    println!("{help}");
                }
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

impl Commands {
    /// The command's name, for tracing: one line saying what ran is the first
    /// thing a `-v` user wants.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Install(_) => "install",
            Self::Uninstall(_) => "uninstall",
            Self::Load(_) => "load",
            Self::CompleteWord(_) => "complete-word",
            Self::Doctor(_) => "doctor",
            Self::Ls(_) => "ls",
            Self::Search(_) => "search",
            Self::Add(_) => "add",
            Self::Clean(_) => "clean",
            Self::Schema(_) => "schema",
            Self::Usage(_) => "usage",
            Self::Completion(_) => "completion",
        }
    }

    /// Dispatch, and report the exit code the command asks for.
    ///
    /// Only `doctor` can fail a check without failing the process: a found
    /// problem is a result, not an error, so it exits 1 without an eyre report.
    pub fn run(self) -> Result<ExitCode> {
        tracing::info!(command = self.name(), "dispatch");
        match self {
            Self::Install(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Uninstall(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Load(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::CompleteWord(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Doctor(command) => command.run(),
            Self::Ls(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Search(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Add(command) => command.run(),
            Self::Clean(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Schema(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Usage(command) => command.run().map(|()| ExitCode::SUCCESS),
            Self::Completion(command) => command.run().map(|()| ExitCode::SUCCESS),
        }
    }
}
