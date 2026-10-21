#![allow(dead_code, unused_variables)]

use crate::settings::Settings;

/// Missing fields default; a corrupt file is backed up and defaults are returned.
pub fn load() -> Settings {
    Settings::default()
}

/// Atomic write (temp file + rename).
pub fn save(settings: &Settings) -> anyhow::Result<()> {
    anyhow::bail!("unimplemented")
}
