#![allow(dead_code, unused_variables)]

pub mod download;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::settings::ModelChoice;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelId {
    WhisperTiny,
    WhisperBase,
    WhisperSmall,
    WhisperTurbo,
    Madlad3bQ4k,
}

impl ModelId {
    pub const ALL: [ModelId; 5] = [
        Self::WhisperTiny,
        Self::WhisperBase,
        Self::WhisperSmall,
        Self::WhisperTurbo,
        Self::Madlad3bQ4k,
    ];

    /// `Auto` = small on CUDA/Metal, base on CPU.
    pub fn whisper(choice: ModelChoice, gpu: bool) -> ModelId {
        match choice {
            ModelChoice::Auto if gpu => Self::WhisperSmall,
            ModelChoice::Auto => Self::WhisperBase,
            ModelChoice::Tiny => Self::WhisperTiny,
            ModelChoice::Base => Self::WhisperBase,
            ModelChoice::Small => Self::WhisperSmall,
            ModelChoice::Turbo => Self::WhisperTurbo,
        }
    }
}

pub struct ModelSpec {
    pub repo: &'static str,
    pub revision: &'static str,
    /// Always `[weights, config, tokenizer]`.
    pub files: [&'static str; 3],
    pub approx_bytes: u64,
    pub label: &'static str,
}

#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub weights: PathBuf,
    pub config: PathBuf,
    pub tokenizer: PathBuf,
    /// Weights are GGUF (quantized) rather than safetensors.
    pub quantized: bool,
}

const MB: u64 = 1_000_000;

// T4 pins revisions to commit SHAs.
pub fn spec(id: ModelId) -> &'static ModelSpec {
    const SAFETENSORS: [&str; 3] = ["model.safetensors", "config.json", "tokenizer.json"];
    static TINY: ModelSpec = ModelSpec {
        repo: "lmz/candle-whisper",
        revision: "main",
        files: [
            "model-tiny-q80.gguf",
            "config-tiny.json",
            "tokenizer-tiny.json",
        ],
        approx_bytes: 40 * MB,
        label: "Tiny",
    };
    static BASE: ModelSpec = ModelSpec {
        repo: "openai/whisper-base",
        revision: "main",
        files: SAFETENSORS,
        approx_bytes: 290 * MB,
        label: "Base",
    };
    static SMALL: ModelSpec = ModelSpec {
        repo: "openai/whisper-small",
        revision: "main",
        files: SAFETENSORS,
        approx_bytes: 967 * MB,
        label: "Small",
    };
    static TURBO: ModelSpec = ModelSpec {
        repo: "openai/whisper-large-v3-turbo",
        revision: "main",
        files: SAFETENSORS,
        approx_bytes: 1_620 * MB,
        label: "Turbo",
    };
    static MADLAD: ModelSpec = ModelSpec {
        repo: "jbochi/madlad400-3b-mt",
        revision: "main",
        files: ["model-q4k.gguf", "config.json", "tokenizer.json"],
        approx_bytes: 1_670 * MB,
        label: "MADLAD-400 3B",
    };
    match id {
        ModelId::WhisperTiny => &TINY,
        ModelId::WhisperBase => &BASE,
        ModelId::WhisperSmall => &SMALL,
        ModelId::WhisperTurbo => &TURBO,
        ModelId::Madlad3bQ4k => &MADLAD,
    }
}

/// Cache layout: `models_dir()/<repo with '/' -> '--'>/<file>`.
pub fn is_cached(id: ModelId) -> bool {
    false
}

pub fn ensure(
    id: ModelId,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<ModelPaths> {
    anyhow::bail!("unimplemented")
}
