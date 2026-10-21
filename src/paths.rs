use std::path::PathBuf;

use anyhow::Context;
use directories::ProjectDirs;

fn dirs() -> anyhow::Result<ProjectDirs> {
    ProjectDirs::from("dev", "menzo", "Hammurabi").context("no home directory")
}

pub fn config_file() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.config_dir().join("config.toml"))
}

pub fn models_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs()?.cache_dir().join("models"))
}
