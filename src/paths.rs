#![allow(dead_code)]

use std::path::PathBuf;

/// `ProjectDirs::from("dev", "menzo", "Hammurabi")` config dir + `config.toml`.
pub fn config_file() -> anyhow::Result<PathBuf> {
    anyhow::bail!("unimplemented")
}

/// `cache_dir()/models`.
pub fn models_dir() -> anyhow::Result<PathBuf> {
    anyhow::bail!("unimplemented")
}
