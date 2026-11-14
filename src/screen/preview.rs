//! What is read, as a small picture a few times a second: the settings window shows it with
//! the subtitle area marked, so that the area can be set by looking.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TrySendError};

use super::capture::{Capture, Target};
use super::{Area, resize, windows};

const EVERY: Duration = Duration::from_millis(400);
/// The picture is made no wider than this.
const WIDTH: usize = 640;
/// It ends after this many rounds nobody looked at.
const UNSEEN: u32 = 4;

/// Rows of pixels, four bytes each: red, green, blue, alpha.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Thread "hm-preview". It ends by itself once `seen` is no longer called.
pub struct Preview {
    /// The newest picture, or why there is none. Closed when the thread has ended.
    pub pictures: Receiver<Result<Picture, String>>,
    seen: Arc<AtomicBool>,
}

impl Preview {
    /// `window` as for reading: part of a title, or empty for the main display. `wake` is
    /// called every round, to have the pictures looked at.
    pub fn spawn(window: String, wake: impl Fn() + Send + 'static) -> Preview {
        let (tx, pictures) = crossbeam_channel::bounded(1);
        let seen = Arc::new(AtomicBool::new(true));
        let s = seen.clone();
        std::thread::Builder::new()
            .name("hm-preview".into())
            .spawn(move || run(&window, &tx, &s, &wake))
            .expect("spawn hm-preview");
        Preview { pictures, seen }
    }

    pub fn seen(&self) {
        self.seen.store(true, Ordering::Relaxed);
    }
}

fn run(window: &str, tx: &Sender<Result<Picture, String>>, seen: &AtomicBool, wake: &dyn Fn()) {
    // The settings window itself would cover the picture of a display.
    let _own = window.is_empty().then(windows::OwnOut::hold);
    let mut capture = None;
    let mut unseen = 0;
    while unseen < UNSEEN {
        unseen = if seen.swap(false, Ordering::Relaxed) {
            0
        } else {
            unseen + 1
        };
        if let Some(p) = picture(window, &mut capture)
            && let Err(TrySendError::Disconnected(_)) = tx.try_send(p)
        {
            return;
        }
        wake();
        std::thread::sleep(EVERY);
    }
}

/// None when nothing was drawn since the last one.
fn picture(window: &str, capture: &mut Option<Capture>) -> Option<Result<Picture, String>> {
    if capture.is_none() {
        let target = if window.is_empty() {
            Target::Display(windows::primary_display())
        } else {
            match windows::find(window) {
                Some(w) => Target::Window(w),
                None => return Some(Err("That window is not open, or is minimized.".into())),
            }
        };
        match Capture::start(&target) {
            Ok(c) => *capture = Some(c),
            Err(e) => return Some(Err(format!("{e:#}"))),
        }
    }
    let c = capture.as_mut()?;
    match c.part(Area::WHOLE) {
        Ok(Some(image)) => {
            let k = image.width.div_ceil(WIDTH).max(1);
            let (width, height, rgba) = resize::shrink(&image.bgra, image.width, image.height, k);
            Some(Ok(Picture {
                width,
                height,
                rgba,
            }))
        }
        Ok(None) if c.alive() && !c.minimized() => None,
        // Closed, minimized or not to be had: it is looked for again.
        _ => {
            *capture = None;
            None
        }
    }
}
