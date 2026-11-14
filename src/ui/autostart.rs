//! Start at login: HKCU Run value (Windows; the package's startup task when installed from the
//! Microsoft Store), LaunchAgent (macOS), XDG autostart entry (Linux).
//! `is_enabled` reads the OS state and is true only when the entry launches this executable.
//! Entries from before `--background` existed still count; the app rewrites them on start.

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
    // A quarantined app opened in place runs from a random read-only copy that is gone at next login.
    #[cfg(target_os = "macos")]
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        anyhow::bail!("move Hammurabi.app to the Applications folder first");
    }
    Ok(exe)
}

#[cfg(windows)]
mod imp {
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::core::HSTRING;
    use windows_registry::CURRENT_USER;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Task Manager's Startup tab disables an entry here without touching the Run value.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "Hammurabi";

    fn command() -> anyhow::Result<String> {
        Ok(format!(
            "\"{}\" {}",
            super::exe()?.display(),
            crate::ui::BACKGROUND_ARG
        ))
    }

    /// The startup task the package declares (packaging/windows/msix/AppxManifest.xml): a
    /// packaged app's Run value is not read at login.
    const TASK: &str = "HammurabiStartup";

    /// Asks the package's startup task, on a thread of its own: Windows answers through a
    /// call that is waited for, which the window's thread must not do.
    fn with_task<T: Send + 'static>(
        ask: impl FnOnce(StartupTask) -> windows::core::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        std::thread::spawn(move || ask(StartupTask::GetAsync(&HSTRING::from(TASK))?.join()?))
            .join()
            .map_err(|_| anyhow::anyhow!("the startup task could not be asked"))?
            .map_err(Into::into)
    }

    fn enabled(state: StartupTaskState) -> bool {
        state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy
    }

    pub fn is_enabled() -> bool {
        if crate::ui::platform::packaged() {
            return with_task(|task| task.State()).is_ok_and(enabled);
        }
        let ours = CURRENT_USER
            .open(RUN)
            .and_then(|k| k.get_string(NAME))
            .is_ok_and(|v| {
                command().is_ok_and(|c| {
                    let old = c.replace(&format!(" {}", crate::ui::BACKGROUND_ARG), "");
                    v.eq_ignore_ascii_case(&c) || v.eq_ignore_ascii_case(&old)
                })
            });
        // First byte: even = enabled, odd = disabled.
        let disabled = CURRENT_USER
            .open(APPROVED)
            .and_then(|k| k.get_value(NAME))
            .is_ok_and(|v| v.first().is_some_and(|b| b & 1 == 1));
        ours && !disabled
    }

    pub fn set(on: bool) -> anyhow::Result<()> {
        if crate::ui::platform::packaged() {
            if !on {
                return with_task(|task| task.Disable());
            }
            // Turned off in Windows' own settings, it can only be turned on again there.
            let state = with_task(|task| task.RequestEnableAsync()?.join())?;
            anyhow::ensure!(
                enabled(state),
                "Windows keeps Hammurabi from starting at login. Allow it in Windows Settings > Apps > Startup."
            );
            return Ok(());
        }
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
        <string>{arg}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
            arg = crate::ui::BACKGROUND_ARG
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
            "[Desktop Entry]\nType=Application\nName=Hammurabi\nComment=Live captions and translation\nExec=\"{}\" {}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            super::exe()?.display(),
            crate::ui::BACKGROUND_ARG
        );
        Ok((
            base.config_dir().join("autostart/hammurabi.desktop"),
            desktop,
        ))
    }

    /// The entry as written before `--background` existed.
    fn old_entry(text: &str) -> String {
        let arg = crate::ui::BACKGROUND_ARG;
        text.replace(&format!("        <string>{arg}</string>\n"), "")
            .replace(&format!(" {arg}"), "")
    }

    pub fn is_enabled() -> bool {
        entry().is_ok_and(|(path, text)| {
            std::fs::read_to_string(path).is_ok_and(|t| t == text || t == old_entry(&text))
        })
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
