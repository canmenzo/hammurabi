use std::path::PathBuf;

use anyhow::Context;
use directories::ProjectDirs;

fn dirs() -> anyhow::Result<ProjectDirs> {
    ProjectDirs::from("dev", "menzo", "Hammurabi").context("no home directory")
}

pub fn config_file() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.config_dir().join("config.toml"))
}

/// `HAMMURABI_MODELS_DIR` overrides the cache location.
pub fn models_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("HAMMURABI_MODELS_DIR").filter(|d| !d.is_empty()) {
        return Ok(dir.into());
    }
    Ok(dirs()?.cache_dir().join("models"))
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.data_local_dir().join("logs"))
}
