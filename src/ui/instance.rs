//! How a second launch reaches the copy that is already running.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Show,
    Quit,
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForMultipleObjects,
    };
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    use windows::core::{PCWSTR, w};

    use super::Signal;

    const SHOW: PCWSTR = w!(r"Local\dev.menzo.hammurabi.show");
    const QUIT: PCWSTR = w!(r"Local\dev.menzo.hammurabi.quit");

    pub fn send(signal: Signal) -> anyhow::Result<()> {
        let name = if signal == Signal::Quit { QUIT } else { SHOW };
        // SAFETY: the names are static, and the handle is closed before returning.
        unsafe {
            // Windows only lets a background app take focus if the app the user just started allows it.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let event = OpenEventW(EVENT_MODIFY_STATE, false, name)?;
            let sent = SetEvent(event);
            let _ = CloseHandle(event);
            sent?;
        }
        Ok(())
    }

    pub fn listen(on: impl Fn(Signal) + Send + 'static) {
        let _ = std::thread::Builder::new()
            .name("hm-instance".into())
            .spawn(move || {
                // SAFETY: the names are static; the handles live as long as this thread.
                let events = unsafe {
                    match (
                        CreateEventW(None, false, false, SHOW),
                        CreateEventW(None, false, false, QUIT),
                    ) {
                        (Ok(show), Ok(quit)) => [show, quit],
                        (show, quit) => {
                            log::warn!("no second-launch signal: {:?}", show.and(quit));
                            return;
                        }
                    }
                };
                loop {
                    // SAFETY: `events` holds two open handles.
                    let hit = unsafe { WaitForMultipleObjects(&events, false, INFINITE) };
                    match hit.0.wrapping_sub(WAIT_OBJECT_0.0) {
                        0 => on(Signal::Show),
                        1 => on(Signal::Quit),
                        _ => return,
                    }
                }
            });
    }
}

#[cfg(unix)]
mod imp {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::Signal;

    pub fn send(signal: Signal) -> anyhow::Result<()> {
        let byte = if signal == Signal::Quit { b"q" } else { b"s" };
        UnixStream::connect(crate::paths::instance_socket()?)?.write_all(byte)?;
        Ok(())
    }

    fn bind() -> anyhow::Result<UnixListener> {
        let path = crate::paths::instance_socket()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Only the holder of the single-instance lock gets here, so any socket left is stale.
        let _ = std::fs::remove_file(&path);
        Ok(UnixListener::bind(path)?)
    }

    pub fn listen(on: impl Fn(Signal) + Send + 'static) {
        let listener = match bind() {
            Ok(l) => l,
            Err(e) => {
                log::warn!("no second-launch signal: {e:#}");
                return;
            }
        };
        let _ = std::thread::Builder::new()
            .name("hm-instance".into())
            .spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).is_ok() {
                        on(if byte == *b"q" {
                            Signal::Quit
                        } else {
                            Signal::Show
                        });
                    }
                }
            });
    }
}

pub use imp::{listen, send};
