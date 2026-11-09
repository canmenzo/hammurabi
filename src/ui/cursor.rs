use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{Context, Rect, pos2};

const POLL: Duration = Duration::from_millis(30);

/// The pointer in screen coordinates: physical pixels, or points on macOS.
pub struct Pointer {
    pub x: f32,
    pub y: f32,
    pub down: bool,
}

/// The overlay is click-through, so it hears nothing from the pointer. This watches for it
/// entering a set of screen rectangles and wakes the UI, which then lets clicks through to itself.
pub struct Watch {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    rects: Mutex<Vec<Rect>>,
    over: AtomicBool,
}

impl Watch {
    pub fn spawn(ctx: Context) -> Self {
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("hm-pointer".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(POLL);
                    let rects = s.rects.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    let over = !rects.is_empty()
                        && read().is_some_and(|p| rects.iter().any(|r| r.contains(pos2(p.x, p.y))));
                    if s.over.swap(over, Ordering::Relaxed) != over {
                        ctx.request_repaint();
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("pointer watch failed to start: {e}");
        }
        Self { shared }
    }

    pub fn set_rects(&self, rects: Vec<Rect>) {
        *self.shared.rects.lock().unwrap_or_else(|p| p.into_inner()) = rects;
    }

    pub fn over(&self) -> bool {
        self.shared.over.load(Ordering::Relaxed)
    }
}

#[cfg(windows)]
pub fn read() -> Option<Pointer> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_SWAPBUTTON};

    let mut p = POINT::default();
    // SAFETY: plain queries, and `p` outlives the call.
    unsafe {
        GetCursorPos(&mut p).ok()?;
        // This key state follows the physical buttons, not the primary-button setting.
        let primary = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        Some(Pointer {
            x: p.x as f32,
            y: p.y as f32,
            down: GetAsyncKeyState(primary.0 as i32) < 0,
        })
    }
}

#[cfg(target_os = "macos")]
pub fn read() -> Option<Pointer> {
    use objc2_app_kit::NSEvent;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
    }
    // SAFETY: these take and return plain values and may be called from any thread.
    let height = unsafe { CGDisplayPixelsHigh(CGMainDisplayID()) } as f64;
    let p = NSEvent::mouseLocation();
    Some(Pointer {
        x: p.x as f32,
        // AppKit counts up from the bottom of the main display.
        y: (height - p.y) as f32,
        down: NSEvent::pressedMouseButtons() & 1 != 0,
    })
}

#[cfg(target_os = "linux")]
pub fn read() -> Option<Pointer> {
    use std::cell::OnceCell;
    use x11_dl::xlib::{Button1Mask, Display, Xlib};

    thread_local! {
        static X: OnceCell<Option<(Xlib, *mut Display)>> = const { OnceCell::new() };
    }
    X.with(|x| {
        let opened = x.get_or_init(|| {
            let xlib = Xlib::open().ok()?;
            // SAFETY: a null name opens the default display; null means there is none.
            let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
            (!display.is_null()).then_some((xlib, display))
        });
        let (xlib, display) = opened.as_ref().map(|(xlib, d)| (xlib, *d))?;
        let (mut root, mut child) = (0, 0);
        let (mut x, mut y, mut wx, mut wy, mut mask) = (0, 0, 0, 0, 0);
        // SAFETY: the display stays open for the life of this thread, and every out pointer is live.
        let found = unsafe {
            (xlib.XQueryPointer)(
                display,
                (xlib.XDefaultRootWindow)(display),
                &mut root,
                &mut child,
                &mut x,
                &mut y,
                &mut wx,
                &mut wy,
                &mut mask,
            )
        };
        (found != 0).then_some(Pointer {
            x: x as f32,
            y: y as f32,
            down: mask & Button1Mask != 0,
        })
    })
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn read() -> Option<Pointer> {
    None
}
