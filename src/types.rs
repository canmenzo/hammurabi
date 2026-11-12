use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::models::ModelId;
use crate::settings::Settings;
use crate::tts::{Activity, Style};

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
    /// Read a sample aloud with `style` now, replacing a running preview; capture stays muted
    /// meanwhile.
    Preview {
        text: String,
        lang: String,
        style: Style,
    },
    /// Stop reading aloud, and drop what is waiting to be read.
    StopSpeech,
    /// Speaker's voice learns the voice again from the next speech that qualifies.
    #[cfg(omnivoice)]
    RelearnVoice,
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
    Speech(Activity),
    Stats { asr_ms: u32, device: String },
}

pub struct AsrRequest {
    pub id: u64,
    pub segment: u64,
    pub is_final: bool,
    pub audio: Vec<f32>,
    pub language: Option<String>,
    pub prior: HashMap<String, f32>,
    /// `Settings::hint`.
    pub hint: String,
}

pub struct AsrResult {
    pub id: u64,
    pub text: String,
    pub language: String,
    pub no_speech: f32,
    pub lang_conf: f32,
    pub avg_logprob: f32,
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
    /// `Settings::hint`: names to keep as they are.
    pub hint: String,
    /// When the caption was made; one that waited too long goes out untranslated.
    pub at: std::time::Instant,
}

pub enum TranslatorEvent {
    Progress(Progress),
    Ready,
    Result {
        id: u64,
        text: Option<String>,
    },
    /// Translations keep failing; they are tried again after a pause.
    Trouble(String),
    Error(String),
}

pub trait Recognizer: Send {
    /// Words to expect (`Settings::hint`), for recognizers that can use them.
    fn set_hint(&mut self, _hint: &str) {}

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
    /// Mean log probability of the tokens of `text`: how sure the recognizer is of it.
    pub avg_logprob: f32,
}

pub trait Translate: Send {
    /// Names and words to keep as they are (`Settings::hint`), for translators that can.
    fn set_hint(&mut self, _hint: &str) {}

    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String>;
}
