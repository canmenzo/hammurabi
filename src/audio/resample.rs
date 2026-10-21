use std::collections::VecDeque;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::types::{AudioChunk, FRAME_SAMPLES, SAMPLE_RATE, SourceKind};

/// With both sources, a source holding more than this while the other is empty mixes with silence.
const HOLD: usize = SAMPLE_RATE as usize * 3 / 10;

#[derive(Default)]
struct Source {
    rate: u32,
    resampler: Option<Fft<f32>>,
    pending: Vec<f32>,
    scratch: Vec<f32>,
    /// Leading resampler delay still to discard, in output samples.
    skip: usize,
    fifo: VecDeque<f32>,
    seen: bool,
}

impl Source {
    fn push(&mut self, samples: &[f32], rate: u32) {
        self.seen = true;
        if rate != self.rate {
            self.rate = rate;
            self.pending.clear();
            self.resampler = (rate != SAMPLE_RATE).then(|| {
                Fft::new(
                    rate as usize,
                    SAMPLE_RATE as usize,
                    1024,
                    1,
                    FixedSync::Both,
                )
                .expect("valid device sample rate")
            });
            if let Some(r) = &self.resampler {
                self.skip = r.output_delay();
                self.scratch = vec![0.0; r.output_frames_max()];
            }
        }
        let Some(r) = &mut self.resampler else {
            self.fifo.extend(samples);
            return;
        };
        self.pending.extend_from_slice(samples);
        let (n_in, n_out) = (r.input_frames_next(), r.output_frames_next());
        let mut used = 0;
        while self.pending.len() - used >= n_in {
            let input = InterleavedSlice::new(&self.pending[used..used + n_in], 1, n_in)
                .expect("sized input");
            let mut output = InterleavedSlice::new_mut(&mut self.scratch[..n_out], 1, n_out)
                .expect("sized output");
            let (_, written) = r
                .process_into_buffer(&input, &mut output, None)
                .expect("buffers sized by the resampler");
            used += n_in;
            let skip = self.skip.min(written);
            self.skip -= skip;
            self.fifo.extend(&self.scratch[skip..written]);
        }
        self.pending.drain(..used);
    }

    /// Up to one frame from the FIFO, zero-padded.
    fn take(&mut self) -> Vec<f32> {
        let n = self.fifo.len().min(FRAME_SAMPLES);
        let mut frame: Vec<f32> = self.fifo.drain(..n).collect();
        frame.resize(FRAME_SAMPLES, 0.0);
        frame
    }
}

/// Per-source resampling to 16 kHz mono plus mixing into `FRAME_SAMPLES` frames.
#[derive(Default)]
pub struct Frames16k {
    system: Source,
    mic: Source,
}

impl Frames16k {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends every completed 1600-sample frame to `out`.
    pub fn push(&mut self, chunk: AudioChunk, out: &mut Vec<Vec<f32>>) {
        match chunk.source {
            SourceKind::System => &mut self.system,
            SourceKind::Microphone => &mut self.mic,
        }
        .push(&chunk.samples, chunk.sample_rate);

        let (a, b) = (&mut self.system, &mut self.mic);
        loop {
            if !(a.seen && b.seen) {
                let s = if a.seen { &mut *a } else { &mut *b };
                if s.fifo.len() < FRAME_SAMPLES {
                    break;
                }
                out.push(s.take());
                continue;
            }
            let (la, lb) = (a.fifo.len(), b.fifo.len());
            let ready = (la >= FRAME_SAMPLES && lb >= FRAME_SAMPLES)
                || (la > HOLD && lb < FRAME_SAMPLES)
                || (lb > HOLD && la < FRAME_SAMPLES);
            if !ready {
                break;
            }
            out.push(
                a.take()
                    .into_iter()
                    .zip(b.take())
                    .map(|(x, y)| (x + y) * 0.5)
                    .collect(),
            );
        }
    }

    /// Appends one silent frame (used when the device delivers nothing).
    pub fn push_silence(&mut self, out: &mut Vec<Vec<f32>>) {
        out.push(vec![0.0; FRAME_SAMPLES]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(source: SourceKind, rate: u32, secs: f32, amp: f32) -> AudioChunk {
        let n = (rate as f32 * secs) as usize;
        let samples = (0..n)
            .map(|i| amp * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect();
        AudioChunk {
            source,
            samples,
            sample_rate: rate,
        }
    }

    fn rms(f: &[f32]) -> f32 {
        (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt()
    }

    #[test]
    fn resamples_48k_and_44k() {
        for rate in [48_000, 44_100, 16_000] {
            let mut f = Frames16k::new();
            let mut out = Vec::new();
            for _ in 0..20 {
                f.push(chunk(SourceKind::System, rate, 0.1, 0.5), &mut out);
            }
            // 2 s in; resampler latency may hold back one frame.
            assert!(out.len() >= 19 && out.len() <= 20, "{rate}: {}", out.len());
            assert!(out.iter().all(|fr| fr.len() == FRAME_SAMPLES));
            let r = rms(&out[5]);
            assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.02, "{rate}: rms {r}");
        }
    }

    #[test]
    fn mixes_both_and_fills_missing_source() {
        let mut f = Frames16k::new();
        let mut out = Vec::new();
        f.push(chunk(SourceKind::Microphone, 16_000, 0.2, 0.4), &mut out);
        assert_eq!(out.len(), 2);
        out.clear();
        f.push(chunk(SourceKind::System, 16_000, 0.2, 0.4), &mut out);
        // Both seen now: system alone has only 200 ms, below HOLD, so it waits.
        assert!(out.is_empty());
        f.push(chunk(SourceKind::System, 16_000, 0.2, 0.4), &mut out);
        assert!(!out.is_empty());
        let r = rms(&out[0]);
        assert!((r - 0.2 / 2f32.sqrt()).abs() < 0.01, "rms {r}");
        out.clear();
        f.push(chunk(SourceKind::Microphone, 16_000, 0.1, 0.4), &mut out);
        f.push(chunk(SourceKind::System, 16_000, 0.1, 0.4), &mut out);
        assert_eq!(out.len(), 1);
    }
}
