//! Reads a game's own subtitles off the screen (Windows): a window or a display is captured,
//! the band the subtitles are in is read by Windows' text recognition, and what settles there
//! becomes captions, in place of listening.

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod ocr;
mod prepare;
pub mod reader;
mod resize;
#[cfg(windows)]
pub mod windows;

pub enum ScreenEvent {
    /// Reading has begun; what is read from.
    Ready(String),
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

    use anyhow::anyhow;
    use crossbeam_channel::{Receiver, Sender};

    use super::capture::{Capture, Target};
    use super::ocr::Ocr;
    use super::reader::Reader;
    use super::{ScreenEvent, windows};

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
        /// part of the picture's height the subtitles are in. `lang`: what they are written in.
        pub fn spawn(window: String, band: (f32, f32), lang: String) -> ScreenText {
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

    fn run(
        window: &str,
        band: (f32, f32),
        lang: &str,
        tx: &Sender<ScreenEvent>,
        stop: &AtomicBool,
    ) -> anyhow::Result<()> {
        let (target, name) = if window.is_empty() {
            (
                Target::Display(windows::primary_display()),
                "the main display".to_string(),
            )
        } else {
            let w = windows::find(window)
                .ok_or_else(|| anyhow!("no open window is titled like \"{window}\""))?;
            (Target::Window(w), format!("\"{window}\""))
        };
        let mut capture = Capture::start(&target)?;
        let ocr = Ocr::new(lang)?;
        let _ = tx.send(ScreenEvent::Ready(name));
        let t0 = Instant::now();
        // The newest picture: Windows gives one only when something was drawn.
        let mut image = None;
        let mut reader: Option<(usize, usize, Reader)> = None;
        // The prepared picture read last and the lines in it: a picture that stands still is
        // read once.
        let mut read = (Vec::new(), Vec::new());
        while !stop.load(Ordering::Relaxed) {
            let t = Instant::now();
            match capture.band(band.0, band.1)? {
                Some(new) => image = Some(new),
                None if capture.alive() => {}
                None => return Err(anyhow!("the window was closed")),
            }
            if let Some(image) = &image {
                let size = (image.width, image.height);
                let reader = match &mut reader {
                    Some((w, h, reader)) if (*w, *h) == size => reader,
                    slot => &mut slot.insert((size.0, size.1, Reader::new(size.0, size.1))).2,
                };
                let (w, h) = reader.size();
                let grey = reader.prepare(&image.bgra);
                if grey != read.0 {
                    read = (grey.to_vec(), ocr.read(grey, w, h)?);
                }
                if let Some((_, line)) = reader.read((t - t0).as_secs_f64(), &read.1) {
                    log::debug!("screen: {line:?}");
                    if tx.send(ScreenEvent::Line(line)).is_err() {
                        break;
                    }
                }
            }
            std::thread::sleep(EVERY.saturating_sub(t.elapsed()));
        }
        Ok(())
    }
}
