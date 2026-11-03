//! Turns other apps down while a voice reads. Windows only: it has a volume per app session, but
//! it also remembers that volume for the app's next run, so what was changed is written to a
//! journal first and put back on the next start if Hammurabi dies before it can.

pub const SUPPORTED: bool = cfg!(windows);

#[cfg(windows)]
pub use imp::{Ducker, recover};

#[cfg(not(windows))]
#[derive(Default)]
pub struct Ducker(());

#[cfg(not(windows))]
impl Ducker {
    pub fn duck(&mut self, _level: f32) {}
    pub fn release(&mut self) {}
    pub fn active(&self) -> bool {
        false
    }
    pub fn gain(&self) -> f32 {
        1.0
    }
}

#[cfg(not(windows))]
pub fn recover() {}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use std::time::Duration;

    use serde::{Deserialize, Serialize};
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::Media::Audio::{
        AudioSessionStateActive, AudioSessionStateExpired, DEVICE_STATE_ACTIVE,
        IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
        MMDeviceEnumerator, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    };
    use windows::core::Interface;

    /// A session whose volume was lowered.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub(super) struct Entry {
        pub(super) session: String,
        pub(super) original: f32,
        pub(super) ducked: f32,
    }

    /// The user moved the volume themselves when it is no longer what was set; theirs then stands.
    pub(super) fn untouched(current: f32, ducked: f32) -> bool {
        (current - ducked).abs() < 0.01
    }

    const STEPS: u32 = 6;
    const STEP: Duration = Duration::from_millis(25);

    struct Session {
        id: String,
        control: IAudioSessionControl2,
        volume: ISimpleAudioVolume,
    }

    impl Session {
        fn expired(&self) -> bool {
            // SAFETY: a plain COM call on a live interface.
            unsafe { self.control.GetState() }.is_ok_and(|s| s == AudioSessionStateExpired)
        }

        fn active(&self) -> bool {
            // SAFETY: as in `expired`.
            unsafe { self.control.GetState() }.is_ok_and(|s| s == AudioSessionStateActive)
        }

        fn get(&self) -> windows::core::Result<f32> {
            // SAFETY: as in `expired`.
            unsafe { self.volume.GetMasterVolume() }
        }

        fn set(&self, v: f32) -> windows::core::Result<()> {
            // SAFETY: as in `expired`; a null event context is allowed.
            unsafe { self.volume.SetMasterVolume(v, std::ptr::null()) }
        }
    }

    /// Every other app's audio session on every active output.
    fn sessions() -> windows::core::Result<Vec<Session>> {
        let own = std::process::id();
        let mut out = Vec::new();
        // SAFETY: COM calls on live interfaces; the id string is copied, then freed as documented.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            for d in 0..devices.GetCount()? {
                let manager: IAudioSessionManager2 = devices.Item(d)?.Activate(CLSCTX_ALL, None)?;
                let list = manager.GetSessionEnumerator()?;
                for s in 0..list.GetCount()? {
                    let control: IAudioSessionControl2 = list.GetSession(s)?.cast()?;
                    if control.GetProcessId().is_ok_and(|p| p == own)
                        || control.IsSystemSoundsSession() == S_OK
                    {
                        continue;
                    }
                    let id = control.GetSessionIdentifier()?;
                    let text = id.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(id.as_ptr().cast()));
                    out.push(Session {
                        id: text,
                        volume: control.cast()?,
                        control,
                    });
                }
            }
        }
        Ok(out)
    }

    fn journal_path() -> anyhow::Result<PathBuf> {
        crate::paths::duck_journal()
    }

    fn read_journal() -> Vec<Entry> {
        journal_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// False when it could not be written, and so nothing may be turned down.
    fn write_journal(entries: &[Entry]) -> bool {
        let write = || -> anyhow::Result<()> {
            let path = journal_path()?;
            if entries.is_empty() {
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
                return Ok(());
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            Ok(std::fs::write(path, serde_json::to_vec(entries)?)?)
        };
        write()
            .inspect_err(|e| log::warn!("volume journal: {e:#}"))
            .is_ok()
    }

    /// Puts back what a previous run left turned down. An app that is not running stays in the
    /// journal, for a later try.
    pub fn recover() {
        let entries = read_journal();
        if entries.is_empty() {
            return;
        }
        let Ok(all) = sessions() else { return };
        let mut left = Vec::new();
        for e in entries {
            let found: Vec<_> = all
                .iter()
                .filter(|s| s.id == e.session && !s.expired())
                .collect();
            if found.is_empty() {
                left.push(e);
                continue;
            }
            for s in found {
                if s.get().is_ok_and(|v| untouched(v, e.ducked)) {
                    log::info!("restoring an app's volume left low by an earlier run");
                    let _ = s.set(e.original);
                }
            }
        }
        write_journal(&left);
    }

    /// The turned down sessions. Lives on one thread, and puts them back when dropped.
    #[derive(Default)]
    pub struct Ducker {
        ducked: Vec<(Session, Entry)>,
        level: Option<f32>,
    }

    impl Ducker {
        pub fn active(&self) -> bool {
            !self.ducked.is_empty()
        }

        /// How much to raise captured system audio to make up for the lowered apps.
        pub fn gain(&self) -> f32 {
            self.level
                .filter(|_| self.active())
                .map_or(1.0, |l| 1.0 / l)
        }

        /// Lowers playing apps to `level` (0..1) of their volume, including ones started since
        /// the last call. Keeps audio playing at its volume on any failure.
        pub fn duck(&mut self, level: f32) {
            if !self.active() {
                recover();
            }
            let all = match sessions() {
                Ok(all) => all,
                Err(e) => return log::warn!("listing audio sessions: {e}"),
            };
            let mut new = Vec::new();
            for s in all {
                if !s.active() || self.ducked.iter().any(|(d, _)| d.id == s.id) {
                    continue;
                }
                let Ok(original) = s.get() else { continue };
                let e = Entry {
                    session: s.id.clone(),
                    original,
                    ducked: original * level,
                };
                new.push((s, e));
            }
            if new.is_empty() {
                return;
            }
            let mut journal = read_journal();
            for (_, e) in &new {
                journal.retain(|j| j.session != e.session);
                journal.push(e.clone());
            }
            if !write_journal(&journal) {
                return;
            }
            ramp(&new.iter().collect::<Vec<_>>(), false);
            self.level = Some(level);
            self.ducked.extend(new);
        }

        /// Puts back every session whose volume the user has not changed meanwhile.
        pub fn release(&mut self) {
            if !self.active() {
                return;
            }
            let ours: Vec<_> = self
                .ducked
                .iter()
                .filter(|(s, e)| s.get().is_ok_and(|v| untouched(v, e.ducked)))
                .collect();
            ramp(&ours, true);
            // A session that ended cannot take its volume back now; the journal tries later.
            let done: Vec<&str> = self
                .ducked
                .iter()
                .filter(|(s, _)| !s.expired())
                .map(|(_, e)| e.session.as_str())
                .collect();
            let left: Vec<Entry> = read_journal()
                .into_iter()
                .filter(|j| !done.contains(&j.session.as_str()))
                .collect();
            write_journal(&left);
            self.ducked.clear();
        }
    }

    impl Drop for Ducker {
        fn drop(&mut self) {
            self.release();
        }
    }

    /// Moves the volumes between original and ducked in small steps, so nothing clicks.
    fn ramp(sessions: &[&(Session, Entry)], back: bool) {
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let t = if back { 1.0 - t } else { t };
            for (s, e) in sessions {
                let _ = s.set(e.original + (e.ducked - e.original) * t);
            }
            std::thread::sleep(STEP);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::imp::{Entry, untouched};

    #[test]
    fn leaves_a_volume_the_user_moved() {
        assert!(untouched(0.3, 0.3));
        assert!(untouched(0.305, 0.3));
        assert!(!untouched(0.5, 0.3));
    }

    #[test]
    fn journal_round_trips() {
        let e = vec![Entry {
            session: "a|b".into(),
            original: 0.8,
            ducked: 0.24,
        }];
        let back: Vec<Entry> = serde_json::from_slice(&serde_json::to_vec(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
