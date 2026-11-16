//! Resolve the directories shcomp reads and writes.

use std::path::PathBuf;

use eyre::Result;

pub fn config_file() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SHCOMP_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    Ok(config_dir()?.join("shcomp").join("config.toml"))
}

pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir().ok_or_else(|| eyre::eyre!("cannot determine the config directory"))
}

pub fn registry_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SHCOMP_REGISTRY_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(config_dir()?.join("shcomp").join("registry"))
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SHCOMP_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::data_dir()
        .map(|dir| dir.join("shcomp"))
        .ok_or_else(|| eyre::eyre!("cannot determine the data directory"))
}

pub fn cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SHCOMP_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::cache_dir()
        .map(|dir| dir.join("shcomp"))
        .ok_or_else(|| eyre::eyre!("cannot determine the cache directory"))
}

pub fn zsh_site_functions() -> Result<PathBuf> {
    Ok(data_dir()?.join("zsh").join("site-functions"))
}
