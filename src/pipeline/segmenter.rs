//! Port of Babil's `onFrame`/`closeSegment`: RMS gate over an adaptive noise floor.

use std::collections::VecDeque;

use crate::types::FRAME_SAMPLES;

// All in 100 ms frames.
/// A segment needs at least 0.8 s before a pause can close it.
const MIN_FINAL: usize = 8;
/// 0.3 s of quiet closes the segment.
const PAUSE: usize = 3;
/// Hard cut at 5 s so continuous talk still finalizes.
const MAX_SEG: usize = 50;
/// On a hard cut, split at the quietest frame within the last 1.5 s.
const CUT_SEARCH: usize = 15;
/// Re-run the open segment every 0.6 s of new audio.
const PARTIAL_EVERY: usize = 6;
/// Segments with fewer loud frames are dropped (Whisper hallucinates on silence).
pub const MIN_SPEECH: usize = 3;
/// Quiet frames kept before speech onset so the first syllable is not clipped.
const PREROLL: usize = 3;

pub struct Segment {
    pub id: u64,
    /// Index of the first frame among all frames pushed.
    pub start: u64,
    pub frames: Vec<Vec<f32>>,
    rms: Vec<f32>,
    pub speech: usize,
    quiet: usize,
    since_probe: usize,
}

impl Segment {
    pub fn audio(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.frames.len() * FRAME_SAMPLES);
        for f in &self.frames {
            out.extend_from_slice(f);
        }
        out
    }
}

pub struct Segmenter {
    noise: f32,
    preroll: VecDeque<(Vec<f32>, f32)>,
    open: Option<Segment>,
    next_id: u64,
    /// Frames pushed so far, across resets.
    pushed: u64,
    /// Whether the last frame pushed was speech.
    pub last_loud: bool,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

fn rms_of(f: &[f32]) -> f32 {
    if f.is_empty() {
        return 0.0;
    }
    (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt()
}

impl Segmenter {
    pub fn new() -> Self {
        Self {
            noise: 0.005,
            preroll: VecDeque::new(),
            open: None,
            next_id: 1,
            pushed: 0,
            last_loud: false,
        }
    }

    /// Drops the open segment; ids keep counting.
    pub fn reset(&mut self) {
        self.noise = 0.005;
        self.preroll.clear();
        self.open = None;
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    pub fn open_id(&self) -> Option<u64> {
        self.open.as_ref().map(|s| s.id)
    }

    fn threshold(&self) -> f32 {
        0.004f32.max(self.noise * 2.5)
    }

    fn new_segment(
        &mut self,
        start: u64,
        frames: Vec<Vec<f32>>,
        rms: Vec<f32>,
        speech: usize,
    ) -> Segment {
        let id = self.next_id;
        self.next_id += 1;
        Segment {
            id,
            start,
            since_probe: frames.len(),
            frames,
            rms,
            speech,
            quiet: 0,
        }
    }

    /// Feeds one 100 ms frame; returns the segment it closed, if any.
    pub fn push(&mut self, frame: Vec<f32>) -> Option<Segment> {
        let r = rms_of(&frame);
        // Slow-rising, fast-falling floor so background music raises the bar but speech does not.
        self.noise = if r < self.noise {
            self.noise * 0.9 + r * 0.1
        } else {
            self.noise * 1.002
        };
        let loud = r > self.threshold();
        self.pushed += 1;
        self.last_loud = loud;

        if self.open.is_none() {
            if !loud {
                self.preroll.push_back((frame, r));
                if self.preroll.len() > PREROLL {
                    self.preroll.pop_front();
                }
                return None;
            }
            let start = self.pushed - 1 - self.preroll.len() as u64;
            let (frames, rms) = self.preroll.drain(..).unzip();
            self.open = Some(self.new_segment(start, frames, rms, 0));
        }
        let seg = self.open.as_mut().unwrap();
        seg.frames.push(frame);
        seg.rms.push(r);
        seg.since_probe += 1;
        if loud {
            seg.speech += 1;
            seg.quiet = 0;
        } else {
            seg.quiet += 1;
        }

        let n = seg.frames.len();
        if (seg.quiet >= PAUSE && n >= MIN_FINAL) || seg.quiet >= PAUSE * 4 {
            let end = n - seg.quiet + 1;
            Some(self.close(end))
        } else if n >= MAX_SEG {
            let mut cut = n - 1;
            for i in n - CUT_SEARCH..n {
                if seg.rms[i] < seg.rms[cut] {
                    cut = i;
                }
            }
            Some(self.close(cut + 1))
        } else {
            None
        }
    }

    /// Closes the open segment at frame `end`; loud frames after it start the next segment.
    fn close(&mut self, end: usize) -> Segment {
        let mut closed = self.open.take().unwrap();
        let rest = closed.frames.split_off(end);
        let rest_rms = closed.rms.split_off(end);
        let t = self.threshold();
        let speech = rest_rms.iter().filter(|&&r| r > t).count();
        if speech > 0 {
            let start = closed.start + end as u64;
            self.open = Some(self.new_segment(start, rest, rest_rms, speech));
        }
        closed
    }

    /// Audio of the open segment when a partial re-run is due.
    pub fn probe(&mut self) -> Option<(u64, Vec<f32>)> {
        let seg = self.open.as_mut()?;
        if seg.speech < MIN_SPEECH || seg.since_probe < PARTIAL_EVERY {
            return None;
        }
        seg.since_probe = 0;
        Some((seg.id, seg.audio()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: f32) -> Vec<f32> {
        (0..FRAME_SAMPLES)
            .map(|i| amp * (i as f32 * 0.0628).sin())
            .collect()
    }

    /// Feeds `(amplitude, frames)` runs; returns (frame index at close, closed length, speech).
    fn run(seg: &mut Segmenter, runs: &[(f32, usize)]) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        let mut i = 0;
        for &(amp, n) in runs {
            for _ in 0..n {
                if let Some(s) = seg.push(tone(amp)) {
                    out.push((i, s.frames.len(), s.speech));
                }
                i += 1;
            }
        }
        out
    }

    #[test]
    fn pause_closes_after_min_length() {
        let mut s = Segmenter::new();
        // 5 quiet (3 kept as preroll), 10 loud, then quiet: closes on the 3rd quiet frame.
        let c = run(&mut s, &[(0.0, 5), (0.3, 10), (0.0, 10)]);
        assert_eq!(c, vec![(17, 3 + 10 + 1, 10)]);
        assert_eq!(s.open_id(), None);
        // The preroll is part of the segment, so it starts 3 frames before the speech.
        let mut s = Segmenter::new();
        run(&mut s, &[(0.0, 5), (0.3, 1)]);
        assert_eq!(s.open.as_ref().unwrap().start, 2);
    }

    #[test]
    fn short_blip_waits_for_min_length() {
        let mut s = Segmenter::new();
        // 2 loud frames: the pause only closes once the segment reaches 8 frames (6 quiet).
        let c = run(&mut s, &[(0.3, 2), (0.0, 20)]);
        assert_eq!(c, vec![(7, 3, 2)]);
    }

    #[test]
    fn hard_cut_at_quietest_frame() {
        let mut s = Segmenter::new();
        // Continuous speech with one softer (still loud) frame at index 40 of the segment.
        let c = run(&mut s, &[(0.3, 40), (0.1, 1), (0.3, 30)]);
        // Closes at 50 frames, cut after the quiet frame: 41 frames; the rest (9 loud) stays open.
        assert_eq!(c[0], (49, 41, 50));
        assert_eq!(s.open_id(), Some(2));
        assert_eq!(s.open.as_ref().unwrap().start, 41);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn silence_never_opens() {
        let mut s = Segmenter::new();
        assert!(run(&mut s, &[(0.0, 100), (0.001, 100)]).is_empty());
        assert_eq!(s.open_id(), None);
    }

    #[test]
    fn probe_cadence() {
        let mut s = Segmenter::new();
        run(&mut s, &[(0.3, 2)]);
        assert!(s.probe().is_none(), "needs 3 speech frames");
        run(&mut s, &[(0.3, 4)]);
        let (id, audio) = s.probe().unwrap();
        assert_eq!((id, audio.len()), (1, 6 * FRAME_SAMPLES));
        run(&mut s, &[(0.3, 5)]);
        assert!(s.probe().is_none());
        run(&mut s, &[(0.3, 1)]);
        assert!(s.probe().is_some());
    }
}
