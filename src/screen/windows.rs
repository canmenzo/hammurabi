//! The windows and displays there are to read from.

use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible,
};
use windows::core::BOOL;

/// Every visible window with a title, front to back.
pub fn list() -> Vec<(HWND, String)> {
    unsafe extern "system" fn each(w: HWND, out: LPARAM) -> BOOL {
        let out = unsafe { &mut *(out.0 as *mut Vec<(HWND, String)>) };
        let n = unsafe { GetWindowTextLengthW(w) };
        if n > 0 && unsafe { IsWindowVisible(w) }.as_bool() {
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

pub fn primary_display() -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }
}
