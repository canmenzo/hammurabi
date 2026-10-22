//! Engine thread: audio → segmenter → scheduler → ASR → text cleaning → assembler → captions.

pub mod assembler;
pub mod rolling;
pub mod scheduler;
pub mod segmenter;
pub mod text;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, never, select};

use crate::asr::AsrWorker;
#[cfg(all(windows, feature = "win-livecaptions"))]
use crate::asr::livecaptions::{LcEvent, LiveCaptions};
use crate::audio::{AudioCapture, Frames16k};
use crate::models::{self, ModelId};
use crate::settings::{AsrBackend, ModelChoice, Settings, TranslatorChoice};
use crate::translate::TranslatorWorker;
use crate::tts::{self, Speaker};
use crate::types::{
    AsrEvent, AsrRequest, AsrResult, AudioChunk, CaptionUpdate, EngineCommand, EngineEvent,
    EngineState, Notifier, Progress, ProgressPhase, Recognizer, Status, StatusLevel, Translate,
    TranslateRequest, TranslatorEvent,
};
use assembler::{Assembler, Out};
use rolling::{RollingText, Step};
use scheduler::Scheduler;
use segmenter::Segmenter;

const FRAME: Duration = Duration::from_millis(100);
/// WASAPI loopback sends nothing while nothing plays; after this gap silence is injected.
const SILENCE_AFTER: Duration = Duration::from_millis(300);
const CONTEXT_PAIRS: usize = 3;
/// Live Captions does not report what it hears (its language is set in Windows); English is
/// its default, and the translators read the source text itself either way.
const LC_LANG: &str = "en";
/// Live Captions shows what it heard about a second late, so its text stays muted longer after speech.
const LC_TAIL: Duration = Duration::from_secs(2);

#[cfg(not(all(windows, feature = "win-livecaptions")))]
enum LcEvent {}

#[derive(Clone)]
struct Events {
    tx: Sender<EngineEvent>,
    notify: Notifier,
}

impl Events {
    fn send(&self, e: EngineEvent) {
        let _ = self.tx.send(e);
        (self.notify)();
    }

    fn status(&self, level: StatusLevel, text: String, ttl: Option<Duration>) {
        self.send(EngineEvent::Status(Status { level, text, ttl }));
    }
}

enum Pending {
    Final {
        id: u64,
        original: String,
        lang: String,
    },
    Live {
        version: u64,
    },
}

/// Everything between frames and captions, independent of threads and real workers.
/// Requests are queued in `asr_out`/`tr_out` for the caller to deliver.
struct Core {
    settings: Settings,
    seg: Segmenter,
    sched: Scheduler,
    asm: Assembler,
    /// Windows Live Captions text → sentences.
    rolling: RollingText,
    /// Language → recent share of sentences; steers auto-detect.
    prior: HashMap<String, f32>,
    tr_ready: bool,
    next_tr: u64,
    pending: HashMap<u64, Pending>,
    context: VecDeque<(String, String)>,
    /// Current live line; only its latest version is translated, one request at a time.
    live: (u64, String, Option<String>),
    live_in_flight: bool,
    asr_out: Vec<AsrRequest>,
    tr_out: Vec<TranslateRequest>,
    /// Final captions to read aloud: (text, language).
    speak_out: Vec<(String, String)>,
    events: Events,
}

impl Core {
    fn new(settings: Settings, events: Events) -> Self {
        Self {
            settings,
            seg: Segmenter::new(),
            sched: Scheduler::default(),
            asm: Assembler::default(),
            rolling: RollingText::default(),
            prior: HashMap::new(),
            tr_ready: false,
            next_tr: 0,
            pending: HashMap::new(),
            context: VecDeque::new(),
            live: (0, String::new(), None),
            live_in_flight: false,
            asr_out: Vec::new(),
            tr_out: Vec::new(),
            speak_out: Vec::new(),
            events,
        }
    }

    fn set_settings(&mut self, s: Settings) {
        if s.spoken_language != self.settings.spoken_language {
            self.prior.clear();
        }
        if s.target_language != self.settings.target_language {
            self.context.clear();
        }
        self.settings = s;
    }

    fn reset(&mut self) {
        self.seg.reset();
        self.sched.reset();
        self.asm.reset();
        self.rolling = RollingText::default();
        self.live = (self.live.0 + 1, String::new(), None);
    }

    fn translator_replaced(&mut self, ready: bool) {
        self.tr_ready = ready;
        self.pending.clear();
        self.live_in_flight = false;
    }

    fn deadline(&self) -> Option<Instant> {
        match (self.asm.flush_deadline(), self.rolling.deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn tick(&mut self, now: Instant) {
        let out = self.asm.tick(now);
        self.apply(out);
        let step = self.rolling.tick(now);
        self.apply_step(step, now);
    }

    #[cfg(any(test, all(windows, feature = "win-livecaptions")))]
    fn lc_text(&mut self, text: &str, now: Instant) {
        let step = self.rolling.update(text, now);
        self.apply_step(step, now);
    }

    /// Live Captions text heard while we were speaking: taken as already said.
    #[cfg(any(test, all(windows, feature = "win-livecaptions")))]
    fn lc_muted(&mut self, text: &str, now: Instant) {
        self.rolling.reseed(text, now);
    }

    fn apply_step(&mut self, step: Step, now: Instant) {
        if step.sentences.is_empty() && step.tail.is_none() {
            return;
        }
        let out = self.asm.external(step.sentences, step.tail, LC_LANG, now);
        self.apply(out);
    }

    fn frame(&mut self, f: Vec<f32>, now: Instant) {
        if let Some(closed) = self.seg.push(f) {
            let c = self.sched.close(closed, now);
            if c.lagging {
                self.events.status(
                    StatusLevel::Warning,
                    "Your computer can't keep up, choose a smaller model".into(),
                    Some(Duration::from_secs(5)),
                );
            }
            for id in c.abandoned {
                let out = self.asm.finish(id, None, now);
                self.apply(out);
            }
        }
        self.pump();
    }

    fn pump(&mut self) {
        if let Some((job, audio)) = self.sched.pump(&mut self.seg) {
            self.asr_out.push(AsrRequest {
                id: job.id,
                segment: job.segment,
                is_final: job.is_final,
                audio,
                language: self.settings.spoken_language.clone(),
                prior: self.prior.clone(),
            });
        }
    }

    fn asr_ready(&mut self) {
        self.sched.ready = true;
        self.pump();
    }

    fn asr_result(&mut self, r: AsrResult, now: Instant) {
        if let Some(job) = self.sched.complete(r.id) {
            let out = if r.error.is_some() {
                if job.is_final {
                    self.asm.finish(job.segment, None, now)
                } else {
                    Vec::new()
                }
            } else {
                let text = text::clean(&r.text, &r.language, r.no_speech, r.lang_conf);
                let words: Vec<String> = text.split_whitespace().map(String::from).collect();
                if job.is_final {
                    if words.len() >= 3 {
                        self.prior.values_mut().for_each(|v| *v *= 0.7);
                        *self.prior.entry(r.language.clone()).or_default() += 0.3;
                    }
                    self.asm.finish(job.segment, Some((words, r.language)), now)
                } else if !words.is_empty() && self.seg.open_id() == Some(job.segment) {
                    // A partial that finishes after its segment closed may hold the next one's audio.
                    self.asm.partial(job.segment, words, &r.language, now)
                } else {
                    Vec::new()
                }
            };
            self.apply(out);
        }
        self.pump();
    }

    /// Queues a translation; returns the text itself when no translation is needed.
    fn request(&mut self, text: &str, lang: &str, p: Pending) -> Option<String> {
        if !self.settings.translate || lang.is_empty() {
            return None;
        }
        if lang == self.settings.target_language {
            return Some(text.to_string());
        }
        if !self.tr_ready {
            return None;
        }
        self.next_tr += 1;
        self.tr_out.push(TranslateRequest {
            id: self.next_tr,
            text: text.to_string(),
            source: lang.to_string(),
            target: self.settings.target_language.clone(),
            context: self.context.iter().cloned().collect(),
        });
        self.pending.insert(self.next_tr, p);
        None
    }

    fn apply(&mut self, out: Vec<Out>) {
        for o in out {
            match o {
                Out::Sentence { id, text, lang } => {
                    let lang = lang.unwrap_or_default();
                    self.caption(CaptionUpdate::Final {
                        id,
                        original: text.clone(),
                        translated: None,
                        lang: lang.clone(),
                    });
                    let p = Pending::Final {
                        id,
                        original: text.clone(),
                        lang: lang.clone(),
                    };
                    let before = self.next_tr;
                    if let Some(t) = self.request(&text, &lang, p) {
                        self.speak(&t, &lang);
                        self.caption(CaptionUpdate::Final {
                            id,
                            original: text,
                            translated: Some(t),
                            lang,
                        });
                    } else if self.next_tr == before {
                        self.speak(&text, &lang);
                    }
                }
                Out::Live { text, lang } => {
                    self.live = (self.live.0 + 1, text.clone(), lang.clone());
                    self.caption(CaptionUpdate::Live {
                        original: text,
                        translated: None,
                        lang,
                    });
                    if !self.live_in_flight {
                        self.translate_live();
                    }
                }
            }
        }
    }

    /// Translated only in instant mode, since translating a half sentence reorders words on
    /// every update (worst for verb-final languages like Turkish).
    fn translate_live(&mut self) {
        let (version, text, lang) = self.live.clone();
        let Some(l) = lang
            .as_deref()
            .filter(|_| self.settings.instant && !text.is_empty())
        else {
            return;
        };
        let before = self.next_tr;
        if let Some(t) = self.request(&text, l, Pending::Live { version }) {
            self.caption(CaptionUpdate::Live {
                original: text,
                translated: Some(t),
                lang,
            });
        }
        self.live_in_flight = self.next_tr != before;
    }

    fn translation(&mut self, id: u64, text: Option<String>) {
        let Some(p) = self.pending.remove(&id) else {
            return;
        };
        match p {
            Pending::Final { id, original, lang } => {
                let Some(t) = text else {
                    return self.speak(&original, &lang);
                };
                let target = self.settings.target_language.clone();
                self.speak(&t, &target);
                self.context.push_back((original.clone(), t.clone()));
                if self.context.len() > CONTEXT_PAIRS {
                    self.context.pop_front();
                }
                self.caption(CaptionUpdate::Final {
                    id,
                    original,
                    translated: Some(t),
                    lang,
                });
            }
            Pending::Live { version } => {
                self.live_in_flight = false;
                if version != self.live.0 {
                    return self.translate_live();
                }
                if let Some(t) = text {
                    let (_, original, lang) = self.live.clone();
                    self.caption(CaptionUpdate::Live {
                        original,
                        translated: Some(t),
                        lang,
                    });
                }
            }
        }
    }

    /// Only final text is read; a translation is read in place of its original.
    fn speak(&mut self, text: &str, lang: &str) {
        if self.settings.speak && !text.trim().is_empty() {
            self.speak_out.push((text.to_string(), lang.to_string()));
        }
    }

    fn caption(&self, c: CaptionUpdate) {
        self.events.send(EngineEvent::Caption(c));
    }
}

/// Runs the pipeline over 100 ms frames on a virtual clock (frame i at i*100 ms), calling the
/// recognizer and translator inline as if they were instant. Returns every event produced,
/// including the final flush of an unfinished sentence.
pub fn simulate(
    frames: impl IntoIterator<Item = Vec<f32>>,
    rec: &mut dyn Recognizer,
    mut tr: Option<&mut dyn Translate>,
    settings: &Settings,
) -> Vec<EngineEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut core = Core::new(
        settings.clone(),
        Events {
            tx,
            notify: Arc::new(|| {}),
        },
    );
    core.tr_ready = tr.is_some();
    core.asr_ready();
    let mut now = Instant::now();
    for f in frames {
        core.tick(now);
        core.frame(f, now);
        drain(&mut core, rec, &mut tr, now);
        now += FRAME;
    }
    if let Some(t) = core.deadline() {
        core.tick(t.max(now));
        drain(&mut core, rec, &mut tr, now);
    }
    rx.try_iter().collect()
}

fn drain(
    core: &mut Core,
    rec: &mut dyn Recognizer,
    tr: &mut Option<&mut dyn Translate>,
    now: Instant,
) {
    loop {
        let asr = std::mem::take(&mut core.asr_out);
        let trs = std::mem::take(&mut core.tr_out);
        if asr.is_empty() && trs.is_empty() {
            return;
        }
        for req in asr {
            let started = Instant::now();
            let r = rec.transcribe(&req.audio, req.language.as_deref(), &req.prior);
            let ms = started.elapsed().as_millis() as u32;
            let r = match r {
                Ok(t) => AsrResult {
                    id: req.id,
                    text: t.text,
                    language: t.language,
                    no_speech: t.no_speech,
                    lang_conf: t.lang_conf,
                    ms,
                    error: None,
                },
                Err(e) => AsrResult {
                    id: req.id,
                    text: String::new(),
                    language: String::new(),
                    no_speech: 1.0,
                    lang_conf: 0.0,
                    ms,
                    error: Some(e.to_string()),
                },
            };
            core.asr_result(r, now);
        }
        for req in trs {
            let text = tr.as_mut().and_then(|t| {
                t.translate(&req.text, &req.source, &req.target, &req.context)
                    .ok()
            });
            core.translation(req.id, text);
        }
    }
}

/// Thread "hm-engine".
pub struct Engine {
    core: Core,
    running: bool,
    asr: Option<((ModelChoice, bool), AsrWorker)>,
    asr_device: String,
    tr: Option<(TranslatorChoice, TranslatorWorker)>,
    capture: Option<AudioCapture>,
    #[cfg(all(windows, feature = "win-livecaptions"))]
    lc: Option<LiveCaptions>,
    speaker: Option<Speaker>,
    /// Audio arrives through `audio_tx` from outside (CLI) instead of capture devices.
    fed: bool,
    audio_tx: Sender<AudioChunk>,
    audio_rx: Receiver<AudioChunk>,
    frames: Frames16k,
    buf: Vec<Vec<f32>>,
    last_audio: Instant,
    padded_until: Instant,
    /// Earliest time to reopen a capture whose device went away.
    reopen_at: Instant,
    /// A worker is downloading its model (Stop cancels that download).
    asr_downloading: bool,
    tr_downloading: bool,
}

impl Engine {
    /// `notifier` is called after every event is queued (= `ctx.request_repaint()`).
    pub fn spawn(notifier: Notifier) -> EngineHandle {
        Self::spawn_inner(notifier, false).0
    }

    /// An engine that never opens audio devices; while running it consumes the chunks sent
    /// through the returned sender instead.
    pub fn spawn_fed(notifier: Notifier) -> (EngineHandle, Sender<AudioChunk>) {
        Self::spawn_inner(notifier, true)
    }

    fn spawn_inner(notifier: Notifier, fed: bool) -> (EngineHandle, Sender<AudioChunk>) {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(256);
        let feed_tx = audio_tx.clone();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (done_tx, done) = crossbeam_channel::bounded::<()>(1);
        let events = Events {
            tx: event_tx,
            notify: notifier,
        };
        std::thread::Builder::new()
            .name("hm-engine".into())
            .spawn(move || {
                let now = Instant::now();
                let mut engine = Engine {
                    core: Core::new(Settings::default(), events),
                    running: false,
                    asr: None,
                    asr_device: String::new(),
                    tr: None,
                    capture: None,
                    #[cfg(all(windows, feature = "win-livecaptions"))]
                    lc: None,
                    speaker: None,
                    fed,
                    audio_tx,
                    audio_rx,
                    frames: Frames16k::new(),
                    buf: Vec::new(),
                    last_audio: now,
                    padded_until: now,
                    reopen_at: now,
                    asr_downloading: false,
                    tr_downloading: false,
                };
                engine.run(cmd_rx);
                engine.stop_capture();
                drop(done_tx);
            })
            .expect("spawn engine thread");
        (
            EngineHandle {
                cmd_tx,
                event_rx,
                done,
            },
            feed_tx,
        )
    }

    fn events(&self) -> &Events {
        &self.core.events
    }

    fn run(&mut self, cmd_rx: Receiver<EngineCommand>) {
        loop {
            let asr_rx = self
                .asr
                .as_ref()
                .map_or_else(never, |(_, w)| w.events.clone());
            let tr_rx = self
                .tr
                .as_ref()
                .map_or_else(never, |(_, w)| w.events.clone());
            let audio_rx = self.audio_rx.clone();
            let lc_rx = self.lc_rx();
            let tts_rx = self
                .speaker
                .as_ref()
                .map_or_else(never, |s| s.warnings.clone());
            let mut timeout = if self.running {
                FRAME
            } else {
                Duration::from_secs(3600)
            };
            if let Some(d) = self.core.deadline() {
                timeout = timeout.min(d.saturating_duration_since(Instant::now()));
            }
            select! {
                recv(cmd_rx) -> cmd => match cmd {
                    Ok(EngineCommand::Shutdown) | Err(_) => return,
                    Ok(c) => self.command(c),
                },
                recv(audio_rx) -> chunk => if let Ok(c) = chunk { self.audio(c) },
                recv(asr_rx) -> ev => match ev {
                    Ok(e) => self.on_asr(e),
                    Err(_) => self.on_asr(AsrEvent::Error("speech worker stopped".into())),
                },
                recv(lc_rx) -> ev => match ev {
                    Ok(e) => self.on_lc(e),
                    Err(_) => self.lc_failed("reader stopped".into()),
                },
                recv(tr_rx) -> ev => match ev {
                    Ok(e) => self.on_tr(e),
                    Err(_) => self.on_tr(TranslatorEvent::Error("translation worker stopped".into())),
                },
                recv(tts_rx) -> w => match w {
                    Ok(text) => self.events().status(StatusLevel::Warning, text, Some(Duration::from_secs(8))),
                    Err(_) => self.speaker = None,
                },
                default(timeout) => {}
            }
            let now = Instant::now();
            if self.running {
                self.check_capture(now);
                self.pad_silence(now);
            }
            self.core.tick(now);
            self.submit();
        }
    }

    fn submit(&mut self) {
        for req in std::mem::take(&mut self.core.asr_out) {
            if let Some((_, w)) = &self.asr {
                w.submit(req);
            }
        }
        for req in std::mem::take(&mut self.core.tr_out) {
            match &self.tr {
                Some((_, w)) => w.submit(req),
                None => self.core.translation(req.id, None),
            }
        }
        for (text, lang) in std::mem::take(&mut self.core.speak_out) {
            if let Some(s) = &self.speaker {
                s.say(text, lang);
            }
        }
    }

    fn feed(&mut self, now: Instant) {
        for f in std::mem::take(&mut self.buf) {
            self.core.frame(f, now);
        }
    }

    /// Captured audio holds our own speech (loopback, or the microphone hearing the speakers).
    /// Muting it misses whatever else plays meanwhile, but a voice that is never captioned
    /// cannot be read aloud again.
    fn muted(&self, now: Instant) -> bool {
        let tail = if self.lc_active() { LC_TAIL } else { tts::TAIL };
        self.speaker.as_ref().is_some_and(|s| s.muted(now, tail))
    }

    fn audio(&mut self, mut chunk: AudioChunk) {
        if !self.running {
            return;
        }
        let now = Instant::now();
        if self.muted(now) {
            chunk.samples.fill(0.0);
        }
        self.frames.push(chunk, &mut self.buf);
        self.last_audio = now;
        self.feed(now);
    }

    fn pad_silence(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_audio) < SILENCE_AFTER {
            return;
        }
        let from = self.padded_until.max(self.last_audio);
        let n = (now.saturating_duration_since(from).as_millis() / FRAME.as_millis()) as u32;
        for _ in 0..n {
            self.frames.push_silence(&mut self.buf);
        }
        self.padded_until = from + FRAME * n;
        self.feed(now);
    }

    fn command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Start(s) => {
                if self.running {
                    self.stop();
                }
                self.core.set_settings(s);
                self.core.reset();
                self.sync_speaker();
                if self.start_lc() {
                    self.ensure_workers();
                    self.running = true;
                    self.events().send(EngineEvent::State(EngineState::Loading));
                    return;
                }
                self.ensure_workers();
                self.core.reset();
                if self.start_capture() {
                    self.running = true;
                    let state = if self.core.sched.ready {
                        EngineState::Running
                    } else {
                        EngineState::Loading
                    };
                    self.events().send(EngineEvent::State(state));
                }
            }
            EngineCommand::Stop => {
                self.stop();
                self.cancel_downloads();
                self.events().send(EngineEvent::State(EngineState::Idle));
            }
            EngineCommand::Reconfigure(s) => {
                let prev = self.core.settings.clone();
                if self.running && s.asr_backend != prev.asr_backend {
                    return self.command(EngineCommand::Start(s));
                }
                self.core.set_settings(s);
                self.sync_speaker();
                if !self.running {
                    return;
                }
                self.ensure_workers();
                if self.lc_active() {
                    return;
                }
                let s = &self.core.settings;
                if (s.source, &s.input_device, &s.output_device)
                    != (prev.source, &prev.input_device, &prev.output_device)
                {
                    self.stop_capture();
                    if !self.start_capture() {
                        self.stop();
                    }
                }
            }
            EngineCommand::Prefetch(id) => self.prefetch(id),
            // With Read aloud off this speaker reads no captions (none are queued for it) and goes
            // at the next Start or Reconfigure.
            EngineCommand::Preview { text, lang, style } => self
                .speaker
                .get_or_insert_with(Speaker::spawn)
                .preview(text, lang, style),
            EngineCommand::Shutdown => {}
        }
    }

    fn start_capture(&mut self) -> bool {
        self.frames = Frames16k::new();
        self.last_audio = Instant::now();
        if self.fed {
            return true;
        }
        let s = &self.core.settings;
        let start = |source| {
            AudioCapture::start(
                source,
                s.input_device.as_deref(),
                s.output_device.as_deref(),
                self.audio_tx.clone(),
            )
        };
        match start(s.source) {
            Ok(c) => {
                for note in &c.notes {
                    log::warn!("{note}");
                    self.events().status(
                        StatusLevel::Warning,
                        note.clone(),
                        Some(Duration::from_secs(8)),
                    );
                }
                self.capture = Some(c);
                true
            }
            Err(e) => {
                self.events().status(
                    StatusLevel::Error,
                    format!("Audio capture failed: {e}"),
                    None,
                );
                self.events().send(EngineEvent::State(EngineState::Error));
                false
            }
        }
    }

    /// Reopens the capture when its device went away (unplugged headset, default device changed).
    fn check_capture(&mut self, now: Instant) {
        if now < self.reopen_at {
            return;
        }
        let Some(reason) = self.capture.as_ref().and_then(AudioCapture::failure) else {
            return;
        };
        self.reopen_at = now + Duration::from_secs(2);
        log::warn!("audio capture lost ({reason}), reopening");
        self.stop_capture();
        if self.start_capture() {
            self.events().status(
                StatusLevel::Info,
                "Audio device changed, captions continue".into(),
                Some(Duration::from_secs(3)),
            );
        } else {
            self.stop();
        }
    }

    /// Stopping during a model download cancels it; the next Start resumes it.
    fn cancel_downloads(&mut self) {
        let (asr, tr) = (self.asr_downloading, self.tr_downloading);
        if asr {
            self.asr = None;
            self.asr_downloading = false;
            self.core.sched.forget_in_flight();
            self.core.sched.ready = false;
        }
        if tr {
            self.tr = None;
            self.tr_downloading = false;
            self.core.translator_replaced(false);
        }
        if asr || tr {
            self.events().status(
                StatusLevel::Warning,
                "Download paused, it continues on the next Start".into(),
                Some(Duration::from_secs(4)),
            );
        }
    }

    fn stop_capture(&mut self) {
        if let Some(c) = self.capture.take() {
            c.stop();
        }
        while self.audio_rx.try_recv().is_ok() {}
    }

    fn stop(&mut self) {
        self.stop_capture();
        if let Some(s) = &self.speaker {
            s.clear();
        }
        #[cfg(all(windows, feature = "win-livecaptions"))]
        {
            self.lc = None;
        }
        self.running = false;
        self.core.reset();
        self.core.caption(CaptionUpdate::Clear);
    }

    fn sync_speaker(&mut self) {
        let s = &self.core.settings;
        if !s.speak {
            self.speaker = None;
            return;
        }
        self.speaker
            .get_or_insert_with(Speaker::spawn)
            .configure(s.speak_style());
    }

    /// Workers survive Stop/Start; they are rebuilt only when their model changes.
    fn ensure_workers(&mut self) {
        let key = (self.core.settings.model, self.core.settings.trim_audio);
        if !self.lc_active() && self.asr.as_ref().is_none_or(|(k, _)| *k != key) {
            self.asr = Some((key, AsrWorker::spawn(key.0, key.1)));
            self.asr_downloading = false;
            self.core.reset();
            self.core.sched.forget_in_flight();
            self.core.sched.ready = false;
            if self.running {
                self.events().send(EngineEvent::State(EngineState::Loading));
            }
        }
        let choice = &self.core.settings.translator;
        if self.core.settings.translate && self.tr.as_ref().is_none_or(|(c, _)| c != choice) {
            self.tr = Some((choice.clone(), TranslatorWorker::spawn(choice.clone())));
            self.tr_downloading = false;
            self.core.translator_replaced(false);
        }
    }

    fn on_asr(&mut self, ev: AsrEvent) {
        match ev {
            AsrEvent::Progress(p) => {
                self.asr_downloading = downloading(&p);
                self.events().send(EngineEvent::Progress(p));
            }
            AsrEvent::Ready { model, device } => {
                self.core.asr_ready();
                if self.running {
                    self.events().send(EngineEvent::State(EngineState::Running));
                    self.events().status(
                        StatusLevel::Info,
                        format!("Running: {model} on {device}"),
                        Some(Duration::from_secs(3)),
                    );
                }
                self.asr_device = device;
            }
            AsrEvent::Result(r) => {
                let ms = r.ms;
                self.core.asr_result(r, Instant::now());
                self.events().send(EngineEvent::Stats {
                    asr_ms: ms,
                    device: self.asr_device.clone(),
                });
            }
            AsrEvent::Error(e) => {
                self.asr = None;
                self.asr_downloading = false;
                self.core.sched.forget_in_flight();
                self.core.sched.ready = false;
                if self.running {
                    self.stop();
                }
                self.events().status(
                    StatusLevel::Error,
                    format!("Speech model failed: {e}"),
                    None,
                );
                self.events().send(EngineEvent::State(EngineState::Error));
            }
        }
    }

    fn wants_lc(&self) -> bool {
        self.core.settings.asr_backend == AsrBackend::WindowsLiveCaptions && !self.fed
    }

    #[cfg(all(windows, feature = "win-livecaptions"))]
    fn lc_active(&self) -> bool {
        self.lc.is_some()
    }

    #[cfg(not(all(windows, feature = "win-livecaptions")))]
    fn lc_active(&self) -> bool {
        false
    }

    /// Starts reading Windows Live Captions when selected; false = use Whisper.
    #[cfg(all(windows, feature = "win-livecaptions"))]
    fn start_lc(&mut self) -> bool {
        if !self.wants_lc() {
            return false;
        }
        self.lc = Some(LiveCaptions::spawn());
        true
    }

    #[cfg(not(all(windows, feature = "win-livecaptions")))]
    fn start_lc(&mut self) -> bool {
        if self.wants_lc() {
            self.events().status(
                StatusLevel::Warning,
                "Windows Live Captions is not supported on this system, using Whisper".into(),
                Some(Duration::from_secs(8)),
            );
        }
        false
    }

    #[cfg(all(windows, feature = "win-livecaptions"))]
    fn lc_rx(&self) -> Receiver<LcEvent> {
        self.lc.as_ref().map_or_else(never, |l| l.events.clone())
    }

    #[cfg(not(all(windows, feature = "win-livecaptions")))]
    fn lc_rx(&self) -> Receiver<LcEvent> {
        never()
    }

    #[cfg(all(windows, feature = "win-livecaptions"))]
    fn on_lc(&mut self, ev: LcEvent) {
        match ev {
            LcEvent::Ready => {
                if self.running {
                    self.events().send(EngineEvent::State(EngineState::Running));
                    self.events().status(
                        StatusLevel::Info,
                        "Running: Windows Live Captions".into(),
                        Some(Duration::from_secs(3)),
                    );
                }
            }
            LcEvent::Text(t) => {
                let now = Instant::now();
                if self.running && self.muted(now) {
                    self.core.lc_muted(&t, now);
                } else if self.running {
                    self.core.lc_text(&t, now);
                }
            }
            LcEvent::Error(e) => self.lc_failed(e),
        }
    }

    #[cfg(not(all(windows, feature = "win-livecaptions")))]
    fn on_lc(&mut self, ev: LcEvent) {
        match ev {}
    }

    /// Live Captions could not start or went away: carry on with Whisper.
    fn lc_failed(&mut self, e: String) {
        #[cfg(all(windows, feature = "win-livecaptions"))]
        {
            self.lc = None;
        }
        if !self.running {
            return;
        }
        self.events().status(
            StatusLevel::Warning,
            format!("Windows Live Captions unavailable ({e}), using Whisper"),
            Some(Duration::from_secs(10)),
        );
        self.core.reset();
        self.ensure_workers();
        if !self.start_capture() {
            self.running = false;
            self.core.reset();
        } else if self.core.sched.ready {
            self.events().send(EngineEvent::State(EngineState::Running));
        }
    }

    fn on_tr(&mut self, ev: TranslatorEvent) {
        match ev {
            TranslatorEvent::Progress(p) => {
                self.tr_downloading = downloading(&p);
                self.events().send(EngineEvent::Progress(p));
            }
            TranslatorEvent::Ready => {
                self.core.tr_ready = true;
                self.events().status(
                    StatusLevel::Info,
                    "Translation ready".into(),
                    Some(Duration::from_secs(3)),
                );
            }
            TranslatorEvent::Result { id, text } => self.core.translation(id, text),
            TranslatorEvent::Error(e) => {
                self.tr = None;
                self.tr_downloading = false;
                self.core.translator_replaced(false);
                self.events().status(
                    StatusLevel::Warning,
                    format!("Translation unavailable: {e}"),
                    Some(Duration::from_secs(10)),
                );
            }
        }
    }

    fn prefetch(&self, id: ModelId) {
        let ev = self.events().clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let mut progress = |done, total| {
                ev.send(EngineEvent::Progress(Progress {
                    model: id,
                    phase: ProgressPhase::Downloading,
                    done,
                    total,
                }))
            };
            match models::ensure(id, &mut progress, &cancel) {
                Ok(_) => ev.status(
                    StatusLevel::Info,
                    format!("{} downloaded", models::spec(id).label),
                    Some(Duration::from_secs(3)),
                ),
                Err(e) => ev.status(StatusLevel::Error, format!("Download failed: {e}"), None),
            }
        });
    }
}

fn downloading(p: &Progress) -> bool {
    matches!(p.phase, ProgressPhase::Downloading) && p.total.is_none_or(|t| p.done < t)
}

pub struct EngineHandle {
    cmd_tx: Sender<EngineCommand>,
    event_rx: Receiver<EngineEvent>,
    /// Disconnects when the engine thread exits.
    done: Receiver<()>,
}

impl EngineHandle {
    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<EngineEvent> {
        self.event_rx.recv_timeout(timeout).ok()
    }

    /// Sends Shutdown and joins the engine thread, giving up after `timeout`.
    pub fn shutdown(self, timeout: Duration) {
        self.send(EngineCommand::Shutdown);
        let _ = self.done.recv_timeout(timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_captions_text_reaches_captions_and_translation() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let settings = Settings {
            translate: true,
            target_language: "tr".into(),
            ..Settings::default()
        };
        let mut core = Core::new(
            settings,
            Events {
                tx,
                notify: Arc::new(|| {}),
            },
        );
        core.tr_ready = true;
        let t = Instant::now();
        core.lc_text("", t);
        core.lc_text("Hello there. How are", t);
        assert_eq!(core.tr_out.len(), 1);
        assert_eq!(core.tr_out[0].text, "Hello there.");
        assert_eq!(core.tr_out[0].source, "en");
        core.translation(core.tr_out[0].id, Some("Merhaba.".into()));
        core.tick(t + Duration::from_secs(2));
        let captions: Vec<CaptionUpdate> = rx
            .try_iter()
            .filter_map(|e| match e {
                EngineEvent::Caption(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(captions.iter().any(|c| matches!(c,
            CaptionUpdate::Final { translated: Some(t), original, .. }
                if t == "Merhaba." && original == "Hello there.")));
        assert!(captions.iter().any(|c| matches!(c,
            CaptionUpdate::Live { original, .. } if original == "How are")));
        // The paused tail became a sentence of its own.
        assert!(captions.iter().any(|c| matches!(c,
            CaptionUpdate::Final { original, translated: None, .. } if original == "How are")));
    }

    #[test]
    fn reads_final_captions_aloud() {
        let core = |speak, translate, ready| {
            let (tx, _rx) = crossbeam_channel::unbounded();
            let settings = Settings {
                speak,
                translate,
                instant: true,
                target_language: "tr".into(),
                ..Settings::default()
            };
            let mut c = Core::new(
                settings,
                Events {
                    tx,
                    notify: Arc::new(|| {}),
                },
            );
            c.tr_ready = ready;
            c
        };
        fn said(c: &Core) -> Vec<(&str, &str)> {
            c.speak_out
                .iter()
                .map(|(t, l)| (t.as_str(), l.as_str()))
                .collect()
        }
        let t = Instant::now();

        // The translation is read once it arrives, never the live line or the untranslated original.
        let mut c = core(true, true, true);
        c.lc_text("", t);
        c.lc_text("Hello there. How are", t);
        assert!(said(&c).is_empty());
        let ids: Vec<u64> = c.tr_out.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 2);
        c.translation(ids[1], Some("Nasıl".into()));
        c.translation(ids[0], Some("Merhaba.".into()));
        assert_eq!(said(&c), [("Merhaba.", "tr")]);

        // A failed translation reads the original.
        let mut c = core(true, true, true);
        c.lc_text("", t);
        c.lc_text("Hello there. How are", t);
        let id = c.tr_out[0].id;
        c.translation(id, None);
        assert_eq!(said(&c), [("Hello there.", "en")]);

        // No translation (off, or the translator not ready): the original right away.
        for (translate, ready) in [(false, true), (true, false)] {
            let mut c = core(true, translate, ready);
            c.lc_text("", t);
            c.lc_text("Hello there. How are", t);
            assert_eq!(said(&c), [("Hello there.", "en")]);
        }

        let mut c = core(false, false, true);
        c.lc_text("", t);
        c.lc_text("Hello there. How are", t);
        assert!(said(&c).is_empty());

        // What Live Captions shows while we speak is never captioned.
        let mut c = core(true, false, true);
        c.lc_text("", t);
        c.lc_muted("Merhaba dünya nasılsınız", t);
        c.tick(t + Duration::from_secs(2));
        c.lc_text("Merhaba dünya nasılsınız. Next", t + Duration::from_secs(2));
        assert!(said(&c).is_empty());
    }
}
