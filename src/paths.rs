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

/// App volumes to put back if Hammurabi dies while it has them turned down.
pub fn duck_journal() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.data_local_dir().join("ducked.json"))
}

/// A second launch reaches the running copy through this socket (Windows uses named events).
#[cfg(unix)]
pub fn instance_socket() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.data_local_dir().join("instance.sock"))
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.data_local_dir().join("logs"))
}
