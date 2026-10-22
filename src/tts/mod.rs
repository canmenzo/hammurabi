//! Reads final captions aloud with the OS's own voices: WinRT SpeechSynthesizer (Windows),
//! AVSpeechSynthesizer (macOS), spd-say or espeak-ng (Linux; spawned, so the binary does not
//! link libspeechd). Utterances are spoken one at a time on thread "hm-tts".

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(any(windows, target_os = "macos")), path = "unix.rs")]
mod imp;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::settings::lang_name;

/// Utterances waiting behind the one being spoken; older ones are dropped to stay near real time.
const MAX_QUEUED: usize = 2;
/// Captured audio stays muted this long after speech ends (output latency, room echo).
pub const TAIL: Duration = Duration::from_millis(600);
const POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// BCP 47, e.g. "tr-TR" (Linux: often just "tr").
    pub lang: String,
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

/// `preferred` if it speaks `lang`, else the OS default for `lang`, else the first voice that does.
fn pick<'a>(
    voices: &'a [Voice],
    lang: &str,
    preferred: Option<&str>,
    default: Option<&str>,
) -> Option<&'a Voice> {
    let ok = |v: &&Voice| lang_matches(&v.lang, lang);
    let by_id = |id: Option<&str>| voices.iter().filter(ok).find(|v| Some(v.id.as_str()) == id);
    by_id(preferred)
        .or_else(|| by_id(default))
        .or_else(|| voices.iter().find(ok))
}

/// The voice for `lang`. Ok(None) leaves the choice to the OS (no voice list, or language unknown);
/// Err says which voice to install.
pub fn resolve<'a>(
    voices: &'a [Voice],
    lang: &str,
    preferred: Option<&str>,
) -> Result<Option<&'a Voice>, String> {
    if voices.is_empty() || lang.is_empty() {
        return Ok(None);
    }
    pick(voices, lang, preferred, imp::default_voice(lang).as_deref())
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

/// Speaks `text` and returns when it is done or `cancel` is set. `rate`: 1.0 = normal speed.
pub fn speak(
    text: &str,
    lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    imp::speak(text, lang, voice, rate, cancel)
}

pub struct Utterance {
    pub text: String,
    pub lang: String,
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
    voice: Option<String>,
    rate: f32,
    quit: bool,
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
}

impl Speaker {
    pub fn spawn() -> Speaker {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                gate: Gate::default(),
                voice: None,
                rate: 1.0,
                quit: false,
            }),
            wake: Condvar::new(),
            cancel: AtomicBool::new(false),
        });
        let (tx, warnings) = crossbeam_channel::unbounded();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("hm-tts".into())
            .spawn(move || run(&s, &tx))
            .expect("spawn hm-tts");
        Speaker { shared, warnings }
    }

    pub fn configure(&self, voice: Option<String>, rate: f32) {
        let mut st = self.shared.lock();
        st.voice = voice;
        st.rate = rate;
    }

    pub fn say(&self, text: String, lang: String) {
        let dropped = enqueue(&mut self.shared.lock().queue, Utterance { text, lang });
        if dropped > 0 {
            log::debug!("tts behind, dropped {dropped} utterance(s)");
        }
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
    }
}

fn run(shared: &Shared, warnings: &Sender<String>) {
    let mut warned = HashSet::new();
    let mut warn = |text: String| {
        if warned.insert(text.clone()) {
            log::warn!("{text}");
            let _ = warnings.send(text);
        }
    };
    loop {
        let (u, preferred, rate) = {
            let mut st = shared.lock();
            loop {
                if st.quit {
                    return;
                }
                if let Some(u) = st.queue.pop_front() {
                    // Under the lock, so a clear() after the pop still cancels this utterance.
                    shared.cancel.store(false, Ordering::Relaxed);
                    break (u, st.voice.clone(), st.rate);
                }
                st = shared.wake.wait(st).unwrap_or_else(|p| p.into_inner());
            }
        };
        let all = voices();
        let voice = match resolve(&all, &u.lang, preferred.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                warn(e);
                continue;
            }
        };
        shared.lock().gate.speaking = true;
        let r = speak(&u.text, &u.lang, voice, rate, &shared.cancel);
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
        assert_eq!(id(pick(&all, "tr", None, None)), Some("yelda".into()));
        assert_eq!(
            id(pick(&all, "en", None, Some("samantha"))),
            Some("samantha".into())
        );
        assert_eq!(
            id(pick(&all, "en", Some("albert"), Some("samantha"))),
            Some("albert".into())
        );
        // A preferred voice in another language does not read this one.
        assert_eq!(
            id(pick(&all, "en", Some("yelda"), Some("samantha"))),
            Some("samantha".into())
        );
        // An OS default for another language is ignored too.
        assert_eq!(
            id(pick(&all, "en", None, Some("yelda"))),
            Some("albert".into())
        );
        assert_eq!(id(pick(&all, "zh-Hant", None, None)), Some("meijia".into()));
        assert_eq!(id(pick(&all, "zh", None, None)), None);
        assert_eq!(resolve(&[], "tr", None), Ok(None));
        assert_eq!(resolve(&all, "", None), Ok(None));
        assert!(resolve(&all, "ko", None).unwrap_err().contains("Korean"));
    }

    #[test]
    fn queue_drops_oldest() {
        let mut q = VecDeque::new();
        let u = |t: &str| Utterance {
            text: t.into(),
            lang: "tr".into(),
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
