//! `shcomp schema`: print the JSON Schema for the config or a registry item.

use eyre::Result;

#[derive(Debug, usage_rs::Args)]
pub struct Schema {
    /// Print the registry-item schema instead of the config schema
    #[usage(long)]
    pub registry: bool,
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
