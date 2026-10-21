use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::models::ModelId;
use crate::settings::Settings;

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 1_600; // 100 ms
pub type Notifier = std::sync::Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioSource {
    System,
    Microphone,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    System,
    Microphone,
}

/// Mono, device sample rate.
pub struct AudioChunk {
    pub source: SourceKind,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub enum EngineCommand {
    Start(Settings),
    Stop,
    Reconfigure(Settings),
    Prefetch(ModelId),
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    Idle,
    Loading,
    Running,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub enum StatusLevel {
    Info,
    Warning,
    Error,
}

pub struct Status {
    pub level: StatusLevel,
    pub text: String,
    pub ttl: Option<Duration>,
}

#[derive(Clone, Copy, Debug)]
pub enum ProgressPhase {
    Downloading,
    Loading,
}

pub struct Progress {
    pub model: ModelId,
    pub phase: ProgressPhase,
    pub done: u64,
    pub total: Option<u64>,
}

pub enum CaptionUpdate {
    /// Sent twice: without, then with translation.
    Final {
        id: u64,
        original: String,
        translated: Option<String>,
        lang: String,
    },
    /// Empty original = clear live line.
    Live {
        original: String,
        translated: Option<String>,
        lang: Option<String>,
    },
    Clear,
}

pub enum EngineEvent {
    State(EngineState),
    Status(Status),
    Progress(Progress),
    Caption(CaptionUpdate),
    Stats { asr_ms: u32, device: String },
}

pub struct AsrRequest {
    pub id: u64,
    pub segment: u64,
    pub is_final: bool,
    pub audio: Vec<f32>,
    pub language: Option<String>,
    pub prior: HashMap<String, f32>,
}

pub struct AsrResult {
    pub id: u64,
    pub text: String,
    pub language: String,
    pub no_speech: f32,
    pub lang_conf: f32,
    pub ms: u32,
    pub error: Option<String>,
}

pub enum AsrEvent {
    Progress(Progress),
    Ready { model: String, device: String },
    Result(AsrResult),
    Error(String),
}

pub struct TranslateRequest {
    pub id: u64,
    pub text: String,
    pub source: String,
    pub target: String,
    pub context: Vec<(String, String)>,
}

pub enum TranslatorEvent {
    Progress(Progress),
    Ready,
    Result { id: u64, text: Option<String> },
    Error(String),
}

pub trait Recognizer: Send {
    fn transcribe(
        &mut self,
        audio: &[f32],
        language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript>;
}

pub struct Transcript {
    pub text: String,
    pub language: String,
    pub no_speech: f32,
    pub lang_conf: f32,
}

pub trait Translate: Send {
    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String>;
}
