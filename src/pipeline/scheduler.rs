//! Port of Babil's `pump`: one ASR job in flight, finals first, bounded backlog.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::segmenter::{MIN_SPEECH, Segment, Segmenter};

/// Finals waiting beyond this are skipped, keeping their last partial text.
const MAX_BACKLOG: usize = 2;
const LAG_REPORT_EVERY: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub segment: u64,
    pub is_final: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Closing {
    /// Segments that will get no final; their last partial text (if any) should be finished.
    pub abandoned: Vec<u64>,
    /// Show the "can't keep up" status.
    pub lagging: bool,
}

#[derive(Default)]
pub struct Scheduler {
    finals: VecDeque<Segment>,
    in_flight: Option<(Job, bool)>, // bool = stale (from before a reset)
    next_job: u64,
    lag_reported: Option<Instant>,
    /// False until the recognizer reports ready.
    pub ready: bool,
}

impl Scheduler {
    pub fn close(&mut self, seg: Segment, now: Instant) -> Closing {
        let mut out = Closing::default();
        if seg.speech < MIN_SPEECH {
            out.abandoned.push(seg.id);
            return out;
        }
        self.finals.push_back(seg);
        while self.finals.len() > MAX_BACKLOG {
            out.abandoned.push(self.finals.pop_front().unwrap().id);
            if self.ready && self.lag_reported.is_none_or(|t| now - t > LAG_REPORT_EVERY) {
                self.lag_reported = Some(now);
                out.lagging = true;
            }
        }
        out
    }

    /// Next job to submit, if the recognizer is free.
    pub fn pump(&mut self, seg: &mut Segmenter) -> Option<(Job, Vec<f32>)> {
        if !self.ready || self.in_flight.is_some() {
            return None;
        }
        let (segment, is_final, audio) = match self.finals.pop_front() {
            Some(s) => (s.id, true, s.audio()),
            None => {
                let (id, audio) = seg.probe()?;
                (id, false, audio)
            }
        };
        self.next_job += 1;
        let job = Job {
            id: self.next_job,
            segment,
            is_final,
        };
        self.in_flight = Some((job, false));
        Some((job, audio))
    }

    /// Frees the recognizer; returns the job unless it is unknown or stale.
    pub fn complete(&mut self, id: u64) -> Option<Job> {
        match self.in_flight {
            Some((job, stale)) if job.id == id => {
                self.in_flight = None;
                (!stale).then_some(job)
            }
            _ => None,
        }
    }

    /// Drops queued finals. A job still running keeps the recognizer busy but its result is ignored.
    pub fn reset(&mut self) {
        self.finals.clear();
        if let Some((_, stale)) = &mut self.in_flight {
            *stale = true;
        }
    }

    /// The recognizer was replaced; nothing is in flight any more.
    pub fn forget_in_flight(&mut self) {
        self.in_flight = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FRAME_SAMPLES;

    /// Closes one segment with `loud` loud frames.
    fn segment(seg: &mut Segmenter, loud: usize) -> Segment {
        for _ in 0..loud {
            assert!(seg.push(vec![0.3; FRAME_SAMPLES]).is_none());
        }
        loop {
            if let Some(s) = seg.push(vec![0.0; FRAME_SAMPLES]) {
                return s;
            }
        }
    }

    #[test]
    fn finals_first_one_in_flight_and_backlog() {
        let now = Instant::now();
        let mut seg = Segmenter::new();
        let mut s = Scheduler::default();
        let a = segment(&mut seg, 10);
        assert_eq!(s.close(a, now), Closing::default());
        assert!(s.pump(&mut seg).is_none(), "not ready");
        s.ready = true;
        let (job, audio) = s.pump(&mut seg).unwrap();
        assert!(job.is_final && job.segment == 1 && !audio.is_empty());
        assert!(s.pump(&mut seg).is_none(), "one in flight");

        for _ in 0..2 {
            let x = segment(&mut seg, 10);
            assert_eq!(s.close(x, now), Closing::default());
        }
        let x = segment(&mut seg, 10);
        let c = s.close(x, now);
        assert_eq!(c.abandoned, vec![2]);
        assert!(c.lagging);
        // A second drop within 10 s is not reported again.
        let d = segment(&mut seg, 10);
        let c = s.close(d, now + Duration::from_secs(1));
        assert_eq!(c.abandoned, vec![3]);
        assert!(!c.lagging);

        assert_eq!(s.complete(99), None);
        assert_eq!(s.complete(job.id), Some(job));
        assert_eq!(s.pump(&mut seg).unwrap().0.segment, 4);
    }

    #[test]
    fn low_speech_abandoned_and_stale_jobs_ignored() {
        let now = Instant::now();
        let mut seg = Segmenter::new();
        let mut s = Scheduler {
            ready: true,
            ..Default::default()
        };
        let blip = segment(&mut seg, 2);
        assert_eq!(s.close(blip, now).abandoned, vec![1]);
        assert!(s.pump(&mut seg).is_none());

        let a = segment(&mut seg, 10);
        s.close(a, now);
        let (job, _) = s.pump(&mut seg).unwrap();
        s.reset();
        assert!(s.pump(&mut seg).is_none(), "stale job still running");
        assert_eq!(s.complete(job.id), None);
    }

    #[test]
    fn partial_probe() {
        let mut seg = Segmenter::new();
        let mut s = Scheduler {
            ready: true,
            ..Default::default()
        };
        for _ in 0..6 {
            seg.push(vec![0.3; FRAME_SAMPLES]);
        }
        let (job, audio) = s.pump(&mut seg).unwrap();
        assert!(!job.is_final);
        assert_eq!(audio.len(), 6 * FRAME_SAMPLES);
    }
}
