//! Windows Live Captions backend: LiveCaptions.exe (Windows 11 22H2+) transcribes system audio
//! itself; its caption text is read through UI Automation and polled on thread "hm-livecaptions".
//! The window is moved off-screen while in use and put back on stop.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use crossbeam_channel::{Receiver, Sender};
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetSystemMetrics, GetWindowRect, IsWindow, PostMessageW, SM_XVIRTUALSCREEN,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, WM_CLOSE,
};
use windows::core::w;

const POLL: Duration = Duration::from_millis(150);
const TEXT_ID: &str = "CaptionsTextBlock";

pub enum LcEvent {
    Ready,
    /// The whole caption window text, sent when it changes.
    Text(String),
    Error(String),
}

/// Reader thread handle; dropping it stops polling and restores the window.
pub struct LiveCaptions {
    pub events: Receiver<LcEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LiveCaptions {
    pub fn spawn() -> LiveCaptions {
        let (tx, events) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("hm-livecaptions".into())
            .spawn(move || run(&tx, &s))
            .expect("spawn hm-livecaptions");
        LiveCaptions {
            events,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for LiveCaptions {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wait so the window is back on screen before the app exits.
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(tx: &Sender<LcEvent>, stop: &AtomicBool) {
    if let Err(e) = poll(tx, stop) {
        log::warn!("live captions: {e:#}");
        let _ = tx.send(LcEvent::Error(format!("{e:#}")));
    }
}

fn find_window() -> Option<HWND> {
    unsafe { FindWindowW(w!("LiveCaptionsDesktopWindow"), None) }.ok()
}

struct Session {
    hwnd: HWND,
    rect: RECT,
    launched: bool,
}

impl Session {
    fn open(stop: &AtomicBool) -> anyhow::Result<Session> {
        let mut launched = false;
        let hwnd = match find_window() {
            Some(h) => h,
            None => {
                let mut buf = [0u16; 260];
                let n = unsafe { GetSystemWindowsDirectoryW(Some(&mut buf)) } as usize;
                if n == 0 || n >= buf.len() {
                    bail!("can't find the Windows directory");
                }
                let exe = std::path::PathBuf::from(OsString::from_wide(&buf[..n]))
                    .join(r"System32\LiveCaptions.exe");
                if !exe.exists() {
                    bail!("Windows Live Captions needs Windows 11 22H2 or newer");
                }
                std::process::Command::new(&exe)
                    .spawn()
                    .with_context(|| format!("starting {}", exe.display()))?;
                launched = true;
                let t0 = Instant::now();
                loop {
                    if let Some(h) = find_window() {
                        break h;
                    }
                    if stop.load(Ordering::Relaxed) || t0.elapsed() > Duration::from_secs(10) {
                        bail!("Live Captions window did not appear");
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        };
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }?;
        let s = Session {
            hwnd,
            rect,
            launched,
        };
        s.hide();
        Ok(s)
    }

    /// Off-screen rather than minimized: a minimized window may stop updating its text.
    fn hide(&self) {
        let w = self.rect.right - self.rect.left;
        let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } - w - 100;
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        if let Err(e) = unsafe { SetWindowPos(self.hwnd, None, x, self.rect.top, 0, 0, flags) } {
            log::warn!("live captions: moving the window off-screen failed: {e}");
        }
    }

    fn alive(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd)) }.as_bool()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.alive() {
            return;
        }
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        let _ =
            unsafe { SetWindowPos(self.hwnd, None, self.rect.left, self.rect.top, 0, 0, flags) };
        // Live Captions remembers its position, so it is closed only after being put back.
        if self.launched {
            let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
    }
}

/// Logs the element tree (debugging aid, since the text element's id is undocumented).
fn dump(auto: &UIAutomation, root: &UIElement) {
    let Ok(walker) = auto.get_raw_view_walker() else {
        return;
    };
    fn walk(walker: &uiautomation::UITreeWalker, e: &UIElement, depth: usize) {
        if depth > 12 {
            return;
        }
        let name: String = e.get_name().unwrap_or_default().chars().take(80).collect();
        log::info!(
            "live captions tree: {}{:?} id={:?} class={:?} name={:?}",
            "  ".repeat(depth),
            e.get_control_type().ok(),
            e.get_automation_id().unwrap_or_default(),
            e.get_classname().unwrap_or_default(),
            name
        );
        for c in walker.get_children(e).unwrap_or_default() {
            walk(walker, &c, depth + 1);
        }
    }
    walk(&walker, root, 0);
}

/// Searches the raw view: the caption text sits inside a XAML island.
fn find_id(auto: &UIAutomation, window: &UIElement, id: &str) -> anyhow::Result<UIElement> {
    let cond = auto.create_property_condition(UIProperty::AutomationId, Variant::from(id), None)?;
    if let Ok(e) = window.find_first(TreeScope::Descendants, &cond) {
        return Ok(e);
    }
    let walker = auto.get_raw_view_walker().map_err(|e| anyhow!("{e}"))?;
    let mut stack = vec![(window.clone(), 0)];
    while let Some((e, depth)) = stack.pop() {
        if e.get_automation_id().is_ok_and(|a| a == id) {
            return Ok(e);
        }
        if depth < 12 {
            stack.extend(
                walker
                    .get_children(&e)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (c, depth + 1)),
            );
        }
    }
    bail!("no element {id}")
}

fn find_text(auto: &UIAutomation, window: &UIElement) -> anyhow::Result<UIElement> {
    find_id(auto, window, TEXT_ID)
}

fn poll(tx: &Sender<LcEvent>, stop: &AtomicBool) -> anyhow::Result<()> {
    let session = Session::open(stop)?;
    let auto = UIAutomation::new().map_err(|e| anyhow!("UI Automation: {e}"))?;
    let window = auto
        .element_from_handle(session.hwnd.into())
        .map_err(|e| anyhow!("UI Automation: {e}"))?;
    // A freshly started Live Captions takes a moment to build its UI.
    let t0 = Instant::now();
    let mut text_el = loop {
        match find_text(&auto, &window) {
            Ok(e) => break e,
            // First run shows a privacy prompt that only the user should accept.
            Err(_) if find_id(&auto, &window, "AgreeButton").is_ok() => {
                bail!(
                    "Live Captions is not set up yet; open it once (Win+Ctrl+L) and choose \"Yes, continue\""
                );
            }
            Err(_) if stop.load(Ordering::Relaxed) => return Ok(()),
            Err(_) if t0.elapsed() < Duration::from_secs(8) => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => {
                dump(&auto, &window);
                bail!(
                    "Live Captions has no caption text yet; open Live Captions once and finish its setup (language download)"
                );
            }
        }
    };
    if log::log_enabled!(log::Level::Debug) {
        dump(&auto, &window);
    }
    let _ = tx.send(LcEvent::Ready);
    let mut last = None;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        if !session.alive() {
            bail!("Live Captions was closed");
        }
        let text = match text_el.get_name() {
            Ok(t) => t,
            // The element is rebuilt sometimes (e.g. after a settings change).
            Err(_) => {
                text_el = find_text(&auto, &window)?;
                continue;
            }
        };
        if last.as_ref() != Some(&text) {
            if tx.send(LcEvent::Text(text.clone())).is_err() {
                break;
            }
            last = Some(text);
        }
    }
    Ok(())
}
