//! shcomp: manage shell completions that follow the active tool version.

pub mod cli;
pub mod config;
pub mod dirs;
pub mod reference;
pub mod registry;
pub mod schema;
pub mod template;

pub use eyre::Result;
