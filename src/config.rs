use std::path::Path;

use anyhow::Context;

use crate::settings::Settings;

/// Missing fields default; a corrupt file is backed up and defaults are returned.
pub fn load() -> Settings {
    match crate::paths::config_file() {
        Ok(path) => load_from(&path),
        Err(_) => Settings::default(),
    }
}

/// Atomic write (temp file + rename).
pub fn save(settings: &Settings) -> anyhow::Result<()> {
    save_to(&crate::paths::config_file()?, settings)
}

fn load_from(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Settings::default();
    };
    match toml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("corrupt config {}: {e}; using defaults", path.display());
            let bak = path.with_extension("toml.bak");
            let _ = std::fs::remove_file(&bak);
            if let Err(e) = std::fs::rename(path, &bak) {
                log::warn!("config backup failed: {e}");
            }
            Settings::default()
        }
    }
}

fn save_to(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let dir = path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(settings)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ModelChoice, TranslatorChoice};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hammurabi-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trip() {
        let dir = tmp("rt");
        let path = dir.join("sub").join("config.toml");
        assert_eq!(load_from(&path), Settings::default());
        let s = Settings {
            model: ModelChoice::Turbo,
            translator: TranslatorChoice::Llm {
                base_url: "http://localhost:1234/v1".into(),
                model: "m".into(),
            },
            overlay_pos: Some([1.5, 2.5]),
            ..Settings::default()
        };
        save_to(&path, &s).unwrap();
        assert_eq!(load_from(&path), s);
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_and_missing_fields() {
        let dir = tmp("unk");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "future_flag = true\ntranslate = false\n[caption]\ncues = 5\nnew = 1\n",
        )
        .unwrap();
        let s = load_from(&path);
        assert!(!s.translate);
        assert_eq!(s.caption.cues, 5);
        assert_eq!(s.caption.trans_size, 30.0);
        assert!(!path.with_extension("toml.bak").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = tmp("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is [not toml").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        assert_eq!(
            std::fs::read_to_string(path.with_extension("toml.bak")).unwrap(),
            "this is [not toml"
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
