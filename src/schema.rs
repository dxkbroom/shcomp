//! JSON Schema output for the config and registry-item formats.

use eyre::{Result, WrapErr};
use schemars::schema_for;

use crate::config::Config;
use crate::registry::RegistryItem;

pub fn config_schema() -> Result<String> {
    schema(&schema_for!(Config))
}

pub fn registry_schema() -> Result<String> {
    schema(&schema_for!(RegistryItem))
}

fn schema(schema: &schemars::Schema) -> Result<String> {
    serde_json::to_string_pretty(schema).wrap_err("cannot serialize JSON Schema")
}
