//! Engine thread: audio → segmenter → scheduler → ASR → text cleaning → assembler → captions.

pub mod assembler;
pub mod rolling;
pub mod scheduler;
pub mod segmenter;
pub mod text;
mod vad;
#[cfg(omnivoice)]
mod voice;

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
use crate::screen::ScreenEvent;
#[cfg(windows)]
use crate::screen::ScreenText;
use crate::settings::{AsrBackend, ModelChoice, Settings, TranslatorChoice};
use crate::translate::TranslatorWorker;
use crate::tts::{self, Speaker};
use crate::types::{
    AsrEvent, AsrRequest, AsrResult, AudioChunk, CaptionUpdate, EngineCommand, EngineEvent,
    EngineState, Notifier, Progress, ProgressPhase, Recognizer, SourceKind, Status, StatusLevel,
    Translate, TranslateRequest, TranslatorEvent,
};
use assembler::{Assembler, Out};
use rolling::{RollingText, Step};
use scheduler::Scheduler;
use segmenter::Segmenter;

const FRAME: Duration = Duration::from_millis(100);
/// WASAPI loopback sends nothing while nothing plays; after this gap silence is injected.
const SILENCE_AFTER: Duration = Duration::from_millis(300);
const CONTEXT_PAIRS: usize = 4;
/// A subtitle longer than this is translated sentence by sentence: two full rows take the
/// language model seconds, which the first sentence need not wait for.
const SPLIT_CHARS: usize = 60;
/// A sentence shorter than this ("W...") stays with what follows it.
const PIECE_CHARS: usize = 12;
/// Captions whose translations may still come.
const CAPTIONS: usize = 4;
/// Live Captions does not report what it hears (its language is set in Windows); English is
/// its default, and the translators read the source text itself either way.
const LC_LANG: &str = "en";
/// Live Captions shows what it heard about a second late, so its text stays muted longer after speech.
const LC_TAIL: Duration = Duration::from_secs(2);
/// This many failed transcriptions in a row reload the speech model.
const ASR_MAX_ERRORS: u32 = 5;
/// A transcription still running after this long never returns; the speech model is reloaded.
const ASR_STUCK: Duration = Duration::from_secs(30);
/// A speech model that failed to load is tried again this many times, this far apart.
const ASR_RETRIES: u32 = 3;
const ASR_RETRY_AFTER: Duration = Duration::from_secs(5);
/// The Large speech model taking this long a run on average, over this many runs, is not getting
/// the GPU it needs (captions run seconds late by then): Turbo takes over. Large alone averages
/// under 350 ms over any such stretch, also beside a three-way stream.
const ASR_SLOW_MS: u32 = 450;
const ASR_SLOW_RUNS: usize = 40;
/// How often a lost audio device is looked for.
const REOPEN_EVERY: Duration = Duration::from_secs(2);
/// The recognizer reads noise and music as words it is unsure of: text below this mean token log
/// probability is dropped. Sound that may not be speech at all (`Segment::unsure`) has to do better.
const LOGPROB_MIN: f32 = -1.2;
const UNSURE_LOGPROB_MIN: f32 = -0.8;
/// Text at or above this mean token log probability passes the recognizer's other scores.
const SURE_LOGPROB: f32 = -0.5;

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
    /// Part `part` of caption `id`.
    Final {
        id: u64,
        part: usize,
    },
    Live {
        version: u64,
    },
    /// Asked for before its sentence was a caption (`Core::ahead`).
    Ahead,
}

/// How far the translation of a part of a caption is.
enum Translation {
    Asked,
    Done(String),
    /// None is coming: the part reads as it was said.
    Plain,
}

/// A caption and what was added to it each time it grew. Each part is translated by itself, so
/// that what is on the screen is not worded anew when more is added, and only that is translated.
struct Shown {
    id: u64,
    lang: String,
    parts: Vec<(String, Translation)>,
}

/// A sentence still on the live line that is translated already, or being translated.
struct Ahead {
    text: String,
    translation: Option<String>,
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
    /// The last few captions, oldest first.
    captions: VecDeque<Shown>,
    /// Sentences the live line has whole are translated before they are captions, so that a
    /// caption comes with its translation instead of a fifth of a second before it. In the
    /// order they were said.
    ahead: Vec<Ahead>,
    /// The request for the one of them being translated: one at a time.
    ahead_out: Option<u64>,
    /// A request the translator need not finish, for the caller to tell it.
    tr_drop: Option<u64>,
    /// Current live line; only its latest version is translated, one request at a time.
    live: (u64, String, Option<String>),
    live_in_flight: bool,
    asr_out: Vec<AsrRequest>,
    tr_out: Vec<TranslateRequest>,
    /// Final captions to read aloud: (text, language).
    speak_out: Vec<(String, String)>,
    /// Speaker's voice: captured speech paired with its transcript.
    #[cfg(omnivoice)]
    voice: voice::Sampler,
    /// A voice sample (16 kHz, transcript) for the speaker to learn.
    #[cfg(omnivoice)]
    voice_out: Option<(Vec<f32>, String)>,
    events: Events,
}

impl Core {
    fn new(settings: Settings, events: Events, vad: bool) -> Self {
        #[cfg(omnivoice)]
        let enabled = settings.speak && settings.speak_engine == tts::Engine::Speaker;
        let separate = settings.speak;
        Self {
            settings,
            seg: if vad {
                Segmenter::with_vad()
            } else {
                Segmenter::new()
            },
            sched: Scheduler::default(),
            asm: {
                let mut asm = Assembler::default();
                asm.separate = separate;
                asm
            },
            rolling: RollingText::default(),
            prior: HashMap::new(),
            tr_ready: false,
            next_tr: 0,
            pending: HashMap::new(),
            context: VecDeque::new(),
            captions: VecDeque::new(),
            ahead: Vec::new(),
            ahead_out: None,
            tr_drop: None,
            live: (0, String::new(), None),
            live_in_flight: false,
            asr_out: Vec::new(),
            tr_out: Vec::new(),
            speak_out: Vec::new(),
            #[cfg(omnivoice)]
            voice: voice::Sampler::new(enabled),
            #[cfg(omnivoice)]
            voice_out: None,
            events,
        }
    }

    fn set_settings(&mut self, s: Settings) {
        #[cfg(omnivoice)]
        {
            self.voice.enabled = s.speak && s.speak_engine == tts::Engine::Speaker;
        }
        if s.spoken_language != self.settings.spoken_language {
            self.prior.clear();
        }
        if s.target_language != self.settings.target_language {
            self.context.clear();
        }
        self.asm.separate = s.speak;
        self.settings = s;
    }

    fn reset(&mut self) {
        #[cfg(omnivoice)]
        self.voice.reset();
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
        self.ahead.clear();
        self.ahead_out = None;
        // What was asked of the old one is not coming.
        let mut waiting = Vec::new();
        for c in &mut self.captions {
            for p in &mut c.parts {
                if matches!(p.1, Translation::Asked) {
                    p.1 = Translation::Plain;
                    waiting.push(c.id);
                }
            }
        }
        waiting.dedup();
        for id in waiting {
            self.show(id);
        }
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

    /// A subtitle read off the screen: a caption as it stands, a long one sentence by sentence.
    fn screen_line(&mut self, text: String, now: Instant) {
        let lang = self.settings.spoken_language.clone();
        let out = self
            .asm
            .external(pieces(&text), None, lang.as_deref().unwrap_or("en"), now);
        self.apply(out);
    }

    fn apply_step(&mut self, step: Step, now: Instant) {
        if step.sentences.is_empty() && step.tail.is_none() {
            return;
        }
        let out = self.asm.external(step.sentences, step.tail, LC_LANG, now);
        self.apply(out);
    }

    fn frame(&mut self, f: Vec<f32>, now: Instant) {
        #[cfg(omnivoice)]
        let copy = self.voice.enabled.then(|| f.clone());
        let closed = self.seg.push(f);
        #[cfg(omnivoice)]
        if let Some(f) = copy {
            self.voice
                .push(self.seg.pushed() - 1, f, self.seg.last_loud);
        }
        if let Some(closed) = closed {
            #[cfg(omnivoice)]
            self.voice
                .closed(closed.id, closed.start, closed.frames.len());
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
                hint: self.settings.hint.clone(),
                prefix: self.asm.shown(job.segment),
            });
        }
    }

    fn asr_ready(&mut self) {
        self.sched.ready = true;
        self.pump();
    }

    fn asr_result(&mut self, r: AsrResult, now: Instant) {
        if let Some(job) = self.sched.complete(r.id) {
            if r.error.is_none() {
                self.seg.pace(r.ms);
            }
            let out = if r.error.is_some() {
                if job.is_final {
                    self.asm.finish(job.segment, None, now)
                } else {
                    Vec::new()
                }
            } else {
                let floor = if job.unsure {
                    UNSURE_LOGPROB_MIN
                } else {
                    LOGPROB_MIN
                };
                // The recognizer's scores for "no speech" and for the language are taken at its
                // first step, before it has read anything, and are often wrong about speech over
                // music or noise (the full large-v3's about most speech). What it is this sure
                // of token by token is speech whatever those say.
                let (no_speech, lang_conf) = if r.avg_logprob >= SURE_LOGPROB {
                    (0.0, 1.0)
                } else {
                    (r.no_speech, r.lang_conf)
                };
                let text = if r.avg_logprob < floor {
                    String::new()
                } else {
                    text::clean(&r.text, &r.language, no_speech, lang_conf)
                };
                let words: Vec<String> = text.split_whitespace().map(String::from).collect();
                log::debug!(
                    "segment {} {}: {text:?}",
                    job.segment,
                    if job.is_final { "final" } else { "partial" }
                );
                if job.is_final {
                    #[cfg(omnivoice)]
                    if let Some(v) = self.voice.final_text(job.segment, &words.join(" "), now) {
                        self.voice_out = Some(v);
                    }
                    if words.len() >= 3 {
                        self.prior.values_mut().for_each(|v| *v *= 0.7);
                        *self.prior.entry(r.language.clone()).or_default() += 0.3;
                    }
                    self.asm.cut = job.cut;
                    self.asm.finish(job.segment, Some((words, r.language)), now)
                } else if !words.is_empty() && self.seg.open_id() == Some(job.segment) {
                    // A partial that finishes after its segment closed may hold the next one's audio.
                    self.asm.paused = job.paused;
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
            hint: self.settings.hint.clone(),
            at: Instant::now(),
        });
        self.pending.insert(self.next_tr, p);
        None
    }

    fn apply(&mut self, out: Vec<Out>) {
        for o in out {
            match o {
                Out::Sentence { id, text, lang } => {
                    log::debug!("caption #{id}: {text:?}");
                    let lang = lang.unwrap_or_default();
                    if self.captions.back().is_none_or(|c| c.id != id) {
                        self.captions.push_back(Shown {
                            id,
                            lang: lang.clone(),
                            parts: Vec::new(),
                        });
                        if self.captions.len() > CAPTIONS {
                            self.captions.pop_front();
                        }
                    }
                    // What this adds to the caption.
                    let c = self.captions.back_mut().unwrap();
                    let had: Vec<&str> = c.parts.iter().map(|p| p.0.as_str()).collect();
                    let new = match text.strip_prefix(had.join(" ").as_str()) {
                        Some(new) => new.trim().to_string(),
                        None => {
                            c.parts.clear();
                            text.clone()
                        }
                    };
                    let part = c.parts.len();
                    c.parts.push((new.clone(), Translation::Asked));
                    let p = Pending::Final { id, part };
                    // Translated ahead, or being translated: no second request.
                    let ahead = self.ahead.iter().position(|a| a.text == new);
                    log::debug!(
                        "ahead: {:?} {}",
                        new,
                        match ahead {
                            Some(i) if self.ahead[i].translation.is_some() => "in",
                            Some(_) => "out",
                            None => "not asked",
                        }
                    );
                    let before = self.next_tr;
                    match ahead.map(|i| self.ahead.remove(i)) {
                        Some(Ahead {
                            translation: Some(t),
                            ..
                        }) => self.translated(id, part, Some(t)),
                        Some(_) if self.ahead_out.is_some() => {
                            let request = self.ahead_out.take().unwrap();
                            self.pending.insert(request, p);
                            self.show(id);
                        }
                        _ => {
                            // A caption does not wait for a sentence that is none yet.
                            if let Some(request) = self.ahead_out.take() {
                                self.tr_drop = Some(request);
                                self.ahead.retain(|a| a.translation.is_some());
                            }
                            match self.request(&new, &lang, p) {
                                Some(same) => {
                                    // Already in the target language: it is heard as it is.
                                    if self.settings.speak_same_lang {
                                        self.speak(&same, &lang);
                                    }
                                    self.set_part(id, part, Translation::Done(same));
                                }
                                None if self.next_tr == before => {
                                    self.speak(&new, &lang);
                                    self.set_part(id, part, Translation::Plain);
                                }
                                None => {}
                            }
                            self.show(id);
                        }
                    }
                }
                Out::Live { text, lang, whole } => {
                    self.translate_ahead(whole, lang.as_deref());
                    self.live = (self.live.0 + 1, text.clone(), lang.clone());
                    // Unfinished speech in another language is the original, too.
                    let hidden = self.translation_only()
                        && !self.settings.instant
                        && lang.as_deref() != Some(self.settings.target_language.as_str());
                    self.caption(CaptionUpdate::Live {
                        original: if hidden { String::new() } else { text },
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

    /// Captions show translations alone, not what was said.
    fn translation_only(&self) -> bool {
        self.settings.translate && !self.settings.caption.show_original
    }

    /// Asks for the translation of the first of `whole`, the sentences the live line has whole,
    /// that has none yet. One at a time, and not while a caption waits for its own: the
    /// translator takes them in turn.
    fn translate_ahead(&mut self, whole: Vec<String>, lang: Option<&str>) {
        // What the live line no longer says is of no use, the one being translated too.
        self.ahead.retain(|a| whole.contains(&a.text));
        if self.ahead_out.is_some() && self.ahead.iter().all(|a| a.translation.is_some()) {
            self.tr_drop = self.ahead_out.take();
        }
        let Some(lang) = lang else { return };
        let busy = self.ahead_out.is_some()
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Final { .. }));
        // In instant mode the live line is translated as it is.
        if busy || self.settings.instant {
            return;
        }
        let Some(text) = whole
            .into_iter()
            .find(|s| !self.ahead.iter().any(|a| a.text == *s))
        else {
            return;
        };
        let before = self.next_tr;
        self.request(&text, lang, Pending::Ahead);
        if self.next_tr != before {
            self.ahead_out = Some(self.next_tr);
            self.ahead.push(Ahead {
                text,
                translation: None,
            });
        }
    }

    fn set_part(&mut self, id: u64, part: usize, t: Translation) {
        if let Some(p) = self
            .captions
            .iter_mut()
            .find(|c| c.id == id)
            .and_then(|c| c.parts.get_mut(part))
        {
            p.1 = t;
        }
    }

    /// The translation of a part of a caption is in, or (None) is not coming: read, kept as
    /// context for the next, and shown.
    fn translated(&mut self, id: u64, part: usize, t: Option<String>) {
        let Some(c) = self.captions.iter().find(|c| c.id == id) else {
            return;
        };
        let Some((original, lang)) = c.parts.get(part).map(|p| (p.0.clone(), c.lang.clone()))
        else {
            return;
        };
        match t {
            Some(t) => {
                let target = self.settings.target_language.clone();
                self.speak(&t, &target);
                self.context.push_back((original, t.clone()));
                if self.context.len() > CONTEXT_PAIRS {
                    self.context.pop_front();
                }
                self.set_part(id, part, Translation::Done(t));
            }
            None => {
                self.speak(&original, &lang);
                self.set_part(id, part, Translation::Plain);
            }
        }
        self.show(id);
    }

    /// Sends caption `id` as it stands: all of what was said, and the translation as far as it
    /// has come.
    fn show(&self, id: u64) {
        let Some(c) = self.captions.iter().find(|c| c.id == id) else {
            return;
        };
        // With the original hidden, a caption waits for the first of its translation.
        if self.translation_only() && matches!(c.parts.first(), Some((_, Translation::Asked))) {
            return;
        }
        let original: Vec<&str> = c.parts.iter().map(|p| p.0.as_str()).collect();
        let translated: Vec<&str> = c
            .parts
            .iter()
            .map_while(|p| match &p.1 {
                Translation::Done(t) => Some(t.as_str()),
                Translation::Plain => Some(p.0.as_str()),
                Translation::Asked => None,
            })
            .collect();
        let any = c.parts.iter().any(|p| matches!(p.1, Translation::Done(_)));
        self.caption(CaptionUpdate::Final {
            id,
            original: original.join(" "),
            translated: (any && !translated.is_empty()).then(|| translated.join(" ")),
            lang: c.lang.clone(),
        });
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
        let request = id;
        match p {
            // Not translated after all: better the original than nothing.
            Pending::Final { id, part } => self.translated(id, part, text),
            Pending::Ahead => {
                // The one asked for last is the one out. A sentence that became a caption
                // meanwhile took the request over as its own.
                if self.ahead_out.take_if(|r| *r == request).is_some() {
                    match (
                        self.ahead.iter_mut().rfind(|a| a.translation.is_none()),
                        text,
                    ) {
                        (Some(a), Some(t)) => a.translation = Some(t),
                        _ => self.ahead.retain(|a| a.translation.is_some()),
                    }
                }
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
/// including the final flush of an unfinished sentence. `vad`: find speech with the voice detector
/// (real audio) instead of by loudness (synthetic frames).
pub fn simulate(
    frames: impl IntoIterator<Item = Vec<f32>>,
    rec: &mut dyn Recognizer,
    mut tr: Option<&mut dyn Translate>,
    settings: &Settings,
    vad: bool,
) -> Vec<EngineEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut core = Core::new(
        settings.clone(),
        Events {
            tx,
            notify: Arc::new(|| {}),
        },
        vad,
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
            rec.set_hint(&req.hint);
            rec.set_prefix(&req.prefix);
            let r = rec.transcribe(&req.audio, req.language.as_deref(), &req.prior);
            let ms = started.elapsed().as_millis() as u32;
            let r = match r {
                Ok(t) => AsrResult {
                    id: req.id,
                    text: t.text,
                    language: t.language,
                    no_speech: t.no_speech,
                    lang_conf: t.lang_conf,
                    avg_logprob: t.avg_logprob,
                    ms,
                    error: None,
                },
                Err(e) => AsrResult {
                    id: req.id,
                    text: String::new(),
                    language: String::new(),
                    no_speech: 1.0,
                    lang_conf: 0.0,
                    avg_logprob: 0.0,
                    ms,
                    error: Some(e.to_string()),
                },
            };
            core.asr_result(r, now);
        }
        for req in trs {
            let text = tr.as_mut().and_then(|t| {
                t.set_hint(&req.hint);
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
    /// Captured system audio does not hold our own speech.
    system_clear: bool,
    #[cfg(all(windows, feature = "win-livecaptions"))]
    lc: Option<LiveCaptions>,
    #[cfg(windows)]
    screen: Option<ScreenText>,
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
    /// The audio device is gone and is being waited for.
    capture_lost: bool,
    asr_errors: u32,
    /// How long the Large model's last runs took.
    asr_took: VecDeque<u32>,
    /// Large was too slow here and Turbo stands in for it until another model is chosen.
    asr_stand_in: bool,
    /// When the speech worker took the job it is on.
    asr_busy: Option<Instant>,
    asr_retries: u32,
    asr_retry_at: Option<Instant>,
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
        let exit = ExitSignal {
            done: Some(done_tx),
            notify: notifier.clone(),
        };
        let events = Events {
            tx: event_tx,
            notify: notifier,
        };
        std::thread::Builder::new()
            .name("hm-engine".into())
            .spawn(move || {
                let exit = exit;
                let now = Instant::now();
                // Windows keeps an app's lowered volume if we died with it turned down.
                std::thread::spawn(crate::audio::duck::recover);
                let mut engine = Engine {
                    core: Core::new(Settings::default(), events, true),
                    running: false,
                    asr: None,
                    asr_device: String::new(),
                    tr: None,
                    capture: None,
                    system_clear: false,
                    #[cfg(all(windows, feature = "win-livecaptions"))]
                    lc: None,
                    #[cfg(windows)]
                    screen: None,
                    speaker: None,
                    fed,
                    audio_tx,
                    audio_rx,
                    frames: Frames16k::new(),
                    buf: Vec::new(),
                    last_audio: now,
                    padded_until: now,
                    reopen_at: now,
                    capture_lost: false,
                    asr_errors: 0,
                    asr_took: VecDeque::new(),
                    asr_stand_in: false,
                    asr_busy: None,
                    asr_retries: ASR_RETRIES,
                    asr_retry_at: None,
                    asr_downloading: false,
                    tr_downloading: false,
                };
                engine.run(cmd_rx);
                engine.stop_capture();
                // Read aloud ends (and frees its GPU context) before the app may exit.
                drop(engine.speaker.take());
                drop(exit);
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
            let screen_rx = self.screen_rx();
            let tts_rx = self
                .speaker
                .as_ref()
                .map_or_else(never, |s| s.warnings.clone());
            let activity_rx = self
                .speaker
                .as_ref()
                .map_or_else(never, |s| s.activity.clone());
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
                recv(screen_rx) -> ev => self.on_screen(
                    ev.unwrap_or_else(|_| ScreenEvent::Error("reader stopped".into())),
                ),
                recv(tr_rx) -> ev => match ev {
                    Ok(e) => self.on_tr(e),
                    Err(_) => self.on_tr(TranslatorEvent::Error("translation worker stopped".into())),
                },
                recv(tts_rx) -> w => match w {
                    Ok(text) => self.events().status(StatusLevel::Warning, text, Some(Duration::from_secs(8))),
                    Err(_) => self.speaker = None,
                },
                recv(activity_rx) -> a => if let Ok(a) = a {
                    self.events().send(EngineEvent::Speech(a));
                },
                default(timeout) => {}
            }
            let now = Instant::now();
            if self.running {
                self.check_capture(now);
                self.pad_silence(now);
                self.check_asr(now);
            }
            self.core.tick(now);
            self.submit();
        }
    }

    fn submit(&mut self) {
        for req in std::mem::take(&mut self.core.asr_out) {
            if let Some((_, w)) = &self.asr {
                w.submit(req);
                self.asr_busy = Some(Instant::now());
            }
        }
        if let (Some(id), Some((_, w))) = (self.core.tr_drop.take(), &self.tr) {
            w.give_up(id);
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
        #[cfg(omnivoice)]
        if let Some((pcm, text)) = self.core.voice_out.take()
            && let Some(s) = &self.speaker
        {
            s.learn(pcm, text);
        }
    }

    fn feed(&mut self, now: Instant) {
        for f in std::mem::take(&mut self.buf) {
            self.core.frame(f, now);
        }
    }

    /// Our own speech is playing, where captured audio may hold it (loopback, or the microphone
    /// hearing the speakers). Muting that misses whatever else plays meanwhile, but a voice that
    /// is never captioned cannot be read aloud again.
    fn muted(&self, now: Instant) -> bool {
        let tail = if self.lc_active() { LC_TAIL } else { tts::TAIL };
        self.speaker.as_ref().is_some_and(|s| s.muted(now, tail))
    }

    fn audio(&mut self, mut chunk: AudioChunk) {
        if !self.running {
            return;
        }
        let now = Instant::now();
        let clear = chunk.source == SourceKind::System && self.system_clear;
        if !clear && self.muted(now) {
            chunk.samples.fill(0.0);
        } else if chunk.source == SourceKind::System
            && let Some(gain) = self.speaker.as_ref().map(Speaker::capture_gain)
            && gain != 1.0
        {
            // Other apps are turned down for a voice; the captions should not notice.
            for s in &mut chunk.samples {
                *s = (*s * gain).clamp(-1.0, 1.0);
            }
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
                self.asr_retries = ASR_RETRIES;
                self.core.set_settings(s);
                self.core.reset();
                self.sync_speaker();
                if self.start_lc() || self.start_screen() {
                    self.ensure_workers();
                    self.running = true;
                    self.events().send(EngineEvent::State(EngineState::Loading));
                    return;
                }
                self.ensure_workers();
                self.core.reset();
                if self.start_capture(false) {
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
                let read_from =
                    |s: &Settings| (s.screen_window.clone(), s.screen_top, s.screen_bottom);
                if self.running
                    && (s.asr_backend != prev.asr_backend
                        || (self.screen_active() && read_from(&s) != read_from(&prev)))
                {
                    return self.command(EngineCommand::Start(s));
                }
                self.core.set_settings(s);
                self.sync_speaker();
                if !self.running {
                    return;
                }
                self.ensure_workers();
                if self.lc_active() || self.screen_active() {
                    return;
                }
                self.check_system_clear();
                let s = &self.core.settings;
                if (s.source, &s.input_device, &s.output_device, s.speak)
                    != (
                        prev.source,
                        &prev.input_device,
                        &prev.output_device,
                        prev.speak,
                    )
                {
                    self.stop_capture();
                    if !self.start_capture(false) {
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
            EngineCommand::StopSpeech => {
                if let Some(s) = &self.speaker {
                    s.clear();
                }
            }
            #[cfg(omnivoice)]
            EngineCommand::RelearnVoice => {
                self.core.voice.relearn();
                if let Some(s) = &self.speaker {
                    s.forget_voice();
                }
                self.events()
                    .send(EngineEvent::Speech(tts::Activity::VoiceForgotten));
            }
            EngineCommand::Shutdown => {}
        }
    }

    /// `reopen`: the device went away mid-run. Failing then is not an error (it is tried again),
    /// and the microphone is no substitute for system audio that was working.
    fn start_capture(&mut self, reopen: bool) -> bool {
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
                s.speak,
                self.audio_tx.clone(),
            )
        };
        match start(s.source) {
            Ok(c) if reopen && c.mic_instead => false,
            Err(e) if reopen => {
                log::debug!("audio capture not back yet: {e:#}");
                false
            }
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
                self.check_system_clear();
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

    fn check_system_clear(&mut self) {
        let s = &self.core.settings;
        self.system_clear = self.capture.as_ref().is_some_and(|c| {
            c.own_excluded
                || !crate::audio::same_output(s.speak_device.as_deref(), s.output_device.as_deref())
        });
    }

    /// Reopens the capture when its device went away (unplugged headset, default device changed).
    fn check_capture(&mut self, now: Instant) {
        if now < self.reopen_at {
            return;
        }
        let failure = self.capture.as_ref().and_then(AudioCapture::failure);
        if failure.is_none() && !self.capture_lost {
            return;
        }
        self.reopen_at = now + REOPEN_EVERY;
        if let Some(reason) = failure {
            log::warn!("audio capture lost ({reason}), reopening");
        }
        self.stop_capture();
        if self.start_capture(true) {
            // Only a warning takes the place of the one that stays up while the device is away.
            let level = if std::mem::take(&mut self.capture_lost) {
                StatusLevel::Warning
            } else {
                StatusLevel::Info
            };
            self.events().status(
                level,
                "Audio device changed, captions continue".into(),
                Some(Duration::from_secs(3)),
            );
        } else if !std::mem::replace(&mut self.capture_lost, true) {
            self.events().status(
                StatusLevel::Warning,
                "Audio device lost, waiting for it".into(),
                None,
            );
        }
    }

    /// Reloads a speech model that failed to load, or whose worker never answered.
    fn check_asr(&mut self, now: Instant) {
        if self.asr_retry_at.is_some_and(|t| now >= t) {
            self.asr_retry_at = None;
            self.ensure_workers();
        }
        if self
            .asr_busy
            .is_some_and(|t| now.saturating_duration_since(t) > ASR_STUCK)
        {
            self.restart_asr("Speech model stopped answering, reloading");
        }
    }

    fn large_took(&mut self, ms: u32) {
        self.asr_took.push_back(ms);
        if self.asr_took.len() > ASR_SLOW_RUNS {
            self.asr_took.pop_front();
        }
        if self.asr_took.len() < ASR_SLOW_RUNS
            || self.asr_took.iter().sum::<u32>() <= ASR_SLOW_MS * ASR_SLOW_RUNS as u32
            || !models::is_cached(ModelId::WhisperTurbo)
        {
            return;
        }
        self.asr_took.clear();
        self.asr_stand_in = true;
        log::warn!("the Large speech model is too slow here, Turbo takes over");
        self.events().status(
            StatusLevel::Warning,
            "Large speech model is too slow here, using Turbo".into(),
            Some(Duration::from_secs(8)),
        );
        self.ensure_workers();
    }

    fn restart_asr(&mut self, why: &str) {
        log::error!("{why}");
        self.asr = None;
        self.asr_errors = 0;
        self.asr_busy = None;
        self.events().status(
            StatusLevel::Warning,
            why.into(),
            Some(Duration::from_secs(5)),
        );
        self.ensure_workers();
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
        #[cfg(windows)]
        {
            self.screen = None;
        }
        self.running = false;
        self.capture_lost = false;
        self.asr_retry_at = None;
        self.asr_busy = None;
        self.core.reset();
        self.core.caption(CaptionUpdate::Clear);
    }

    fn sync_speaker(&mut self) {
        let s = &self.core.settings;
        if !s.speak {
            if self.speaker.take().is_some() {
                for a in [tts::Activity::Idle, tts::Activity::VoiceForgotten] {
                    self.events().send(EngineEvent::Speech(a));
                }
            }
            return;
        }
        self.speaker
            .get_or_insert_with(Speaker::spawn)
            .configure(s.speak_style());
    }

    /// Workers survive Stop/Start; they are rebuilt only when their model changes.
    fn ensure_workers(&mut self) {
        let mut model = self.core.settings.model;
        if model != ModelChoice::Large {
            self.asr_stand_in = false;
        } else if self.asr_stand_in {
            model = ModelChoice::Turbo;
        }
        let key = (model, self.core.settings.trim_audio);
        if !self.lc_active()
            && !self.screen_active()
            && self.asr.as_ref().is_none_or(|(k, _)| *k != key)
        {
            self.asr = Some((key, AsrWorker::spawn(key.0, key.1)));
            self.asr_took.clear();
            self.asr_downloading = false;
            self.asr_busy = None;
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
                self.asr_retries = ASR_RETRIES;
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
                self.asr_busy = None;
                match &r.error {
                    Some(e) => {
                        log::warn!("transcription failed: {e}");
                        self.asr_errors += 1;
                    }
                    None => self.asr_errors = 0,
                }
                let ms = r.ms;
                self.core.asr_result(r, Instant::now());
                self.events().send(EngineEvent::Stats {
                    asr_ms: ms,
                    device: self.asr_device.clone(),
                });
                if self.asr_errors >= ASR_MAX_ERRORS {
                    self.restart_asr("Speech model keeps failing, reloading");
                } else if self
                    .asr
                    .as_ref()
                    .is_some_and(|(k, _)| k.0 == ModelChoice::Large)
                {
                    self.large_took(ms);
                }
            }
            AsrEvent::Error(e) => {
                self.asr = None;
                self.asr_downloading = false;
                self.asr_busy = None;
                self.core.sched.forget_in_flight();
                self.core.sched.ready = false;
                if self.running && self.asr_retries > 0 {
                    self.asr_retries -= 1;
                    self.asr_retry_at = Some(Instant::now() + ASR_RETRY_AFTER);
                    log::error!("speech model failed, trying again: {e}");
                    self.events().status(
                        StatusLevel::Warning,
                        "Speech model failed, trying again".into(),
                        Some(ASR_RETRY_AFTER),
                    );
                    return;
                }
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

    #[cfg(windows)]
    fn screen_active(&self) -> bool {
        self.screen.is_some()
    }

    #[cfg(not(windows))]
    fn screen_active(&self) -> bool {
        false
    }

    /// Starts reading subtitles off the screen when that is chosen; false = listen.
    fn start_screen(&mut self) -> bool {
        let s = &self.core.settings;
        if s.asr_backend != AsrBackend::ScreenText || self.fed {
            return false;
        }
        #[cfg(windows)]
        {
            self.screen = Some(ScreenText::spawn(
                s.screen_window.clone(),
                (s.screen_top, s.screen_bottom),
                s.spoken_language.clone().unwrap_or_else(|| "en".into()),
            ));
            true
        }
        #[cfg(not(windows))]
        {
            self.events().status(
                StatusLevel::Warning,
                "Subtitles can only be read off the screen on Windows, listening instead".into(),
                Some(Duration::from_secs(8)),
            );
            false
        }
    }

    #[cfg(windows)]
    fn screen_rx(&self) -> Receiver<ScreenEvent> {
        self.screen
            .as_ref()
            .map_or_else(never, |s| s.events.clone())
    }

    #[cfg(not(windows))]
    fn screen_rx(&self) -> Receiver<ScreenEvent> {
        never()
    }

    fn on_screen(&mut self, ev: ScreenEvent) {
        match ev {
            ScreenEvent::Ready(from) => {
                if self.running {
                    self.events().send(EngineEvent::State(EngineState::Running));
                    self.events().status(
                        StatusLevel::Info,
                        format!("Running: reading subtitles from {from}"),
                        Some(Duration::from_secs(3)),
                    );
                }
            }
            ScreenEvent::Waiting(from) => {
                if self.running {
                    self.events().send(EngineEvent::State(EngineState::Running));
                    self.events().status(
                        StatusLevel::Warning,
                        format!("Waiting for the window {from} to read subtitles from"),
                        None,
                    );
                }
            }
            ScreenEvent::Minimized(from) => {
                if self.running {
                    self.events().status(
                        StatusLevel::Warning,
                        format!("The window {from} is minimized: bring it back to have its subtitles read"),
                        None,
                    );
                }
            }
            ScreenEvent::Line(text) => {
                if self.running {
                    self.core.screen_line(text, Instant::now());
                }
            }
            ScreenEvent::Error(e) => {
                #[cfg(windows)]
                {
                    self.screen = None;
                }
                if self.running {
                    log::error!("reading subtitles off the screen failed: {e}");
                    self.stop();
                    self.events().status(
                        StatusLevel::Error,
                        format!("Cannot read subtitles off the screen: {e}"),
                        None,
                    );
                    self.events().send(EngineEvent::State(EngineState::Error));
                }
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
        if !self.start_capture(false) {
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
            TranslatorEvent::Trouble(e) => self.events().status(
                StatusLevel::Warning,
                format!("Translation is failing, trying again: {e}"),
                Some(Duration::from_secs(8)),
            ),
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

/// Dropped when the engine thread ends, by a panic too: says so, and wakes the UI to look.
struct ExitSignal {
    done: Option<Sender<()>>,
    notify: Notifier,
}

impl Drop for ExitSignal {
    fn drop(&mut self) {
        self.done.take();
        (self.notify)();
    }
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

    pub fn finished(&self) -> bool {
        matches!(
            self.done.try_recv(),
            Err(crossbeam_channel::TryRecvError::Disconnected)
        )
    }

    /// Sends Shutdown and joins the engine thread, giving up after `timeout`.
    pub fn shutdown(self, timeout: Duration) {
        self.send(EngineCommand::Shutdown);
        let _ = self.done.recv_timeout(timeout);
    }
}

/// The sentences of a subtitle longer than `SPLIT_CHARS`; a shorter one whole.
fn pieces(text: &str) -> Vec<String> {
    if text.chars().count() <= SPLIT_CHARS {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
        if assembler::ends_sentence(w) && cur.chars().count() >= PIECE_CHARS {
            out.push(std::mem::take(&mut cur));
        }
    }
    match out.last_mut() {
        Some(last) if !cur.is_empty() && cur.chars().count() < PIECE_CHARS => {
            last.push(' ');
            last.push_str(&cur);
        }
        _ if !cur.is_empty() => out.push(cur),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_subtitle_goes_on_sentence_by_sentence() {
        assert_eq!(pieces("W... wait a second."), ["W... wait a second."]);
        assert_eq!(
            pieces(
                "This chump change. How am I gonna knock a bad bitch, if I don't got a fat pocket?"
            ),
            [
                "This chump change.",
                "How am I gonna knock a bad bitch, if I don't got a fat pocket?"
            ]
        );
        // Sentences too short to translate alone stay with their neighbours.
        assert_eq!(
            pieces("No. No, I'm not calling anybody that, I would never say such a thing. Ever."),
            ["No. No, I'm not calling anybody that, I would never say such a thing. Ever."]
        );
    }

    #[test]
    fn ending_the_engine_is_noticed_and_wakes_the_ui() {
        let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = woken.clone();
        let (engine, _feed) = Engine::spawn_fed(Arc::new(move || {
            w.store(true, std::sync::atomic::Ordering::SeqCst)
        }));
        assert!(!engine.finished());
        engine.send(EngineCommand::Shutdown);
        let t = Instant::now();
        while !engine.finished() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(engine.finished());
        assert!(woken.load(std::sync::atomic::Ordering::SeqCst));
    }

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
            false,
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
    fn sentences_are_translated_ahead_and_one_by_one() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let settings = Settings {
            translate: true,
            target_language: "tr".into(),
            ..Settings::default()
        };
        let mut c = Core::new(
            settings,
            Events {
                tx,
                notify: Arc::new(|| {}),
            },
            false,
        );
        c.tr_ready = true;
        let live = |text: &str, whole: &[&str]| Out::Live {
            text: text.into(),
            lang: Some("en".into()),
            whole: whole.iter().map(|s| s.to_string()).collect(),
        };
        let sentence = |text: &str| Out::Sentence {
            id: 1,
            text: text.into(),
            lang: Some("en".into()),
        };
        let finals = || -> Vec<(String, Option<String>)> {
            rx.try_iter()
                .filter_map(|e| match e {
                    EngineEvent::Caption(CaptionUpdate::Final {
                        original,
                        translated,
                        ..
                    }) => Some((original, translated)),
                    _ => None,
                })
                .collect()
        };
        let f = |o: &str, t: &str| (o.to_string(), Some(t.to_string()));

        // A sentence whole on the live line is translated before it is a caption,
        c.apply(vec![live("Hello there.", &["Hello there."])]);
        assert_eq!(c.tr_out.len(), 1);
        assert_eq!(c.tr_out[0].text, "Hello there.");
        c.translation(c.tr_out[0].id, Some("Merhaba.".into()));
        // so that the caption comes translated, and is not asked for again.
        c.apply(vec![sentence("Hello there.")]);
        assert_eq!(c.tr_out.len(), 1);
        assert_eq!(finals(), [f("Hello there.", "Merhaba.")]);

        // One being translated when the live line words it another way is given up.
        c.apply(vec![live("How is it.", &["How is it."])]);
        let stale = c.tr_out[1].id;
        c.apply(vec![live("How are you? I", &["How are you?"])]);
        assert_eq!(c.tr_drop, Some(stale));
        assert_eq!(c.tr_out[2].text, "How are you?");
        assert_eq!(
            c.tr_out[2].context,
            [("Hello there.".to_string(), "Merhaba.".to_string())]
        );
        c.translation(stale, None);

        // The caption grows while that is out: the request is its own now, only what was added
        // is translated, and what was on the screen stays as it was.
        c.apply(vec![sentence("Hello there. How are you?")]);
        assert_eq!(c.tr_out.len(), 3);
        assert_eq!(finals(), [f("Hello there. How are you?", "Merhaba.")]);
        c.translation(c.tr_out[2].id, Some("Nasılsın?".into()));
        assert_eq!(
            finals(),
            [f("Hello there. How are you?", "Merhaba. Nasılsın?")]
        );

        // A caption does not wait behind a sentence that is none yet.
        c.apply(vec![live("See you. Bye", &["See you."])]);
        let ahead = c.tr_out[3].id;
        c.apply(vec![sentence("Hello there. How are you? Go.")]);
        assert_eq!(c.tr_drop, Some(ahead));
        assert_eq!(c.tr_out[4].text, "Go.");
    }

    #[test]
    fn with_the_original_hidden_a_caption_shows_as_much_as_is_translated() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut settings = Settings {
            translate: true,
            target_language: "tr".into(),
            ..Settings::default()
        };
        settings.caption.show_original = false;
        let mut c = Core::new(
            settings,
            Events {
                tx,
                notify: Arc::new(|| {}),
            },
            false,
        );
        c.tr_ready = true;
        let sentence = |text: &str| Out::Sentence {
            id: 1,
            text: text.into(),
            lang: Some("en".into()),
        };
        let translated = || -> Vec<Option<String>> {
            rx.try_iter()
                .filter_map(|e| match e {
                    EngineEvent::Caption(CaptionUpdate::Final { translated, .. }) => {
                        Some(translated)
                    }
                    _ => None,
                })
                .collect()
        };
        c.apply(vec![sentence("Hello there.")]);
        c.apply(vec![sentence("Hello there. Go.")]);
        assert!(translated().is_empty(), "nothing to show yet");
        c.translation(c.tr_out[0].id, Some("Merhaba.".into()));
        assert_eq!(translated(), [Some("Merhaba.".to_string())]);
        // One that fails is shown as it was said.
        c.translation(c.tr_out[1].id, None);
        assert_eq!(translated(), [Some("Merhaba. Go.".to_string())]);
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
                false,
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

        // Speech already in the target language is only read when asked for.
        let mut c = core(true, true, true);
        c.settings.target_language = "en".into();
        c.lc_text("", t);
        c.lc_text("Hello there. How are", t);
        assert!(said(&c).is_empty());
        let mut c = core(true, true, true);
        c.settings.target_language = "en".into();
        c.settings.speak_same_lang = true;
        c.lc_text("", t);
        c.lc_text("Hello there. How are", t);
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
