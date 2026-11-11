//! Cuts the audio into segments to recognize. Speech is found by a voice detector, and loud sound
//! it hears no speech in goes out as windows; without the detector, by loudness over an adaptive
//! noise floor (Babil's `onFrame`/`closeSegment`).

use std::collections::VecDeque;

use super::vad::Vad;
use crate::types::FRAME_SAMPLES;

// All in 100 ms frames.
/// A segment needs at least 0.8 s before a pause can close it.
const MIN_FINAL: usize = 8;
/// 0.3 s of quiet closes the segment.
const PAUSE: usize = 3;
/// Hard cut so continuous talk still finalizes: at 5 s, or at 12 s on a GPU, which is fast enough
/// to re-run that much for every partial and hears whole sentences better than halves.
const MAX_SEG: usize = 50;
const MAX_SEG_GPU: usize = 120;
/// Quiet frames kept after the last loud one: words end in sounds too soft to count as loud.
const TAIL: usize = 3;
/// On a hard cut, split at the quietest frame within the last 1.5 s.
const CUT_SEARCH: usize = 15;
/// Re-run the open segment every 0.6 s of new audio, or every 0.4 s on a GPU: a sentence is shown
/// once two runs agree on it, so this is most of how late captions are.
const PARTIAL_EVERY: usize = 6;
const PARTIAL_EVERY_GPU: usize = 4;
/// Segments with fewer loud frames are dropped (Whisper hallucinates on silence).
pub const MIN_SPEECH: usize = 3;
/// Quiet frames kept before speech onset so the first syllable is not clipped.
const PREROLL: usize = 3;
/// Speech probability that opens a segment, and the lower one that keeps it going.
const VAD_ON: f32 = 0.3;
const VAD_OFF: f32 = 0.2;
/// Quieter than any real sound: nothing is playing.
const SILENT: f32 = 0.0005;
/// Loud enough to hold speech the voice detector cannot make out (shouting over engines, music
/// and gunfire). Such sound is recognized anyway, in windows, and kept only if it reads as speech.
/// A quiet room's microphone stays below this.
const AUDIBLE: f32 = 0.02;
/// Length of those windows, and the shortest one worth recognizing.
const UNSURE_MAX: usize = 50;
const UNSURE_MIN: usize = 15;

pub struct Segment {
    pub id: u64,
    /// Index of the first frame among all frames pushed.
    pub start: u64,
    pub frames: Vec<Vec<f32>>,
    /// Per frame: how much it sounds like speech (loudness, or the voice detector's probability).
    level: Vec<f32>,
    rms: Vec<f32>,
    loud: Vec<bool>,
    pub speech: usize,
    /// No speech was detected in it, but it is loud enough to hold some.
    pub unsure: bool,
    quiet: usize,
    /// Frames at the end that are not even audible.
    silent: usize,
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
    /// None = loudness gate.
    vad: Option<Vad>,
    preroll: VecDeque<(Vec<f32>, f32, f32)>,
    open: Option<Segment>,
    next_id: u64,
    /// Frames pushed so far, across resets.
    pushed: u64,
    /// Whether the last frame pushed was speech.
    pub last_loud: bool,
    max_seg: usize,
    partial_every: usize,
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
            vad: None,
            preroll: VecDeque::new(),
            open: None,
            next_id: 1,
            pushed: 0,
            last_loud: false,
            max_seg: MAX_SEG,
            partial_every: PARTIAL_EVERY,
        }
    }

    /// Speech is told from other sound by a voice detector instead of by loudness.
    pub fn with_vad() -> Self {
        let vad = Vad::new()
            .inspect_err(|e| log::error!("voice detector unavailable, using loudness: {e:#}"))
            .ok();
        let (max_seg, partial_every) = if crate::device::label() == "CPU" {
            (MAX_SEG, PARTIAL_EVERY)
        } else {
            (MAX_SEG_GPU, PARTIAL_EVERY_GPU)
        };
        Self {
            vad,
            max_seg,
            partial_every,
            ..Self::new()
        }
    }

    /// Drops the open segment; ids keep counting.
    pub fn reset(&mut self) {
        if let Some(v) = &mut self.vad {
            v.reset();
        }
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
        level: Vec<f32>,
        rms: Vec<f32>,
        loud: Vec<bool>,
    ) -> Segment {
        let id = self.next_id;
        self.next_id += 1;
        Segment {
            id,
            start,
            since_probe: frames.len(),
            frames,
            level,
            rms,
            speech: loud.iter().filter(|&&l| l).count(),
            loud,
            unsure: false,
            quiet: 0,
            silent: 0,
        }
    }

    /// Whether `frame` (of loudness `r`) is speech, and how much it sounds like it.
    fn loud(&mut self, frame: &[f32], r: f32) -> (bool, f32) {
        let Some(vad) = &mut self.vad else {
            return (r > self.threshold(), r);
        };
        if r < SILENT {
            vad.reset();
            return (false, 0.0);
        }
        match vad.push(frame) {
            // Speech under way is held on to more loosely than it is picked up.
            Ok(p) if self.open.is_some() => (p >= VAD_OFF, p),
            Ok(p) => (p >= VAD_ON, p),
            Err(e) => {
                log::error!("voice detector failed, using loudness: {e:#}");
                self.vad = None;
                (r > self.threshold(), r)
            }
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
        let (loud, level) = self.loud(&frame, r);
        let windows = self.vad.is_some();
        let audible = loud || (windows && r >= AUDIBLE);
        self.pushed += 1;
        self.last_loud = loud;

        if self.open.is_none() {
            if !audible {
                self.preroll.push_back((frame, level, r));
                if self.preroll.len() > PREROLL {
                    self.preroll.pop_front();
                }
                return None;
            }
            let start = self.pushed - 1 - self.preroll.len() as u64;
            let (mut frames, mut levels, mut rms) = (Vec::new(), Vec::new(), Vec::new());
            for (f, l, r) in self.preroll.drain(..) {
                frames.push(f);
                levels.push(l);
                rms.push(r);
            }
            let quiet = vec![false; frames.len()];
            self.open = Some(self.new_segment(start, frames, levels, rms, quiet));
        }
        let seg = self.open.as_mut().unwrap();
        seg.frames.push(frame);
        seg.level.push(level);
        seg.rms.push(r);
        seg.loud.push(loud);
        seg.since_probe += 1;
        if loud {
            seg.speech += 1;
            seg.quiet = 0;
        } else {
            seg.quiet += 1;
        }
        seg.silent = if audible { 0 } else { seg.silent + 1 };

        let n = seg.frames.len();
        let lowest = |of: &[f32]| {
            (n - CUT_SEARCH.min(n)..n).fold(n - 1, |cut, i| if of[i] < of[cut] { i } else { cut })
        };
        if windows && seg.speech < MIN_SPEECH {
            // Only loud so far: it ends where the sound does, or as a window. Speech found in it
            // later makes it a segment like any other.
            if seg.silent >= PAUSE {
                let end = n - seg.silent + TAIL.min(seg.silent);
                Some(self.close(end, false))
            } else if n >= UNSURE_MAX {
                let cut = lowest(&seg.rms);
                Some(self.close(cut + 1, true))
            } else {
                None
            }
        } else if (seg.quiet >= PAUSE && n >= MIN_FINAL) || seg.quiet >= PAUSE * 4 {
            let end = n - seg.quiet + TAIL.min(seg.quiet);
            Some(self.close(end, false))
        } else if n >= self.max_seg {
            let cut = lowest(&seg.level);
            Some(self.close(cut + 1, false))
        } else {
            None
        }
    }

    /// Closes the open segment at frame `end`. The frames after it start the next segment when
    /// they hold speech, or when `keep` says so.
    fn close(&mut self, end: usize, keep: bool) -> Segment {
        let mut closed = self.open.take().unwrap();
        let rest = closed.frames.split_off(end);
        let level = closed.level.split_off(end);
        let rms = closed.rms.split_off(end);
        let loud = closed.loud.split_off(end);
        closed.unsure =
            self.vad.is_some() && closed.speech < MIN_SPEECH && closed.frames.len() >= UNSURE_MIN;
        if loud.contains(&true) || (keep && !rest.is_empty()) {
            let start = closed.start + end as u64;
            self.open = Some(self.new_segment(start, rest, level, rms, loud));
        }
        closed
    }

    /// Audio of the open segment when a partial re-run is due.
    pub fn probe(&mut self) -> Option<(u64, Vec<f32>)> {
        let seg = self.open.as_mut()?;
        if seg.speech < MIN_SPEECH || seg.since_probe < self.partial_every {
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
        // 5 quiet (3 kept as preroll), 10 loud, then quiet: closes on the 3rd quiet frame, and
        // keeps those three.
        let c = run(&mut s, &[(0.0, 5), (0.3, 10), (0.0, 10)]);
        assert_eq!(c, vec![(17, 3 + 10 + TAIL, 10)]);
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
        assert_eq!(c, vec![(7, 2 + TAIL, 2)]);
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
    fn loud_sound_the_detector_hears_no_speech_in_goes_out_as_windows() {
        let mut s = Segmenter::with_vad();
        assert!(s.vad.is_some());
        // White noise, far louder than `AUDIBLE`.
        let noise = |i: usize| -> Vec<f32> {
            (0..FRAME_SAMPLES)
                .map(|j| {
                    let x = ((i * FRAME_SAMPLES + j) as u32).wrapping_mul(2_654_435_761);
                    (x >> 8) as f32 / (1 << 24) as f32 * 0.4 - 0.2
                })
                .collect()
        };
        let mut closed = Vec::new();
        for i in 0..UNSURE_MAX + 20 {
            closed.extend(s.push(noise(i)));
        }
        for _ in 0..PAUSE {
            closed.extend(s.push(vec![0.0; FRAME_SAMPLES]));
        }
        // A window cut within its last 1.5 s, then what was left when the sound stopped.
        assert_eq!(closed.len(), 2);
        assert!(closed.iter().all(|c| c.unsure && c.speech == 0));
        assert!((UNSURE_MAX - CUT_SEARCH..=UNSURE_MAX).contains(&closed[0].frames.len()));
        assert_eq!(closed[1].start, closed[0].frames.len() as u64);
        assert_eq!(s.open_id(), None);
        assert!(s.probe().is_none());
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
