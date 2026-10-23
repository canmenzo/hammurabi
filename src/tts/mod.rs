//! Reads final captions aloud with the OS's own voices: WinRT SpeechSynthesizer (Windows),
//! AVSpeechSynthesizer (macOS), spd-say or espeak-ng (Linux; spawned, so the binary does not
//! link libspeechd), with downloaded Supertonic 3 voices (`supertonic`), or in the captioned
//! speaker's own voice (`omnivoice`). Utterances are spoken one at a time on thread "hm-tts".

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(any(windows, target_os = "macos")), path = "unix.rs")]
mod imp;
#[cfg(omnivoice)]
pub mod omnivoice;
pub mod supertonic;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::models::{self, ModelId};
use crate::settings::lang_name;
use supertonic::Supertonic;

/// Utterances waiting behind the one being spoken; older ones are dropped to stay near real time.
const MAX_QUEUED: usize = 2;
/// Captured audio stays muted this long after speech ends (output latency, room echo).
pub const TAIL: Duration = Duration::from_millis(600);
const POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// BCP 47, e.g. "tr-TR" (Linux: often just "tr").
    pub lang: String,
    /// None when the OS does not say.
    pub gender: Option<Gender>,
}

pub struct Persona {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Preferred voice gender; None = any.
    pub gender: Option<Gender>,
    /// Multipliers, 1.0 = normal.
    pub rate: f32,
    pub pitch: f32,
}

pub const CUSTOM: &str = "custom";

const fn def(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    gender: Option<Gender>,
    rate: f32,
    pitch: f32,
) -> Persona {
    Persona {
        id,
        name,
        description,
        gender,
        rate,
        pitch,
    }
}

const F: Option<Gender> = Some(Gender::Female);
const M: Option<Gender> = Some(Gender::Male);

/// Shared with the Babil extension: keep ids, names and values identical there.
pub const PERSONAS: [Persona; 7] = [
    def("lale", "Lale", "Clear and neutral", F, 1.0, 1.0),
    def("ayla", "Ayla", "Bright and friendly", F, 1.05, 1.15),
    def("deniz", "Deniz", "Calm and steady", M, 0.95, 0.95),
    def("ates", "Ateş", "Warm and deep", M, 1.0, 0.8),
    def(
        "bilge",
        "Bilge",
        "Slow and patient, easy to follow",
        F,
        0.8,
        1.0,
    ),
    def(
        "ruzgar",
        "Rüzgar",
        "Quick, keeps up with fast talkers",
        None,
        1.35,
        1.05,
    ),
    def(CUSTOM, "Custom", "Your own voice and speed", None, 1.0, 1.0),
];

/// The persona with `id`; unknown ids get the first one.
pub fn persona(id: &str) -> &'static Persona {
    PERSONAS.iter().find(|p| p.id == id).unwrap_or(&PERSONAS[0])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    /// The OS's voices.
    #[default]
    System,
    /// Supertonic 3 where it speaks the language and the pack is downloaded; OS voices otherwise.
    Supertonic,
    /// The captioned speaker's voice, cloned by OmniVoice (Windows builds with the `omnivoice`
    /// feature); Supertonic 3 as above when it cannot run.
    Speaker,
}

/// How captions are read: a persona, or for `CUSTOM` the voice and rate picked by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub persona: String,
    pub voice: Option<String>,
    pub rate: f32,
    pub engine: Engine,
}

/// Supertonic (language, voice style, speed) for text in `lang` read with `style`, from Babil's
/// catalog; None when an OS voice reads it. Custom always uses an OS voice.
pub fn neural(style: &Style, lang: &str) -> Option<(&'static str, &'static str, f32)> {
    if style.engine == Engine::System {
        return None;
    }
    let base = lang.split(['-', '_']).next()?;
    let lang = supertonic::LANGS.iter().find(|&&l| l == base)?;
    let (voice, speed) = match persona(&style.persona).id {
        "lale" => ("F3", 1.05),
        "ayla" => ("F2", 1.1),
        "deniz" => ("M4", 1.0),
        "ates" => ("M2", 1.05),
        "bilge" => ("F5", 0.84),
        "ruzgar" => ("M1", 1.42),
        _ => return None,
    };
    Some((lang, voice, speed))
}

impl Style {
    /// (voice, rate, pitch) for text in `lang`; the voice as in `resolve`.
    pub fn resolve<'a>(
        &self,
        voices: &'a [Voice],
        lang: &str,
    ) -> Result<(Option<&'a Voice>, f32, f32), String> {
        let p = persona(&self.persona);
        if p.id == CUSTOM {
            Ok((
                resolve(voices, lang, self.voice.as_deref(), None)?,
                self.rate,
                1.0,
            ))
        } else {
            Ok((resolve(voices, lang, None, p.gender)?, p.rate, p.pitch))
        }
    }
}

/// A short sentence to preview a persona with in `lang`.
pub fn sample(lang: &str, name: &str) -> String {
    let text = match lang.split(['-', '_']).next().unwrap_or(lang) {
        "tr" => "Merhaba! Altyazıları bu sesle okuyacağım.",
        "en" => "Hi! This is how I will read your captions.",
        "de" => "Hallo! So lese ich deine Untertitel vor.",
        "es" => "¡Hola! Así leeré tus subtítulos.",
        "fr" => "Bonjour ! Voici comment je lirai vos sous-titres.",
        "ar" => "مرحبًا! بهذا الصوت سأقرأ لك الترجمة.",
        "ru" => "Привет! Вот так я буду читать ваши субтитры.",
        _ => return format!("Hi, I'm {name}. This is how I will read your captions."),
    };
    text.into()
}

/// Installed voices; empty when the OS lists none (Linux without speech-dispatcher).
pub fn voices() -> Vec<Voice> {
    imp::voices()
}

/// Whether a voice for `voice` (BCP 47) can read text in caption language `code`.
pub fn lang_matches(voice: &str, code: &str) -> bool {
    let tags =
        |s: &str| -> Vec<String> { s.split(['-', '_']).map(str::to_ascii_lowercase).collect() };
    let (v, c) = (tags(voice), tags(code));
    if v[0] != c[0] {
        return false;
    }
    let hant = |t: &[String]| {
        t[1..]
            .iter()
            .any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo"))
    };
    c[0] != "zh" || hant(&v) == hant(&c)
}

/// `preferred` if it speaks `lang`, else the voice for `lang` that best fits `gender` (None = any):
/// that gender, then unknown, then the other; among equals the OS default for `lang`, then the
/// first listed.
fn pick<'a>(
    voices: &'a [Voice],
    lang: &str,
    preferred: Option<&str>,
    gender: Option<Gender>,
    default: Option<&str>,
) -> Option<&'a Voice> {
    let ok = |v: &&Voice| lang_matches(&v.lang, lang);
    let fit = |v: &Voice| match (gender, v.gender) {
        (None, _) => 0,
        (Some(want), Some(g)) if want == g => 0,
        (_, None) => 1,
        _ => 2,
    };
    let is = |v: &Voice, id: Option<&str>| Some(v.id.as_str()) == id;
    voices
        .iter()
        .filter(ok)
        .find(|v| is(v, preferred))
        .or_else(|| {
            voices
                .iter()
                .filter(ok)
                .min_by_key(|v| (fit(v), !is(v, default)))
        })
}

/// The voice for `lang` (see `pick`). Ok(None) leaves the choice to the OS (no voice list, or
/// language unknown); Err says which voice to install.
pub fn resolve<'a>(
    voices: &'a [Voice],
    lang: &str,
    preferred: Option<&str>,
    gender: Option<Gender>,
) -> Result<Option<&'a Voice>, String> {
    if voices.is_empty() || lang.is_empty() {
        return Ok(None);
    }
    pick(
        voices,
        lang,
        preferred,
        gender,
        imp::default_voice(lang).as_deref(),
    )
    .map(Some)
    .ok_or_else(|| missing_voice(lang))
}

pub fn missing_voice(lang: &str) -> String {
    format!(
        "No {} voice is installed for reading aloud. {}",
        lang_name(lang),
        imp::VOICE_HINT
    )
}

/// Speaks `text` and returns when it is done or `cancel` is set. `rate`, `pitch`: 1.0 = normal.
pub fn speak(
    text: &str,
    lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    pitch: f32,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    imp::speak(text, lang, voice, rate, pitch, cancel)
}

pub struct Utterance {
    pub text: String,
    pub lang: String,
    /// None = the configured style.
    pub style: Option<Style>,
}

/// Adds `u`, dropping the oldest waiting utterances beyond `MAX_QUEUED`; returns how many were dropped.
fn enqueue(queue: &mut VecDeque<Utterance>, u: Utterance) -> usize {
    queue.push_back(u);
    let dropped = queue.len().saturating_sub(MAX_QUEUED);
    queue.drain(..dropped);
    dropped
}

/// When captured audio must be ignored: while speaking and for a tail after it.
#[derive(Clone, Copy, Debug, Default)]
struct Gate {
    speaking: bool,
    ended: Option<Instant>,
}

impl Gate {
    fn closed(&self, now: Instant, tail: Duration) -> bool {
        self.speaking || self.ended.is_some_and(|e| now < e + tail)
    }
}

struct State {
    queue: VecDeque<Utterance>,
    gate: Gate,
    style: Style,
    quit: bool,
    /// Speech (16 kHz) and its transcript for Speaker's voice to learn.
    #[cfg(omnivoice)]
    learn: Option<(Vec<f32>, String)>,
    #[cfg(omnivoice)]
    forget: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cancel: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Thread "hm-tts". Dropping it stops speech and ends the thread.
pub struct Speaker {
    shared: Arc<Shared>,
    /// Problems for the user (missing voice, no speech program), each sent once.
    pub warnings: Receiver<String>,
    /// Disconnects when hm-tts has ended (and freed the GPU).
    #[cfg(omnivoice)]
    done: Receiver<()>,
}

impl Speaker {
    pub fn spawn() -> Speaker {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                gate: Gate::default(),
                style: Style {
                    persona: PERSONAS[0].id.into(),
                    voice: None,
                    rate: 1.0,
                    engine: Engine::System,
                },
                quit: false,
                #[cfg(omnivoice)]
                learn: None,
                #[cfg(omnivoice)]
                forget: false,
            }),
            wake: Condvar::new(),
            cancel: AtomicBool::new(false),
        });
        let (tx, warnings) = crossbeam_channel::unbounded();
        #[cfg(omnivoice)]
        let (done_tx, done) = crossbeam_channel::bounded::<()>(1);
        let s = shared.clone();
        std::thread::Builder::new()
            .name("hm-tts".into())
            .spawn(move || {
                run(&s, &tx);
                #[cfg(omnivoice)]
                drop(done_tx);
            })
            .expect("spawn hm-tts");
        Speaker {
            shared,
            warnings,
            #[cfg(omnivoice)]
            done,
        }
    }

    pub fn configure(&self, style: Style) {
        self.shared.lock().style = style;
        // Speaker's voice loads as soon as it is picked.
        self.shared.wake.notify_one();
    }

    /// Speaker's voice learns the voice in `pcm` (16 kHz) that says `text`.
    #[cfg(omnivoice)]
    pub fn learn(&self, pcm: Vec<f32>, text: String) {
        self.shared.lock().learn = Some((pcm, text));
        self.shared.wake.notify_one();
    }

    /// Speaker's voice drops the learned voice until it learns one again.
    #[cfg(omnivoice)]
    pub fn forget_voice(&self) {
        let mut st = self.shared.lock();
        st.learn = None;
        st.forget = true;
        self.shared.wake.notify_one();
    }

    pub fn say(&self, text: String, lang: String) {
        let u = Utterance {
            text,
            lang,
            style: None,
        };
        let dropped = enqueue(&mut self.shared.lock().queue, u);
        if dropped > 0 {
            log::debug!("tts behind, dropped {dropped} utterance(s)");
        }
        self.shared.wake.notify_one();
    }

    /// Speaks `text` with `style` right away, stopping whatever is speaking or waiting.
    pub fn preview(&self, text: String, lang: String, style: Style) {
        let mut st = self.shared.lock();
        st.queue.clear();
        st.queue.push_back(Utterance {
            text,
            lang,
            style: Some(style),
        });
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
    }

    /// Empties the queue and stops the current utterance.
    pub fn clear(&self) {
        let mut st = self.shared.lock();
        st.queue.clear();
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// True while captured audio would contain our own speech.
    pub fn muted(&self, now: Instant, tail: Duration) -> bool {
        self.shared.lock().gate.closed(now, tail)
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
        // The GPU context must be freed before the process exits.
        #[cfg(omnivoice)]
        let _ = self.done.recv_timeout(Duration::from_secs(2));
    }
}

/// Plays synthesized `pcm` with captured audio muted, unless cancelled meanwhile.
fn play(shared: &Shared, pcm: &[f32], rate: u32) -> anyhow::Result<()> {
    if shared.cancel.load(Ordering::Relaxed) || pcm.is_empty() {
        return Ok(());
    }
    shared.lock().gate.speaking = true;
    let r = supertonic::play(pcm, rate, &shared.cancel);
    shared.lock().gate = Gate {
        speaking: false,
        ended: Some(Instant::now()),
    };
    r
}

/// Synthesizes `text` with Supertonic (loading it into `engine` first) and plays it.
fn speak_neural(
    shared: &Shared,
    engine: &mut Option<Supertonic>,
    text: &str,
    (lang, voice, speed): (&str, &str, f32),
) -> anyhow::Result<()> {
    let st = match engine {
        Some(st) => st,
        None => engine.insert(Supertonic::load(&models::model_dir(ModelId::Supertonic)?)?),
    };
    let t = Instant::now();
    let pcm = st.synthesize(text, lang, voice, speed)?;
    let (ms, secs) = (
        t.elapsed().as_millis(),
        pcm.len() as f32 / st.sample_rate as f32,
    );
    log::info!(
        "supertonic {voice}: {secs:.2} s of audio in {ms} ms (RTF {:.2})",
        ms as f32 / 1000.0 / secs
    );
    play(shared, &pcm, st.sample_rate)
}

/// Synthesizes `text` with OmniVoice in the learned voice and plays it.
#[cfg(omnivoice)]
fn speak_cloned(
    shared: &Shared,
    ov: &mut omnivoice::OmniVoice,
    text: &str,
    lang: &str,
) -> anyhow::Result<()> {
    let t = Instant::now();
    let pcm = ov.synthesize(text, lang, omnivoice::STEPS)?;
    let (ms, secs) = (
        t.elapsed().as_millis(),
        pcm.len() as f32 / omnivoice::SAMPLE_RATE as f32,
    );
    let voice = if ov.has_voice() {
        "speaker's voice"
    } else {
        "own voice"
    };
    log::info!(
        "omnivoice ({voice}): {secs:.2} s of audio in {ms} ms (RTF {:.2})",
        ms as f32 / 1000.0 / secs
    );
    play(shared, &pcm, omnivoice::SAMPLE_RATE)
}

enum Job {
    Speak(Utterance, Style),
    /// Load or unload Speaker's voice to match the style.
    #[cfg(omnivoice)]
    Load,
    #[cfg(omnivoice)]
    Learn(Vec<f32>, String),
}

fn run(shared: &Shared, warnings: &Sender<String>) {
    let mut neural_engine = None;
    // Speaker's voice: the loaded context, and whether loading it failed.
    #[cfg(omnivoice)]
    let (mut ov, mut ov_failed) = (None::<omnivoice::OmniVoice>, false);
    let mut warned = HashSet::new();
    let mut warn = |text: String| {
        if warned.insert(text.clone()) {
            log::warn!("{text}");
            let _ = warnings.send(text);
        }
    };
    loop {
        let job = {
            let mut st = shared.lock();
            loop {
                if st.quit {
                    return;
                }
                #[cfg(omnivoice)]
                {
                    let wanted = st.style.engine == Engine::Speaker;
                    if (wanted
                        && ov.is_none()
                        && !ov_failed
                        && models::is_cached(ModelId::OmniVoice))
                        || (!wanted && (ov.is_some() || ov_failed))
                    {
                        break Job::Load;
                    }
                    if std::mem::take(&mut st.forget)
                        && let Some(ov) = &mut ov
                    {
                        ov.forget();
                    }
                    if let Some((pcm, text)) = st.learn.take() {
                        break Job::Learn(pcm, text);
                    }
                }
                if let Some(u) = st.queue.pop_front() {
                    // Under the lock, so a clear() after the pop still cancels this utterance.
                    shared.cancel.store(false, Ordering::Relaxed);
                    let style = u.style.clone().unwrap_or_else(|| st.style.clone());
                    break Job::Speak(u, style);
                }
                st = shared.wake.wait(st).unwrap_or_else(|p| p.into_inner());
            }
        };
        let (u, style) = match job {
            Job::Speak(u, style) => (u, style),
            #[cfg(omnivoice)]
            Job::Load => {
                if ov.is_some() || ov_failed {
                    (ov, ov_failed) = (None, false);
                    log::info!("Speaker's voice unloaded");
                    continue;
                }
                let t = Instant::now();
                match models::model_dir(ModelId::OmniVoice)
                    .and_then(|d| omnivoice::OmniVoice::load(&d))
                {
                    Ok(o) => {
                        log::info!("Speaker's voice ready in {} ms", t.elapsed().as_millis());
                        ov = Some(o);
                    }
                    Err(e) => {
                        ov_failed = true;
                        warn(format!(
                            "Speaker's voice could not start, reading with other voices: {e:#}"
                        ));
                    }
                }
                continue;
            }
            #[cfg(omnivoice)]
            Job::Learn(pcm, text) => {
                if let Some(ov) = &mut ov {
                    let t = Instant::now();
                    match ov.learn(&pcm, crate::types::SAMPLE_RATE, &text) {
                        Ok(()) => log::info!(
                            "learned the speaker's voice in {} ms",
                            t.elapsed().as_millis()
                        ),
                        Err(e) => log::warn!("Speaker's voice: {e:#}"),
                    }
                }
                continue;
            }
        };
        #[cfg(omnivoice)]
        if style.engine == Engine::Speaker {
            match &mut ov {
                Some(o) => match speak_cloned(shared, o, &u.text, &u.lang) {
                    Ok(()) => continue,
                    Err(e) => warn(format!(
                        "Speaker's voice could not read aloud, using other voices: {e:#}"
                    )),
                },
                None if !models::is_cached(ModelId::OmniVoice) => warn(
                    "Speaker's voice is not downloaded yet (Settings > Read aloud), using other voices"
                        .into(),
                ),
                None => {}
            }
        }
        if let Some(n) = neural(&style, &u.lang)
            && models::is_cached(ModelId::Supertonic)
        {
            let loaded = neural_engine.is_some();
            match speak_neural(shared, &mut neural_engine, &u.text, n) {
                Ok(()) => continue,
                Err(e) => {
                    if !loaded {
                        models::remove_corrupt(ModelId::Supertonic);
                    }
                    warn(format!(
                        "Supertonic could not read aloud, using a system voice: {e:#}"
                    ));
                }
            }
        }
        let all = voices();
        let (voice, rate, pitch) = match style.resolve(&all, &u.lang) {
            Ok(r) => r,
            Err(e) => {
                warn(e);
                continue;
            }
        };
        shared.lock().gate.speaking = true;
        let r = speak(&u.text, &u.lang, voice, rate, pitch, &shared.cancel);
        shared.lock().gate = Gate {
            speaking: false,
            ended: Some(Instant::now()),
        };
        if let Err(e) = r {
            warn(format!("Reading aloud failed: {e:#}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(id: &str, lang: &str) -> Voice {
        Voice {
            id: id.into(),
            name: id.into(),
            lang: lang.into(),
            gender: None,
        }
    }

    fn gendered(id: &str, lang: &str, g: Gender) -> Voice {
        Voice {
            gender: Some(g),
            ..voice(id, lang)
        }
    }

    fn style(persona: &str) -> Style {
        Style {
            persona: persona.into(),
            voice: None,
            rate: 1.0,
            engine: Engine::System,
        }
    }

    #[test]
    fn matches_languages() {
        assert!(lang_matches("tr-TR", "tr"));
        assert!(lang_matches("tr_TR", "tr"));
        assert!(lang_matches("tr", "tr"));
        assert!(!lang_matches("en-US", "tr"));
        assert!(lang_matches("zh-CN", "zh"));
        assert!(!lang_matches("zh-TW", "zh"));
        assert!(lang_matches("zh-TW", "zh-Hant"));
        assert!(lang_matches("zh-HK", "zh-Hant"));
        assert!(!lang_matches("zh-CN", "zh-Hant"));
    }

    #[test]
    fn picks_voice() {
        let all = [
            voice("albert", "en-US"),
            voice("samantha", "en-US"),
            voice("yelda", "tr-TR"),
            voice("meijia", "zh-TW"),
        ];
        let id = |v: Option<&Voice>| v.map(|v| v.id.clone());
        assert_eq!(id(pick(&all, "tr", None, None, None)), Some("yelda".into()));
        assert_eq!(
            id(pick(&all, "en", None, None, Some("samantha"))),
            Some("samantha".into())
        );
        assert_eq!(
            id(pick(&all, "en", Some("albert"), None, Some("samantha"))),
            Some("albert".into())
        );
        // A preferred voice in another language does not read this one.
        assert_eq!(
            id(pick(&all, "en", Some("yelda"), None, Some("samantha"))),
            Some("samantha".into())
        );
        // An OS default for another language is ignored too.
        assert_eq!(
            id(pick(&all, "en", None, None, Some("yelda"))),
            Some("albert".into())
        );
        assert_eq!(
            id(pick(&all, "zh-Hant", None, None, None)),
            Some("meijia".into())
        );
        assert_eq!(id(pick(&all, "zh", None, None, None)), None);
        assert_eq!(resolve(&[], "tr", None, None), Ok(None));
        assert_eq!(resolve(&all, "", None, None), Ok(None));
        assert!(
            resolve(&all, "ko", None, None)
                .unwrap_err()
                .contains("Korean")
        );
    }

    #[test]
    fn picks_by_gender() {
        use Gender::{Female, Male};
        let all = [
            gendered("david", "en-US", Male),
            gendered("zira", "en-US", Female),
            gendered("mark", "en-US", Male),
            voice("eddy", "en-US"),
            gendered("yelda", "tr-TR", Female),
            voice("cem", "tr-TR"),
            gendered("milena", "ru-RU", Female),
        ];
        let id = |v: Option<&Voice>| v.map(|v| v.id.clone());
        assert_eq!(
            id(pick(&all, "en", None, Some(Female), None)),
            Some("zira".into())
        );
        assert_eq!(
            id(pick(&all, "en", None, Some(Male), None)),
            Some("david".into())
        );
        // The OS default wins among voices that fit equally, not over a better fit.
        assert_eq!(
            id(pick(&all, "en", None, Some(Male), Some("mark"))),
            Some("mark".into())
        );
        assert_eq!(
            id(pick(&all, "en", None, Some(Female), Some("david"))),
            Some("zira".into())
        );
        assert_eq!(
            id(pick(&all, "en", None, None, Some("mark"))),
            Some("mark".into())
        );
        // Unknown gender comes before the other one.
        assert_eq!(
            id(pick(&all, "tr", None, Some(Male), None)),
            Some("cem".into())
        );
        assert_eq!(
            id(pick(&all, "tr", None, Some(Female), None)),
            Some("yelda".into())
        );
        // No voice of that gender at all: any voice for the language.
        assert_eq!(
            id(pick(&all, "ru", None, Some(Male), None)),
            Some("milena".into())
        );
        // A voice picked by hand beats the gender.
        assert_eq!(
            id(pick(&all, "en", Some("mark"), Some(Female), None)),
            Some("mark".into())
        );
    }

    #[test]
    fn resolves_personas() {
        use Gender::{Female, Male};
        let all = [
            gendered("david", "en-US", Male),
            gendered("zira", "en-US", Female),
            gendered("mark", "en-US", Male),
            gendered("yelda", "tr-TR", Female),
        ];
        let r = |s: &Style, lang: &str| {
            s.resolve(&all, lang)
                .map(|(v, rate, pitch)| (v.map(|v| v.id.clone()), rate, pitch))
        };
        assert_eq!(
            r(&style("ayla"), "en"),
            Ok((Some("zira".into()), 1.05, 1.15))
        );
        assert_eq!(
            r(&style("ates"), "en"),
            Ok((Some("david".into()), 1.0, 0.8))
        );
        assert_eq!(
            r(&style("ruzgar"), "en"),
            Ok((Some("david".into()), 1.35, 1.05))
        );
        // Turkish has no male voice here: Deniz still reads it, slower and lower.
        assert_eq!(
            r(&style("deniz"), "tr"),
            Ok((Some("yelda".into()), 0.95, 0.95))
        );
        assert!(r(&style("bilge"), "ko").unwrap_err().contains("Korean"));
        // Unknown ids are Lale.
        assert_eq!(r(&style("nope"), "en"), r(&style("lale"), "en"));

        let custom = Style {
            persona: CUSTOM.into(),
            voice: Some("mark".into()),
            rate: 1.4,
            engine: Engine::System,
        };
        assert_eq!(r(&custom, "en"), Ok((Some("mark".into()), 1.4, 1.0)));
        // The manual voice only reads its own language.
        assert_eq!(r(&custom, "tr"), Ok((Some("yelda".into()), 1.4, 1.0)));
        assert_eq!(
            r(&style(CUSTOM), "en"),
            Ok((Some("david".into()), 1.0, 1.0))
        );

        // The same persona always gets the same voice, and female personas share it.
        for p in &PERSONAS {
            let first = r(&style(p.id), "en");
            assert!((0..5).all(|_| r(&style(p.id), "en") == first), "{}", p.id);
        }
        assert_eq!(r(&style("lale"), "en").unwrap().0, Some("zira".into()));
        assert_eq!(r(&style("bilge"), "en").unwrap().0, Some("zira".into()));
    }

    #[test]
    fn neural_voices() {
        let st = |p: &str| Style {
            engine: Engine::Supertonic,
            ..style(p)
        };
        assert_eq!(neural(&st("lale"), "tr"), Some(("tr", "F3", 1.05)));
        assert_eq!(neural(&st("ruzgar"), "en"), Some(("en", "M1", 1.42)));
        assert_eq!(neural(&st("bilge"), "pt-BR"), Some(("pt", "F5", 0.84)));
        assert_eq!(neural(&st("nope"), "de"), neural(&st("lale"), "de"));
        assert_eq!(neural(&st(CUSTOM), "tr"), None);
        assert_eq!(neural(&st("ayla"), "zh"), None);
        assert_eq!(neural(&st("ayla"), "th"), None);
        assert_eq!(neural(&style("ayla"), "tr"), None);
        // Speaker's voice falls back to the same Supertonic voices.
        let sp = Style {
            engine: Engine::Speaker,
            ..style("lale")
        };
        assert_eq!(neural(&sp, "tr"), Some(("tr", "F3", 1.05)));
        for p in &PERSONAS[..6] {
            assert!(neural(&st(p.id), "en").is_some(), "{}", p.id);
        }
    }

    #[test]
    fn persona_table() {
        let ids: Vec<&str> = PERSONAS.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            ["lale", "ayla", "deniz", "ates", "bilge", "ruzgar", "custom"]
        );
        assert_eq!(persona("ates").name, "Ateş");
        assert_eq!(persona("ruzgar").name, "Rüzgar");
        assert_eq!(persona("nope").id, "lale");
    }

    #[test]
    fn samples() {
        assert!(sample("tr", "Ayla").starts_with("Merhaba"));
        assert!(sample("en", "Ayla").starts_with("Hi!"));
        assert!(sample("zh-Hant", "Ayla").contains("Ayla"));
        for l in ["tr", "en", "de", "es", "fr", "ar", "ru"] {
            assert!(!sample(l, "Ayla").contains("Ayla"), "{l}");
        }
    }

    #[test]
    fn queue_drops_oldest() {
        let mut q = VecDeque::new();
        let u = |t: &str| Utterance {
            text: t.into(),
            lang: "tr".into(),
            style: None,
        };
        assert_eq!(enqueue(&mut q, u("a")), 0);
        assert_eq!(enqueue(&mut q, u("b")), 0);
        assert_eq!(enqueue(&mut q, u("c")), 1);
        assert_eq!(enqueue(&mut q, u("d")), 1);
        let left: Vec<&str> = q.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(left, ["c", "d"]);
    }

    #[test]
    fn gate_covers_speech_and_tail() {
        let t = Instant::now();
        let tail = Duration::from_millis(600);
        let mut g = Gate::default();
        assert!(!g.closed(t, tail));
        g.speaking = true;
        assert!(g.closed(t, tail));
        g = Gate {
            speaking: false,
            ended: Some(t),
        };
        assert!(g.closed(t + Duration::from_millis(599), tail));
        assert!(!g.closed(t + tail, tail));
        assert!(!g.closed(t + Duration::from_secs(5), tail));
    }
}
