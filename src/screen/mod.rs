//! Reads a game's own subtitles off the screen (Windows): a window or a display is captured,
//! the band the subtitles are in is read by Windows' text recognition, and what settles there
//! becomes captions, in place of listening.

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod ocr;
mod prepare;
#[cfg(windows)]
pub mod preview;
pub mod reader;
mod resize;
#[cfg(windows)]
pub mod windows;

/// The title of the window Hammurabi's own captions are in: it is kept out of pictures taken of
/// a display, where they would be read as a game's subtitles.
pub const CAPTIONS_WINDOW: &str = "Hammurabi";

/// The part of a picture subtitles are read in, as fractions of its width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    pub const WHOLE: Area = Area {
        left: 0.0,
        top: 0.0,
        right: 1.0,
        bottom: 1.0,
    };
}

pub enum ScreenEvent {
    /// Reading has begun; what is read from.
    Ready(String),
    /// The window to read is not open, or was closed, or the display gives no picture: it is
    /// looked for until it is there.
    Waiting(String),
    /// The window to read is minimized: nothing can be read until it is brought back.
    Minimized(String),
    /// A subtitle, once.
    Line(String),
    Error(String),
}

#[cfg(windows)]
pub use source::ScreenText;

#[cfg(windows)]
mod source {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use crossbeam_channel::{Receiver, Sender};

    use super::capture::{Capture, Target};
    use super::ocr::Ocr;
    use super::reader::Reader;
    use super::{Area, ScreenEvent, windows};

    /// A picture is read this often: what the reader's counts of pictures are set for.
    const EVERY: Duration = Duration::from_millis(100);

    /// Reader thread "hm-screen"; dropping it stops the reading.
    pub struct ScreenText {
        pub events: Receiver<ScreenEvent>,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl ScreenText {
        /// `window`: part of a window's title, or empty for the main display. `band`: the
        /// part of the picture the subtitles are in. `lang`: what they are written in.
        pub fn spawn(window: String, band: Area, lang: String) -> ScreenText {
            let (tx, events) = crossbeam_channel::unbounded();
            let stop = Arc::new(AtomicBool::new(false));
            let s = stop.clone();
            let thread = std::thread::Builder::new()
                .name("hm-screen".into())
                .spawn(move || {
                    if let Err(e) = run(&window, band, &lang, &tx, &s) {
                        let _ = tx.send(ScreenEvent::Error(format!("{e:#}")));
                    }
                })
                .expect("spawn hm-screen");
            ScreenText {
                events,
                stop,
                thread: Some(thread),
            }
        }
    }

    impl Drop for ScreenText {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// How often a window that is not open, or a display that gives no picture, is looked for.
    const LOOK: Duration = Duration::from_secs(1);
    /// Pictures on end text recognition fails on before reading stops with the error.
    const FAILS_MAX: u32 = 50;

    fn run(
        window: &str,
        band: Area,
        lang: &str,
        tx: &Sender<ScreenEvent>,
        stop: &AtomicBool,
    ) -> anyhow::Result<()> {
        let ocr = Ocr::new(lang)?;
        let display = window.is_empty();
        let _own = display.then(windows::OwnOut::hold);
        // A window comes and goes (a game restarted, a projector closed by mistake), and a
        // display can stop giving pictures: what is read is read whenever it is there.
        let name = if display {
            "the main display".to_string()
        } else {
            format!("\"{window}\"")
        };
        let mut waiting = false;
        while !stop.load(Ordering::Relaxed) {
            let target = if display {
                Some(Target::Display(windows::primary_display()))
            } else {
                windows::find(window).map(Target::Window)
            };
            match target.map(|t| Capture::start(&t)) {
                Some(Ok(capture)) => {
                    let _ = tx.send(ScreenEvent::Ready(name.clone()));
                    waiting = false;
                    read(
                        capture,
                        (!display).then_some(name.as_str()),
                        band,
                        &ocr,
                        tx,
                        stop,
                    )?;
                    // What gave no picture, a window or the display, is looked for again after
                    // a while: at once, a window that is open but gives none would be captured
                    // anew many times a second.
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
                Some(Err(e)) => log::debug!("screen: {name} cannot be captured: {e:#}"),
                None => {}
            }
            if !waiting {
                let _ = tx.send(ScreenEvent::Waiting(name.clone()));
                waiting = true;
            }
            std::thread::sleep(LOOK);
        }
        Ok(())
    }

    /// Reads what is captured, a display or the `window` of that name, until told to stop or
    /// until no picture can be had: the window is gone, or capturing failed. Fails when text
    /// recognition fails on `FAILS_MAX` pictures on end.
    fn read(
        mut capture: Capture,
        window: Option<&str>,
        band: Area,
        ocr: &Ocr,
        tx: &Sender<ScreenEvent>,
        stop: &AtomicBool,
    ) -> anyhow::Result<()> {
        let t0 = Instant::now();
        // When a picture that could not be read was last logged, and how many on end were not.
        let mut failed: Option<Instant> = None;
        let mut fails = 0;
        // The newest picture: Windows gives one only when something was drawn.
        let mut image = None;
        let mut reader: Option<(usize, usize, Reader)> = None;
        // The two pictures read last and the lines in them: a picture that stands still is
        // read once.
        let mut read = [(Vec::new(), Vec::new()), (Vec::new(), Vec::new())];
        let mut pictures = 0u32;
        let display = window.is_none();
        let mut minimized = false;
        while !stop.load(Ordering::Relaxed) {
            let t = Instant::now();
            // Once a second, for the windows opened since.
            if display && pictures.is_multiple_of(10) {
                windows::keep_own_out(true);
            }
            pictures += 1;
            if let Some(name) = window
                && capture.minimized() != minimized
            {
                minimized = !minimized;
                let _ = tx.send(if minimized {
                    ScreenEvent::Minimized(name.to_string())
                } else {
                    ScreenEvent::Ready(name.to_string())
                });
            }
            match capture.part(band) {
                Ok(Some(new)) => image = Some(new),
                Ok(None) if capture.alive() => {}
                Ok(None) => return Ok(()),
                Err(e) => {
                    let what = window.unwrap_or("the display");
                    log::debug!("screen: {what} gave no picture: {e:#}");
                    return Ok(());
                }
            }
            if let Some(image) = &image {
                let size = (image.width, image.height);
                let reader = match &mut reader {
                    Some((w, h, reader)) if (*w, *h) == size => reader,
                    slot => &mut slot.insert((size.0, size.1, Reader::new(size.0, size.1))).2,
                };
                let (w, h) = reader.size();
                let (prepared, plain) = reader.prepare(&image.bgra);
                let fresh =
                    [prepared, plain]
                        .into_iter()
                        .zip(&mut read)
                        .try_for_each(|(picture, read)| {
                            if picture != read.0 {
                                *read = (picture.to_vec(), ocr.read(picture, w, h)?);
                            }
                            anyhow::Ok(())
                        });
                let at = (t - t0).as_secs_f64();
                // A picture text recognition fails on is skipped (it is read again with the
                // next), and the failure logged at most once a second.
                if let Err(e) = fresh {
                    fails += 1;
                    if fails >= FAILS_MAX {
                        return Err(e);
                    }
                    if failed.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
                        log::warn!("screen: a picture could not be read: {e:#}");
                        failed = Some(Instant::now());
                    }
                } else {
                    fails = 0;
                    if let Some((_, line)) = reader.read(at, &read[0].1, &read[1].1) {
                        log::debug!("screen: {line:?}");
                        if tx.send(ScreenEvent::Line(line)).is_err() {
                            break;
                        }
                    }
                }
            }
            std::thread::sleep(EVERY.saturating_sub(t.elapsed()));
        }
        Ok(())
    }
}
