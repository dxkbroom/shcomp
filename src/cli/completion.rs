//! `shcomp completion`: print a completion script for shcomp itself.

use eyre::Result;

#[derive(Debug, usage_rs::Args)]
pub struct Completion {
    /// Shell to generate completions for
    #[usage(arg, choices("bash", "fish", "nu", "powershell", "zsh"))]
    pub shell: String,
}

impl Completion {
    pub fn run(self) -> Result<()> {
        let shell = usage_rs::complete::Shell::from_name(&self.shell)
            .ok_or_else(|| eyre::eyre!("unsupported shell `{}`", self.shell))?;
        println!("{}", usage_rs::script::script("shcomp", shell));
        Ok(())
    }
}
