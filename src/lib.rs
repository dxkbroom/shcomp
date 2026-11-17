//! shcomp: manage shell completions that follow the active tool version.

pub mod cache;
pub mod cli;
pub mod complete;
pub mod config;
pub mod dirs;
pub mod logging;
pub mod reference;
pub mod registry;
pub mod report;
pub mod schema;
pub mod shell;
pub mod source;
pub mod template;

pub use eyre::Result;
