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
pub const CAPTIONS_WINDOW: &str = "Hammurabi captions";

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

    /// `inner`, a part of this area given as fractions of it, as fractions of what this is part of.
    pub fn outer(&self, inner: Area) -> Area {
        let x = |f: f32| self.left + f * (self.right - self.left);
        let y = |f: f32| self.top + f * (self.bottom - self.top);
        Area {
            left: x(inner.left),
            top: y(inner.top),
            right: x(inner.right),
            bottom: y(inner.bottom),
        }
    }

    /// This part of `rect` (left, top, right, bottom), in whole pixels that hold all of it.
    pub fn of(&self, [l, t, r, b]: [i32; 4]) -> [i32; 4] {
        let (w, h) = ((r - l) as f32, (b - t) as f32);
        [
            l + (self.left * w).floor() as i32,
            t + (self.top * h).floor() as i32,
            l + (self.right * w).ceil() as i32,
            t + (self.bottom * h).ceil() as i32,
        ]
    }
}

/// The subtitle that stands in the picture read, as fractions of that picture: what its rows
/// cover, and how tall the tallest of them is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub area: Area,
    pub row: f32,
}

/// Where subtitles are read now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    /// The picture's place on the desktop in pixels: left, top, right, bottom. None while
    /// there is no picture.
    pub picture: Option<[i32; 4]>,
    pub subtitle: Option<Spot>,
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
    /// Where the picture is, or the subtitle in it, changed.
    Place(Place),
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
    use super::{Area, Place, ScreenEvent, Spot, windows};

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
        log::info!(
            "screen: {lang} subtitles are read with Windows' text recognition for {}",
            ocr.language()
        );
        let display = window.is_empty();
        let _own = display.then(windows::OwnOut::hold);
        // A window comes and goes (a game restarted, a projector closed by mistake), and a
        // display can stop giving pictures: what is read is read whenever it is there.
        let name = if display {
            crate::i18n::t(crate::i18n::T::TheMainDisplay).to_string()
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
                    let _ = tx.send(ScreenEvent::Place(Place::default()));
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
        let mut place = Place::default();
        // For the log, as nothing else tells why no subtitle comes: the most lines of text
        // found in one picture, the subtitles read, and whether their lack was logged.
        let (mut texts, mut subtitles, mut told) = (0, 0u32, false);
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
                    texts = texts.max(read[0].1.len() + read[1].1.len());
                    if let Some((_, line)) = reader.read(at, &read[0].1, &read[1].1) {
                        subtitles += 1;
                        if subtitles == 1 {
                            log::info!("screen: the first subtitle is read after {at:.0} s");
                        }
                        log::debug!("screen: {line:?}");
                        if tx.send(ScreenEvent::Line(line)).is_err() {
                            break;
                        }
                    }
                }
            }
            if !told && subtitles == 0 && t0.elapsed() >= Duration::from_secs(60) {
                told = true;
                let (w, h) = image.as_ref().map_or((0, 0), |i| (i.width, i.height));
                log::warn!(
                    "screen: no subtitle read in a minute: the area is {w}x{h}, and at most \
                     {texts} lines of text were found in it at once"
                );
            }
            let now = Place {
                picture: capture.place().filter(|_| !minimized),
                subtitle: reader.as_ref().and_then(|(.., r)| r.spot()).map(|s| Spot {
                    area: band.outer(s.area),
                    row: s.row * (band.bottom - band.top),
                }),
            };
            if now != place {
                place = now;
                let _ = tx.send(ScreenEvent::Place(place));
            }
            std::thread::sleep(EVERY.saturating_sub(t.elapsed()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_of_the_band_is_a_part_of_the_picture_and_of_the_desktop() {
        let band = Area {
            left: 0.125,
            top: 0.75,
            right: 0.875,
            bottom: 1.0,
        };
        let spot = band.outer(Area {
            left: 0.25,
            top: 0.5,
            right: 0.75,
            bottom: 1.0,
        });
        assert_eq!(
            spot,
            Area {
                left: 0.3125,
                top: 0.875,
                right: 0.6875,
                bottom: 1.0
            }
        );
        // A window on a display above and left of the main one.
        assert_eq!(spot.of([-1920, -1080, 0, 0]), [-1320, -135, -600, 0]);
        // Part pixels are covered whole.
        assert_eq!(spot.of([0, 0, 1001, 1001]), [312, 875, 689, 1001]);
    }
}
