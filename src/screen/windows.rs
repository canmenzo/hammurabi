//! The windows and displays there are to read from.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};
use windows::core::BOOL;

/// Every window with a title that can be seen, front to back: not Hammurabi's own, and not
/// the ones Windows keeps cloaked (its input panel, apps on another desktop).
pub fn list() -> Vec<(HWND, String)> {
    unsafe extern "system" fn each(w: HWND, out: LPARAM) -> BOOL {
        let out = unsafe { &mut *(out.0 as *mut Vec<(HWND, String)>) };
        let n = unsafe { GetWindowTextLengthW(w) };
        let (mut process, mut cloaked) = (0u32, 0u32);
        unsafe {
            GetWindowThreadProcessId(w, Some(&mut process));
            let _ = DwmGetWindowAttribute(w, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4);
        }
        if n > 0
            && unsafe { IsWindowVisible(w) }.as_bool()
            && process != std::process::id()
            && cloaked == 0
        {
            let mut buf = vec![0u16; n as usize + 1];
            let n = unsafe { GetWindowTextW(w, &mut buf) };
            out.push((w, String::from_utf16_lossy(&buf[..n.max(0) as usize])));
        }
        true.into()
    }
    let mut out: Vec<(HWND, String)> = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
    out
}

/// The frontmost window whose title holds `part` (letter case aside) and is not minimized:
/// a minimized window is not drawn, so there is nothing to read in it.
pub fn find(part: &str) -> Option<HWND> {
    let part = part.to_lowercase();
    list()
        .into_iter()
        .find(|(w, title)| {
            title.to_lowercase().contains(&part) && !unsafe { IsIconic(*w) }.as_bool()
        })
        .map(|(w, _)| w)
}

/// Keeps Hammurabi's captions out of pictures taken of a display, or lets them in again: they
/// are white text at the bottom of a display too, and would be read as a game's subtitles.
/// They stay on the display itself. Its other windows stay in the picture, so that a
/// screenshot shows them. (Not before Windows 10 2004, where this fails and the captions are in
/// the picture.)
///
/// No other window of this process is asked anything. NVIDIA's OpenGL driver keeps two hidden
/// windows here ("NVOGLDC invisible", "__wglDummyWindowFodder"), and Windows starts to draw a
/// hidden window that is told how it is to be captured: they stood on the display, blank, for
/// as long as the app ran.
pub fn keep_own_out(out: bool) {
    unsafe extern "system" fn each(w: HWND, out: LPARAM) -> BOOL {
        let mut process = 0u32;
        unsafe { GetWindowThreadProcessId(w, Some(&mut process)) };
        if process == std::process::id() {
            let mut title = [0u16; 32];
            let n = unsafe { GetWindowTextW(w, &mut title) };
            if String::from_utf16_lossy(&title[..n.max(0) as usize]) == super::CAPTIONS_WINDOW {
                let affinity = if out.0 != 0 {
                    WDA_EXCLUDEFROMCAPTURE
                } else {
                    WDA_NONE
                };
                let _ = unsafe { SetWindowDisplayAffinity(w, affinity) };
            }
        }
        true.into()
    }
    let _ = unsafe { EnumWindows(Some(each), LPARAM(isize::from(out))) };
}

/// Hammurabi's captions stay out of pictures of a display for as long as one of these lives.
pub struct OwnOut(());

static HOLDS: AtomicUsize = AtomicUsize::new(0);

impl OwnOut {
    pub fn hold() -> Self {
        HOLDS.fetch_add(1, Ordering::Relaxed);
        keep_own_out(true);
        Self(())
    }
}

impl Drop for OwnOut {
    fn drop(&mut self) {
        if HOLDS.fetch_sub(1, Ordering::Relaxed) == 1 {
            keep_own_out(false);
        }
    }
}

pub fn primary_display() -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowDisplayAffinity, WDA_MONITOR,
        WINDOW_DISPLAY_AFFINITY, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::{HSTRING, w};

    use super::*;

    /// A hidden window a pixel large.
    fn window(title: &str) -> HWND {
        let title = HSTRING::from(title);
        // SAFETY: a window of a class Windows registers itself.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("Static"),
                &title,
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        }
    }

    fn affinity(w: HWND) -> WINDOW_DISPLAY_AFFINITY {
        let mut affinity = 0u32;
        // SAFETY: the window is this process's.
        unsafe { GetWindowDisplayAffinity(w, &mut affinity) }.unwrap();
        WINDOW_DISPLAY_AFFINITY(affinity)
    }

    /// Only the captions are kept out of pictures and let in again: another window of this
    /// process, as the graphics driver has, is asked nothing and keeps what it had.
    #[test]
    fn only_the_captions_are_kept_out() {
        let (captions, other) = (window(crate::screen::CAPTIONS_WINDOW), window("another"));
        // Where Windows does not compose the desktop no window can be kept out.
        // SAFETY: the windows are this thread's, and live until destroyed below.
        if unsafe { SetWindowDisplayAffinity(other, WDA_MONITOR) }.is_ok() {
            keep_own_out(true);
            assert_eq!(affinity(captions), WDA_EXCLUDEFROMCAPTURE);
            assert_eq!(affinity(other), WDA_MONITOR);
            keep_own_out(false);
            assert_eq!(affinity(captions), WDA_NONE);
            assert_eq!(affinity(other), WDA_MONITOR);
        }
        for w in [captions, other] {
            // SAFETY: as above.
            let _ = unsafe { DestroyWindow(w) };
        }
    }
}
