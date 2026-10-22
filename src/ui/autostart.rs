//! Start at login: HKCU Run value (Windows), LaunchAgent (macOS), XDG autostart entry (Linux).
//! `is_enabled` reads the OS state and is true only when the entry launches this executable.

use std::path::PathBuf;

fn exe() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Ok(p.into());
    }
    let exe = std::env::current_exe()?;
    // canonicalize would add a \\?\ prefix on Windows.
    #[cfg(not(windows))]
    let exe = exe.canonicalize()?;
    Ok(exe)
}

#[cfg(windows)]
mod imp {
    use windows_registry::CURRENT_USER;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Task Manager's Startup tab disables an entry here without touching the Run value.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "Hammurabi";

    fn command() -> anyhow::Result<String> {
        Ok(format!("\"{}\"", super::exe()?.display()))
    }

    pub fn is_enabled() -> bool {
        let ours = CURRENT_USER
            .open(RUN)
            .and_then(|k| k.get_string(NAME))
            .is_ok_and(|v| command().is_ok_and(|c| v.eq_ignore_ascii_case(&c)));
        // First byte: even = enabled, odd = disabled.
        let disabled = CURRENT_USER
            .open(APPROVED)
            .and_then(|k| k.get_value(NAME))
            .is_ok_and(|v| v.first().is_some_and(|b| b & 1 == 1));
        ours && !disabled
    }

    pub fn set(on: bool) -> anyhow::Result<()> {
        let _ = CURRENT_USER
            .options()
            .write()
            .open(APPROVED)
            .and_then(|k| k.remove_value(NAME));
        let key = CURRENT_USER.create(RUN)?;
        if on {
            key.set_string(NAME, command()?)?;
        } else {
            key.remove_value(NAME)?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    use anyhow::Context;

    /// (file, exact contents) of the login entry for this executable.
    #[cfg(target_os = "macos")]
    fn entry() -> anyhow::Result<(PathBuf, String)> {
        let home = directories::BaseDirs::new().context("no home directory")?;
        let exe = super::exe()?.display().to_string();
        let exe = exe
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.menzo.Hammurabi</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
        );
        Ok((
            home.home_dir()
                .join("Library/LaunchAgents/dev.menzo.Hammurabi.plist"),
            plist,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn entry() -> anyhow::Result<(PathBuf, String)> {
        let base = directories::BaseDirs::new().context("no home directory")?;
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Hammurabi\nComment=Live captions and translation\nExec=\"{}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            super::exe()?.display()
        );
        Ok((
            base.config_dir().join("autostart/hammurabi.desktop"),
            desktop,
        ))
    }

    pub fn is_enabled() -> bool {
        entry().is_ok_and(|(path, text)| std::fs::read_to_string(path).is_ok_and(|t| t == text))
    }

    pub fn set(on: bool) -> anyhow::Result<()> {
        let (path, text) = entry()?;
        if on {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, text)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

pub use imp::{is_enabled, set};
