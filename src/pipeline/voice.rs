//! Speaker's voice: picks the captured speech OmniVoice learns the speaker's voice from, with the
//! final transcript of that same audio as the reference text.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// 10 s of 100 ms frames.
const RING: usize = 100;
/// A voice sample needs 3 s of speech frames...
const MIN_SPEECH: usize = 30;
/// ...within at most 6 s.
const MAX_SPAN: u64 = 60;
/// The voice is learned again from newer speech this often, to follow the current speaker.
const REFRESH: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Sampler {
    /// Read aloud is on with Speaker's voice.
    pub enabled: bool,
    /// (frame index, 16 kHz frame, speech) for the last 10 s.
    ring: VecDeque<(u64, Vec<f32>, bool)>,
    /// Closed segments waiting for their final transcript: id -> frames [start, end).
    spans: HashMap<u64, (u64, u64)>,
    /// Segments with a final transcript, oldest first: (id, start, end, text).
    finals: VecDeque<(u64, u64, u64, String)>,
    learned: Option<Instant>,
}

impl Sampler {
    pub fn new(enabled: bool) -> Sampler {
        Sampler {
            enabled,
            ..Sampler::default()
        }
    }

    pub fn push(&mut self, index: u64, frame: Vec<f32>, speech: bool) {
        self.ring.push_back((index, frame, speech));
        if self.ring.len() > RING {
            self.ring.pop_front();
        }
        let first = self.ring[0].0;
        self.finals.retain(|f| f.1 >= first);
        self.spans.retain(|_, s| s.0 >= first);
    }

    pub fn closed(&mut self, id: u64, start: u64, len: usize) {
        if self.enabled {
            self.spans.insert(id, (start, start + len as u64));
        }
    }

    pub fn reset(&mut self) {
        self.spans.clear();
        self.finals.clear();
    }

    /// The next final transcript that qualifies is learned right away.
    pub fn relearn(&mut self) {
        self.learned = None;
    }

    /// Segment `id` got its final transcript `text`. When a voice sample is due, returns the
    /// newest stretch of whole, consecutive segments ending with this one that holds the most
    /// speech (at least 3 s within 6 s), as 16 kHz audio with its transcript.
    pub fn final_text(&mut self, id: u64, text: &str, now: Instant) -> Option<(Vec<f32>, String)> {
        let (start, end) = self.spans.remove(&id)?;
        if text.is_empty() {
            return None;
        }
        self.finals.push_back((id, start, end, text.into()));
        if self.learned.is_some_and(|t| now - t < REFRESH) {
            return None;
        }
        let f = &self.finals;
        let last = f.len() - 1;
        let mut best = None;
        for i in (0..=last).rev() {
            if (i < last && f[i + 1].0 != f[i].0 + 1) || end - f[i].1 > MAX_SPAN {
                break;
            }
            let Some(speech) = self.speech(f[i].1, end) else {
                break;
            };
            if best.is_none_or(|(s, _)| speech > s) {
                best = Some((speech, i));
            }
        }
        let (speech, i) = best.filter(|&(s, _)| s >= MIN_SPEECH)?;
        let first = f[i].1;
        let pcm: Vec<f32> = self
            .ring
            .iter()
            .filter(|r| (first..end).contains(&r.0))
            .flat_map(|r| r.1.iter().copied())
            .collect();
        let text: Vec<&str> = f.range(i..).map(|s| s.3.as_str()).collect();
        let text = text.join(" ");
        log::info!(
            "voice sample: {:.1} s, {:.1} s of speech: {text}",
            (end - first) as f32 / 10.0,
            speech as f32 / 10.0
        );
        self.learned = Some(now);
        Some((pcm, text))
    }

    /// Speech frames in [start, end); None when some of them are no longer (or were never) kept.
    fn speech(&self, start: u64, end: u64) -> Option<usize> {
        let frames: Vec<bool> = self
            .ring
            .iter()
            .filter(|r| (start..end).contains(&r.0))
            .map(|r| r.2)
            .collect();
        (frames.len() as u64 == end - start).then(|| frames.iter().filter(|&&s| s).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pushes `n` frames, speech or not, from frame index `at`.
    fn feed(s: &mut Sampler, at: u64, n: u64, speech: bool) {
        for i in at..at + n {
            s.push(i, vec![i as f32; 2], speech);
        }
    }

    #[test]
    fn picks_speech_with_its_transcript() {
        let t = Instant::now();
        let mut s = Sampler::new(true);
        // Segment 1: 2 s of speech, too little on its own.
        feed(&mut s, 0, 20, true);
        s.closed(1, 0, 20);
        assert_eq!(s.final_text(1, "One two.", t), None);
        // Segment 2 after a pause; 1 and 2 together hold 4 s of speech in 5 s.
        feed(&mut s, 20, 10, false);
        feed(&mut s, 30, 20, true);
        s.closed(2, 30, 20);
        let (pcm, text) = s.final_text(2, "Three four.", t).unwrap();
        assert_eq!(text, "One two. Three four.");
        assert_eq!(pcm.len(), 50 * 2);
        assert_eq!((pcm[0], pcm[99]), (0.0, 49.0));
        // Learned: nothing new until the refresh is due or relearn is asked for.
        feed(&mut s, 50, 40, true);
        s.closed(3, 50, 40);
        assert_eq!(s.final_text(3, "Five.", t + Duration::from_secs(59)), None);
        feed(&mut s, 90, 40, true);
        s.closed(4, 90, 40);
        let (pcm, text) = s.final_text(4, "Six.", t + REFRESH).unwrap();
        // Segments 3 and 4 span 8 s: only the newest one fits.
        assert_eq!((text.as_str(), pcm.len()), ("Six.", 40 * 2));
        s.relearn();
        feed(&mut s, 130, 35, true);
        s.closed(5, 130, 35);
        assert_eq!(
            s.final_text(5, "Seven.", t + REFRESH).unwrap().0.len(),
            35 * 2
        );
    }

    #[test]
    fn skips_gaps_and_lost_audio() {
        let t = Instant::now();
        let mut s = Sampler::new(true);
        feed(&mut s, 0, 20, true);
        s.closed(1, 0, 20);
        s.final_text(1, "One.", t);
        // Segment 2 got no final transcript, so 1 and 3 do not join.
        feed(&mut s, 20, 10, true);
        s.closed(2, 20, 10);
        feed(&mut s, 30, 20, true);
        s.closed(3, 30, 20);
        assert_eq!(s.final_text(3, "Three.", t), None);
        // Audio that left the ring.
        feed(&mut s, 50, 100, false);
        s.closed(4, 40, 60);
        assert_eq!(s.final_text(4, "Four.", t), None);
        // Nothing is kept while disabled.
        s.enabled = false;
        s.closed(5, 100, 40);
        assert_eq!(s.final_text(5, "Five.", t), None);
    }
}
