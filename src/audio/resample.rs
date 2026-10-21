#![allow(dead_code, unused_variables)]

use crate::types::AudioChunk;

/// Per-source resampling to 16 kHz mono plus mixing into `FRAME_SAMPLES` frames.
pub struct Frames16k {}

impl Frames16k {
    pub fn new() -> Self {
        Self {}
    }

    /// Appends every completed 1600-sample frame to `out`.
    pub fn push(&mut self, chunk: AudioChunk, out: &mut Vec<Vec<f32>>) {}

    /// Appends one silent frame (used when the device delivers nothing).
    pub fn push_silence(&mut self, out: &mut Vec<Vec<f32>>) {}
}

impl Default for Frames16k {
    fn default() -> Self {
        Self::new()
    }
}
