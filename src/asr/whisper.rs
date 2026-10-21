#![allow(dead_code, unused_variables)]

use std::collections::HashMap;

use candle_core::Device;

use crate::models::ModelPaths;
use crate::types::{Recognizer, Transcript};

pub struct WhisperEngine {}

impl WhisperEngine {
    pub fn load(paths: &ModelPaths, device: &Device) -> anyhow::Result<Self> {
        anyhow::bail!("unimplemented")
    }
}

impl Recognizer for WhisperEngine {
    fn transcribe(
        &mut self,
        audio: &[f32],
        language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript> {
        anyhow::bail!("unimplemented")
    }
}
