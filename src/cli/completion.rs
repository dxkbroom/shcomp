//! `shcomp completion`: print a completion script for shcomp itself.

use eyre::{Result, bail};

#[derive(Debug, usage_rs::Args)]
pub struct Completion {
    /// Shell to generate completions for
    #[usage(arg, choices("bash", "fish", "nu", "powershell", "zsh"))]
    pub shell: String,

    /// Write the script where the shell looks for it
    #[usage(long)]
    pub install: bool,
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
