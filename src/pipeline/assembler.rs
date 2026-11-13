//! Port of Babil's partial/finishSegment/flush/emit/live: captions are built per sentence, not per
//! audio segment, so a sentence is translated once, when complete, and never rewrites itself.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;

/// An unfinished sentence is shown anyway after this much silence.
const FLUSH_AFTER: Duration = Duration::from_millis(1500);
/// Words; very long unpunctuated speech is shown in chunks.
const MAX_CARRY: usize = 30;
const DEDUPE_WINDOW: Duration = Duration::from_secs(5);
/// A sentence that follows the last caption this soon joins it, up to this many characters (two
/// subtitle lines): "Hey, Simeon." grows into "Hey, Simeon. I'm out." instead of being pushed
/// off the screen by it before it is read.
const JOIN_WITHIN: Duration = Duration::from_millis(1500);
const CAPTION_CHARS: usize = 84;
/// The sentence the audio so far ends in counts as over once partials have said the same for
/// this long. Less, and more words cut in half by the end of the audio become captions.
const SETTLE: Duration = Duration::from_millis(400);
/// Partials a sentence is looked for in.
const HEARD: usize = 4;
/// This many words in a row that were shown already are Whisper starting over. Fewer are
/// people saying a thing twice.
const RERUN: usize = 8;

static TERMINATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[.!?…。！？]["'”’)\]]*$"#).unwrap());

pub(super) fn ends_sentence(w: &str) -> bool {
    TERMINATOR.is_match(w)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Sentence {
        id: u64,
        text: String,
        lang: Option<String>,
    },
    /// Empty text clears the live line. `whole`: the sentences in it that have ended, which are
    /// captions next if nothing about them changes, so that they can be translated before then.
    Live {
        text: String,
        lang: Option<String>,
        whole: Vec<String>,
    },
}

#[derive(Default)]
struct SegText {
    words: Vec<String>,
    language: Option<String>,
    /// The words of this segment shown as captions so far, as `norm` leaves them.
    emitted: Vec<String>,
    /// The same words as they were shown.
    shown: Vec<String>,
    /// Words the segment before this one showed of this one's audio (`Assembler::finish`), as
    /// compared, until this segment has heard that much itself.
    handed: Vec<String>,
    /// The sentences each of the last `HEARD` partials had, as their words compared.
    heard: std::collections::VecDeque<Vec<String>>,
}

/// A word as it is compared: Whisper writes the same audio as "that." one time and "That" the next.
fn norm(w: &str) -> String {
    w.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect()
}

/// Where in `words` what was not shown yet begins, given the words shown so far: after the place
/// their last three (or two, or the last one near where it should be) are found. Counting words
/// is not enough, since a transcript of more audio may word the start differently: put a dash
/// before it, drop a stutter, or leave the start out. When they are not found, the same few
/// words heard differently hold nothing new (the end), and other speech gives None.
fn resume(words: &[String], emitted: &[String]) -> Option<usize> {
    if emitted.is_empty() {
        return Some(0);
    }
    let said: Vec<String> = words.iter().map(|w| norm(w)).collect();
    for k in (1..=emitted.len().min(3)).rev() {
        let tail = &emitted[emitted.len() - k..];
        let end = (k..=said.len())
            .filter(|&end| said[end - k..end] == *tail)
            .min_by_key(|end| end.abs_diff(emitted.len()));
        if let Some(end) = end.filter(|end| k > 1 || end.abs_diff(emitted.len()) <= 3) {
            return Some(end);
        }
    }
    // The same from the start but for its last words: those were heard another way this time.
    let same = said.iter().zip(emitted).take_while(|(a, b)| a == b).count();
    if same > 0 && same + 3 >= emitted.len() && 2 * same >= emitted.len() {
        return Some(emitted.len().min(said.len()));
    }
    let known = said.iter().filter(|w| emitted.contains(w)).count();
    (said.len() <= emitted.len() + 1 && 2 * known >= said.len()).then_some(said.len())
}

/// Where in `words`, from `start` on, Whisper starts over: thrown by a hard stretch, it goes back
/// and says again what was shown already ("Hey, let's go. Do something about the thing. I ain't
/// talking to you. Hey, let's go do something about the thing.").
fn rerun(words: &[String], start: usize, emitted: &[String]) -> Option<usize> {
    let said: Vec<String> = words[start..].iter().map(|w| norm(w)).collect();
    (0..said.len().saturating_sub(RERUN - 1))
        .find(|&i| emitted.windows(RERUN).any(|w| w == &said[i..i + RERUN]))
        .map(|i| start + i)
}

impl SegText {
    /// Takes what was handed down for shown if `words`, this segment's own, begin with it, and
    /// forgets it if they do not: it may be something Whisper made up at the end of the segment
    /// before, and a run started from words that are not in its audio writes nothing at all.
    /// True while there are too few words to tell (`last`: there will be no more).
    fn take_over(&mut self, words: &[String], last: bool) -> bool {
        if self.handed.is_empty() {
            return false;
        }
        if words.len() <= self.handed.len() {
            if !last {
                return true;
            }
            // No more than was handed down: nothing new if it is the start of that.
            let same = words
                .iter()
                .zip(&self.handed)
                .take_while(|(a, b)| norm(a) == **b)
                .count();
            let handed = std::mem::take(&mut self.handed);
            if same + 1 >= words.len() {
                self.emitted = handed;
            }
            return false;
        }
        let handed = std::mem::take(&mut self.handed);
        if let Some(end) = resume(words, &handed).filter(|end| end.abs_diff(handed.len()) <= 2) {
            self.shown = words[..end].to_vec();
            self.emitted = handed;
        }
        false
    }
}

/// Words that say something: Whisper marks a change of speaker with a dash of its own, which
/// comes and goes between transcripts of the same audio.
fn spoken(words: Vec<String>) -> Vec<String> {
    words
        .into_iter()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect()
}

#[derive(Default)]
pub struct Assembler {
    segs: HashMap<u64, SegText>,
    prev_partial: Option<(u64, Vec<String>)>,
    /// Since when partials have said exactly what the last one said.
    unchanged: Option<Instant>,
    /// Finished audio whose sentence has not ended yet.
    carry: Vec<String>,
    carry_lang: Option<String>,
    flush_at: Option<Instant>,
    last_sentence: Option<(String, Instant)>,
    /// The caption emitted last: id, words, when, language.
    last_caption: Option<(u64, Vec<String>, Instant, Option<String>)>,
    /// Captions are read aloud as they come, so one must not come again with more in it.
    pub separate: bool,
    /// The partial given next is of audio that ends in quiet.
    pub paused: bool,
    /// The final given next is of a segment that was cut, not ended: the next one goes on.
    pub cut: bool,
    next_id: u64,
}

impl Assembler {
    /// Forgets segment state and the unfinished sentence; sentence ids keep counting.
    pub fn reset(&mut self) {
        *self = Self {
            next_id: self.next_id,
            separate: self.separate,
            ..Default::default()
        };
    }

    pub fn flush_deadline(&self) -> Option<Instant> {
        self.flush_at
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        match self.flush_at {
            Some(t) if now >= t => self.flush(now),
            _ => Vec::new(),
        }
    }

    /// A word cut in half at a segment boundary is heard by both segments ("input the" + "The whole").
    fn join_carry(&self, words: &[String]) -> Vec<String> {
        let dup =
            matches!((self.carry.last(), words.first()), (Some(a), Some(b)) if norm(a) == norm(b));
        let mut out = self.carry.clone();
        out.extend_from_slice(&words[dup as usize..]);
        out
    }

    /// The unfinished sentence is over after all when what follows it opens a new one: in quick
    /// back-and-forth, one speaker's line often ends without a full stop as the next one starts.
    /// Whisper capitalizes what it hears as a start. Not German, which capitalizes every noun,
    /// and not "I", which says nothing either way.
    fn end_carry(&mut self, out: &mut Vec<Out>, next: Option<&String>, lang: &str, now: Instant) {
        let (Some(last), Some(next)) = (self.carry.last(), next) else {
            return;
        };
        let word = next.trim_start_matches(|c: char| !c.is_alphanumeric());
        let opens = word.chars().next().is_some_and(char::is_uppercase)
            && word.split("'").next() != Some("I");
        if !opens
            || lang == "de"
            || last.ends_with([',', '-'])
            || self.join_carry(std::slice::from_ref(next)).len() == self.carry.len()
        {
            return;
        }
        let carry = std::mem::take(&mut self.carry);
        let lang = self.carry_lang.take();
        self.emit(out, carry, lang, now);
    }

    /// What captions show of segment `seg` so far: its next run starts from there. Not from the
    /// words after that which runs agree on, too: held to those, Whisper says more things twice.
    pub fn shown(&self, seg: u64) -> String {
        self.segs
            .get(&seg)
            .map_or_else(String::new, |s| s.shown.join(" "))
    }

    /// Transcript of a segment still open. Complete sentences that runs agree on are shown right
    /// away instead of waiting for the pause that closes the segment.
    pub fn partial(&mut self, seg: u64, words: Vec<String>, lang: &str, now: Instant) -> Vec<Out> {
        let mut words = spoken(words);
        let waiting = self
            .segs
            .get_mut(&seg)
            .is_some_and(|st| st.take_over(&words, false));
        if let Some(st) = self.segs.get(&seg)
            && let Some(start) = resume(&words, &st.emitted)
            && let Some(at) = rerun(&words, start, &st.emitted)
        {
            words.truncate(at);
        }
        let mut out = Vec::new();
        let same = self
            .prev_partial
            .take()
            .is_some_and(|p| p == (seg, words.clone()));
        let since = *self.unchanged.get_or_insert(now);
        if !same {
            self.unchanged = Some(now);
        }
        // Quiet after it is as good as time passing.
        let paused = std::mem::take(&mut self.paused);
        let settled = paused || (same && now.saturating_duration_since(since) >= SETTLE);
        let st = self.segs.entry(seg).or_default();
        st.words = words.clone();
        st.language = Some(lang.to_string());
        // Each sentence in this run, by the index of its last word and its words as compared.
        let mut sentences = Vec::new();
        let mut first = 0;
        for (i, w) in words.iter().enumerate() {
            if ends_sentence(w) {
                let key: Vec<String> = words[first..=i].iter().map(|w| norm(w)).collect();
                sentences.push((i, key.join(" ")));
                first = i + 1;
            }
        }
        st.heard
            .push_back(sentences.iter().map(|(_, k)| k.clone()).collect());
        if st.heard.len() > HEARD {
            st.heard.pop_front();
        }
        // A partial that is other speech than what was shown (it heard only the end of the audio)
        // is not taken for new yet: the next one, or the final, will have it all.
        let emitted = resume(&words, &st.emitted).filter(|_| !waiting);
        let mut start = emitted.unwrap_or(words.len());
        // A sentence two of the last few runs heard word for word is over, and so is all before
        // it, however differently each run words that: Whisper keeps rewording the start of long
        // audio ("homebolly yo" / "homeboy, yo." / "homeboley-o") without settling, and what
        // follows would wait on it for seconds. Not the sentence the audio so far ends in, until
        // nothing has been added for a while: Whisper closes whatever it has with a full stop
        // ("This is real." before "This is real nice, Dad.").
        let last = words.len().saturating_sub(1);
        let upto = sentences
            .iter()
            .filter(|(end, key)| {
                *end >= start
                    && (*end < last || settled)
                    && st.heard.iter().filter(|h| h.contains(key)).count() >= 2
            })
            .map(|(end, _)| *end)
            .next_back();
        let from = start;
        for &(end, _) in sentences
            .iter()
            .filter(|(end, _)| *end >= from && Some(*end) <= upto)
        {
            self.end_carry(&mut out, words.get(start), lang, now);
            let joined = self.join_carry(&words[start..=end]);
            self.emit(&mut out, joined, Some(lang.to_string()), now);
            self.carry.clear();
            let st = self.segs.get_mut(&seg).unwrap();
            st.emitted
                .extend(words[start..=end].iter().map(|w| norm(w)));
            st.shown = words[..=end].to_vec();
            start = end + 1;
        }
        self.flush_at = None;
        let live = match emitted {
            Some(_) => self.join_carry(&words[start..]),
            None => words.clone(),
        };
        out.push(self.live_line(&live, Some(lang.to_string())));
        self.prev_partial = Some((seg, words));
        out
    }

    /// Final transcript of a segment. `None` = the segment gets no final (dropped); its last
    /// partial text is used if it had one.
    pub fn finish(
        &mut self,
        seg: u64,
        result: Option<(Vec<String>, String)>,
        now: Instant,
    ) -> Vec<Out> {
        if self.prev_partial.as_ref().is_some_and(|(id, _)| *id == seg) {
            self.prev_partial = None;
        }
        let mut st = self.segs.remove(&seg).unwrap_or_default();
        let result_given = result.is_some();
        let cut = std::mem::take(&mut self.cut) && result_given;
        let (words, lang) = match result {
            Some((w, l)) => (spoken(w), Some(l)),
            None if !st.words.is_empty() => (st.words.clone(), st.language.clone()),
            None => return Vec::new(),
        };
        if result_given {
            st.take_over(&words, true);
        }
        // A segment is cut at a quiet moment a little before its end, and its partials heard the
        // audio after that too. What they showed of it is how the next segment begins, which
        // does not show it again once it has heard it too (`SegText::take_over`).
        let said: Vec<String> = words.iter().map(|w| norm(w)).collect();
        if cut && let Some(end) = resume(&st.shown, &said).filter(|end| *end < st.shown.len()) {
            self.segs.entry(seg + 1).or_default().handed =
                st.shown[end..].iter().map(|w| norm(w)).collect();
        }
        // Skip what partials already showed; a final that is other speech altogether is all new.
        let skip = resume(&words, &st.emitted).unwrap_or(0);
        let end = rerun(&words, skip, &st.emitted).unwrap_or(words.len());
        let rest = &words[skip..end];
        let mut out = Vec::new();
        self.end_carry(
            &mut out,
            rest.first(),
            lang.as_deref().unwrap_or_default(),
            now,
        );
        let all = self.join_carry(rest);
        let mut start = 0;
        for i in 0..all.len() {
            if ends_sentence(&all[i]) {
                self.emit(&mut out, all[start..=i].to_vec(), lang.clone(), now);
                start = i + 1;
            }
        }
        self.carry = all[start..].to_vec();
        if lang.is_some() {
            self.carry_lang = lang;
        }
        if self.carry.len() > MAX_CARRY {
            out.extend(self.flush(now));
            return out;
        }
        out.push(self.live_line(&self.carry, self.carry_lang.clone()));
        self.flush_at = (!self.carry.is_empty()).then(|| now + FLUSH_AFTER);
        out
    }

    pub fn flush(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        self.flush_at = None;
        let carry = std::mem::take(&mut self.carry);
        let lang = self.carry_lang.take();
        if !carry.is_empty() {
            self.emit(&mut out, carry, lang, now);
        }
        out.push(self.live_line(&[], None));
        out
    }

    /// Sentences and live text already split by another recognizer (Windows Live Captions).
    pub fn external(
        &mut self,
        sentences: Vec<String>,
        tail: Option<String>,
        lang: &str,
        now: Instant,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        for s in sentences {
            let words = s.split_whitespace().map(String::from).collect();
            self.emit(&mut out, words, Some(lang.to_string()), now);
        }
        if let Some(t) = tail {
            out.push(Out::Live {
                text: t,
                lang: Some(lang.to_string()),
                whole: Vec::new(),
            });
        }
        out
    }

    /// False for "Please. Please." or a sentence the final repeats: one caption is enough.
    fn fresh(&mut self, words: &[String], now: Instant) -> bool {
        let lower = words.join(" ").trim().to_lowercase();
        if lower.is_empty() {
            return false;
        }
        if let Some((prev, t)) = &self.last_sentence
            && *prev == lower
            && now - *t < DEDUPE_WINDOW
        {
            return false;
        }
        self.last_sentence = Some((lower, now));
        true
    }

    /// Whether `words` shown now would join the last caption instead of starting one.
    fn joins(&self, words: &[String], lang: &Option<String>, now: Instant) -> bool {
        let chars = |w: &[String]| w.iter().map(|w| w.chars().count() + 1).sum::<usize>();
        self.last_caption.as_ref().is_some_and(|(_, said, at, l)| {
            !self.separate
                && now.saturating_duration_since(*at) < JOIN_WITHIN
                // What was cut off by another speaker stays apart from what they said.
                && said.last().is_some_and(|w| ends_sentence(w))
                && l == lang
                && chars(said) + chars(words) <= CAPTION_CHARS
        })
    }

    fn live_line(&self, words: &[String], lang: Option<String>) -> Out {
        let mut whole = Vec::new();
        let mut start = 0;
        for (i, w) in words.iter().enumerate() {
            if ends_sentence(w) {
                whole.push(words[start..=i].join(" "));
                start = i + 1;
            }
        }
        Out::Live {
            text: words.join(" ").trim().to_string(),
            lang,
            whole,
        }
    }

    /// Whether `words` are the sentence shown last, heard to its end: Whisper closes whatever
    /// the audio ends in with a full stop, and when that stood long enough to be shown, goes
    /// back and says the whole of it ("Trevor is a liability." and then "Trevor is a liability
    /// that none of us can afford."). Where in the last caption that sentence begins.
    fn said_whole(&self, words: &[String], lang: &Option<String>, now: Instant) -> Option<usize> {
        let (_, said, at, l) = self.last_caption.as_ref()?;
        if self.separate || l != lang || now.saturating_duration_since(*at) >= DEDUPE_WINDOW {
            return None;
        }
        let key = |w: &[String]| -> Vec<String> {
            w.iter().map(|w| norm(w)).filter(|w| !w.is_empty()).collect()
        };
        let from = said[..said.len().saturating_sub(1)]
            .iter()
            .rposition(|w| ends_sentence(w))
            .map_or(0, |i| i + 1);
        let (short, whole) = (key(&said[from..]), key(words));
        (short.len() >= 2 && whole.len() > short.len() && whole.starts_with(&short))
            .then_some(from)
    }

    fn emit(&mut self, out: &mut Vec<Out>, words: Vec<String>, lang: Option<String>, now: Instant) {
        if !self.fresh(&words, now) {
            return;
        }
        if let Some(from) = self.said_whole(&words, &lang, now)
            && let Some((id, said, at, _)) = &mut self.last_caption
        {
            said.truncate(from);
            said.extend(words);
            *at = now;
            out.push(Out::Sentence {
                id: *id,
                text: said.join(" "),
                lang,
            });
            return;
        }
        if self.joins(&words, &lang, now)
            && let Some((id, said, at, _)) = &mut self.last_caption
        {
            said.extend(words);
            *at = now;
            // Once for each sentence added: each is translated by itself.
            out.push(Out::Sentence {
                id: *id,
                text: said.join(" "),
                lang,
            });
            return;
        }
        self.next_id += 1;
        out.push(Out::Sentence {
            id: self.next_id,
            text: words.join(" ").trim().to_string(),
            lang: lang.clone(),
        });
        self.last_caption = Some((self.next_id, words, now, lang));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_shorter_than_what_was_shown() {
        let words = |t: &str| t.split_whitespace().map(String::from).collect::<Vec<_>>();
        let now = Instant::now();
        let mut a = Assembler::default();
        let long = "One two three four five. Six";
        a.partial(1, words(long), "en", now);
        let shown = a.partial(1, words(long), "en", now);
        assert!(matches!(&shown[0], Out::Sentence { text, .. } if text.ends_with("five.")));
        let out = a.partial(1, words("One two three"), "en", now);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Out::Live { text, .. } if text.is_empty()));
    }

    fn w(s: &str) -> Vec<String> {
        s.split(' ')
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect()
    }

    fn sentences(out: &[Out]) -> Vec<&str> {
        out.iter()
            .filter_map(|o| match o {
                Out::Sentence { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn live(out: &[Out]) -> Option<&str> {
        out.iter().rev().find_map(|o| match o {
            Out::Live { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    #[test]
    fn partials_emit_stable_sentences() {
        let t = Instant::now();
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        let o = a.partial(1, w("Hello there. How"), "en", t);
        assert!(sentences(&o).is_empty(), "nothing stable yet");
        assert_eq!(live(&o), Some("Hello there. How"));

        let o = a.partial(1, w("Hello there. How are"), "en", t);
        assert_eq!(sentences(&o), ["Hello there."]);
        assert_eq!(live(&o), Some("How are"));

        // The final rewords slightly; the emitted part is skipped by locating "there.".
        let o = a.finish(1, Some((w("Hello there. How are you?"), "en".into())), t);
        assert_eq!(sentences(&o), ["How are you?"]);
        assert_eq!(live(&o), Some(""));
        assert_eq!(a.flush_deadline(), None);
    }

    #[test]
    fn what_was_shown_is_not_shown_again_when_its_end_is_heard_another_way() {
        let n = |s: &str| -> Vec<String> { s.split(' ').map(norm).collect() };
        let shown = n("man fuck you i see you at work better yet maybe");
        let words = w("Man, fuck you. I see you at work. Better yet, maybe...Tanisha will call.");
        // "maybe" is not there as a word of its own any more.
        assert_eq!(resume(&words, &shown), Some(11));
        // Other speech stays other speech.
        assert_eq!(resume(&w("Look at this guy over there."), &shown), None);
    }

    #[test]
    fn what_was_shown_past_a_cut_is_not_shown_again() {
        let t = Instant::now();
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        a.partial(1, w("One two three. Four five six. Seven"), "en", t);
        let o = a.partial(1, w("One two three. Four five six. Seven eight"), "en", t);
        assert_eq!(sentences(&o), ["One two three.", "Four five six."]);
        // The segment is cut after "Four": its final has less than was shown.
        a.cut = true;
        let o = a.finish(1, Some((w("One two three. Four"), "en".into())), t);
        assert!(sentences(&o).is_empty());
        // The next one begins with the rest, and starts from it once it has heard it too.
        assert_eq!(a.shown(2), "");
        assert!(sentences(&a.partial(2, w("five"), "en", t)).is_empty());
        assert_eq!(a.shown(2), "");
        a.partial(2, w("five six. Seven eight nine. Ten"), "en", t);
        assert_eq!(a.shown(2), "five six.");
        let o = a.partial(2, w("five six. Seven eight nine. Ten eleven"), "en", t);
        assert_eq!(sentences(&o), ["Seven eight nine."]);

        // What the segment before made up is not held against this one: Whisper said its last
        // sentence twice, and the second was taken for the start of the next segment.
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        a.partial(
            1,
            w("I say left, you say right. I say left, you say right. I"),
            "en",
            t,
        );
        a.partial(
            1,
            w("I say left, you say right. I say left, you say right. I say"),
            "en",
            t,
        );
        a.cut = true;
        a.finish(1, Some((w("I say left, you say right."), "en".into())), t);
        a.partial(
            2,
            w("I say become a doctor. You say become a patient. I"),
            "en",
            t,
        );
        assert_eq!(a.shown(2), "");
        let o = a.partial(
            2,
            w("I say become a doctor. You say become a patient. I say"),
            "en",
            t,
        );
        assert_eq!(
            sentences(&o),
            ["I say become a doctor.", "You say become a patient."]
        );

        // A segment that ended leaves nothing to the next.
        let mut a = Assembler::default();
        a.partial(1, w("One two three. Four"), "en", t);
        a.partial(1, w("One two three. Four five"), "en", t);
        a.finish(1, Some((w("One two"), "en".into())), t);
        assert_eq!(a.shown(2), "");
    }

    #[test]
    fn what_whisper_says_again_is_left_out() {
        let shown: Vec<String> = "i'm the king of things i ain't talking to you hey let's go"
            .split(' ')
            .map(norm)
            .collect();
        let words = w(
            "I'm the king of things. I ain't talking to you. Hey, let's go. Do \
            something about it. I ain't talking to you. Hey, let's go do it.",
        );
        let start = resume(&words, &shown).unwrap();
        assert_eq!(start, 13);
        assert_eq!(rerun(&words, start, &shown), Some(17));
        // A few words said again are just that.
        let words = w(
            "I'm the king of things. I ain't talking to you. Hey, let's go. I ain't \
            talking to you, man.",
        );
        assert_eq!(rerun(&words, 13, &shown), None);

        let t = Instant::now();
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        a.partial(1, w("I ain't talking to you right now, okay? Hey"), "en", t);
        a.partial(
            1,
            w("I ain't talking to you right now, okay? Hey man"),
            "en",
            t,
        );
        let o = a.finish(
            1,
            Some((
                w(
                    "I ain't talking to you right now, okay? Hey man. I ain't talking to you \
                    right now, okay? Hey man.",
                ),
                "en".into(),
            )),
            t,
        );
        assert_eq!(sentences(&o), ["Hey man."]);
    }

    #[test]
    fn quiet_after_a_sentence_ends_it_without_the_wait() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.partial(1, w("Good times."), "en", t);
        // Heard again at once, it may yet go on.
        assert!(sentences(&a.partial(1, w("Good times."), "en", t)).is_empty());
        a.paused = true;
        let o = a.partial(1, w("Good times."), "en", t);
        assert_eq!(sentences(&o), ["Good times."]);
        // The next run of the segment starts from what is shown.
        assert_eq!(a.shown(1), "Good times.");
        assert_eq!(a.shown(2), "");

        // Quiet is not enough for what was heard only once.
        let mut a = Assembler {
            paused: true,
            ..Default::default()
        };
        assert!(sentences(&a.partial(1, w("Good times."), "en", t)).is_empty());
    }

    #[test]
    fn the_live_line_says_which_of_its_sentences_are_whole() {
        let t = Instant::now();
        let mut a = Assembler::default();
        let o = a.partial(1, w("Hey. How are you? I was"), "en", t);
        assert!(
            matches!(o.last(), Some(Out::Live { whole, .. }) if whole == &["Hey.", "How are you?"])
        );
    }

    #[test]
    fn sentences_that_follow_closely_share_a_caption() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.finish(1, Some((w("and then"), "en".into())), t);
        a.partial(2, w("we left. Go. Now! What"), "en", t);
        let o = a.partial(2, w("we left. Go. Now! What is"), "en", t);
        // Once for each sentence, growing.
        assert_eq!(
            sentences(&o),
            [
                "and then we left.",
                "and then we left. Go.",
                "and then we left. Go. Now!"
            ]
        );
        assert_eq!(live(&o), Some("What is"));
        // One that follows soon joins them, under the same id.
        let soon = t + Duration::from_secs(1);
        let o = a.finish(
            2,
            Some((w("we left. Go. Now! What is it?"), "en".into())),
            soon,
        );
        assert_eq!(sentences(&o), ["and then we left. Go. Now! What is it?"]);
        assert!(matches!(&o[0], Out::Sentence { id: 1, .. }));
        // Not one that would make the caption too long, nor one that comes later.
        let long = "This one sentence is long enough to fill a line of its own, word by word.";
        let o = a.finish(3, Some((w(long), "en".into())), soon);
        assert!(matches!(&o[0], Out::Sentence { id: 2, .. }));
        let o = a.finish(4, Some((w("Ok."), "en".into())), soon);
        assert_eq!(sentences(&o), [format!("{long} Ok.")]);
        let o = a.finish(5, Some((w("Right."), "en".into())), soon + JOIN_WITHIN);
        assert_eq!(sentences(&o), ["Right."]);
        // Nor when captions are read aloud.
        a.separate = true;
        let o = a.finish(6, Some((w("Fine."), "en".into())), soon + JOIN_WITHIN);
        assert_eq!(sentences(&o), ["Fine."]);
    }

    #[test]
    fn a_sentence_heard_to_its_end_takes_the_place_of_its_start() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.finish(1, Some((w("He left. He is a risk."), "en".into())), t);
        // The same words and more: the caption is put right, under its id.
        let o = a.finish(2, Some((w("He is a risk we cannot take."), "en".into())), t);
        assert_eq!(sentences(&o), ["He left. He is a risk we cannot take."]);
        assert!(matches!(&o[0], Out::Sentence { id: 1, .. }));
        // Not one word said again, nor other words, nor a caption read aloud already.
        let o = a.finish(3, Some((w("No."), "en".into())), t + JOIN_WITHIN);
        let o2 = a.finish(4, Some((w("No way, man."), "en".into())), t + JOIN_WITHIN);
        assert_eq!(sentences(&o), ["No."]);
        assert_eq!(sentences(&o2), ["No. No way, man."]);
        a.separate = true;
        a.finish(5, Some((w("Get out."), "en".into())), t + 3 * JOIN_WITHIN);
        let o = a.finish(6, Some((w("Get out of here."), "en".into())), t + 3 * JOIN_WITHIN);
        assert_eq!(sentences(&o), ["Get out of here."]);
        assert!(matches!(&o[0], Out::Sentence { id: 4, .. }));
    }

    #[test]
    fn the_sentence_the_audio_ends_in_waits() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.partial(1, w("You know, this is real."), "en", t);
        let o = a.partial(1, w("You know, this is real."), "en", t);
        assert!(sentences(&o).is_empty());
        assert_eq!(live(&o), Some("You know, this is real."));
        let o = a.finish(
            1,
            Some((w("You know, this is real nice, Dad."), "en".into())),
            t,
        );
        assert_eq!(sentences(&o), ["You know, this is real nice, Dad."]);

        // Said again and again with nothing added for a while, it is over.
        let later = t + Duration::from_secs(10);
        a.partial(2, w("Good times."), "en", later);
        let soon = later + Duration::from_millis(200);
        assert!(sentences(&a.partial(2, w("Good times."), "en", soon)).is_empty());
        let o = a.partial(2, w("Good times."), "en", later + SETTLE);
        assert_eq!(sentences(&o), ["Good times."]);
        assert_eq!(live(&o), Some(""));
    }

    #[test]
    fn what_was_shown_is_found_again_however_the_start_is_worded() {
        let t = Instant::now();
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        let shown = "N-word. That's not cool, man. I don't see that.";
        a.partial(1, w(&format!("{shown} You're")), "en", t);
        let o = a.partial(1, w(&format!("{shown} You're right")), "en", t);
        assert_eq!(sentences(&o).len(), 3);
        // A dash before it, another before the next speaker, and a different mark after a word.
        let dashed = "- N-word! That's not cool, man. I don't see that. - You're right and";
        a.partial(1, w(dashed), "en", t);
        let o = a.partial(1, w(dashed), "en", t);
        assert!(sentences(&o).is_empty());
        assert_eq!(live(&o), Some("You're right and"));
        // Only the end of the audio, heard on its own: nothing of it is taken for new yet.
        a.partial(1, w("You're right. And you"), "en", t);
        let o = a.partial(1, w("You're right. And you"), "en", t);
        assert!(sentences(&o).is_empty());
        // The final has it all.
        let o = a.finish(
            1,
            Some((
                w(&format!("{shown} You're right, and you know it.")),
                "en".into(),
            )),
            t,
        );
        assert_eq!(sentences(&o), ["You're right, and you know it."]);

        // A final that words the little that was shown differently repeats nothing; one that
        // is other speech altogether is all new.
        for (last, new) in [
            ("I don't say that.", vec![]),
            ("Get in the car now.", vec!["Get in the car now."]),
        ] {
            let mut a = Assembler {
                separate: true,
                ..Default::default()
            };
            a.partial(1, w("I don't see that. So"), "en", t);
            a.partial(1, w("I don't see that. So"), "en", t);
            let o = a.finish(1, Some((w(last), "en".into())), t);
            assert_eq!(sentences(&o), new);
        }
    }

    #[test]
    fn a_sentence_heard_twice_takes_what_came_before_with_it() {
        let t = Instant::now();
        let mut a = Assembler {
            separate: true,
            ..Default::default()
        };
        // The first sentence is worded differently every time; the second is not.
        let o = a.partial(
            1,
            w("Like you know it homebolly yo. You ain't been there. Hang"),
            "en",
            t,
        );
        assert!(sentences(&o).is_empty());
        let o = a.partial(
            1,
            w("Like you know it home ball. Yo, you ain't been there hanging"),
            "en",
            t,
        );
        assert!(sentences(&o).is_empty());
        let o = a.partial(
            1,
            w("Like you know it, homeboy-o. You ain't been there. Hanging out"),
            "en",
            t,
        );
        assert_eq!(
            sentences(&o),
            ["Like you know it, homeboy-o.", "You ain't been there."]
        );
        assert_eq!(live(&o), Some("Hanging out"));
        // Too long ago does not count.
        for text in [
            "One. Two",
            "Three four",
            "Three four five",
            "Three four five six",
            "Seven",
        ] {
            assert!(sentences(&a.partial(2, w(text), "en", t)).is_empty());
        }
        assert!(sentences(&a.partial(2, w("One. Two three"), "en", t)).is_empty());
    }

    #[test]
    fn a_new_speaker_ends_the_unfinished_sentence() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.finish(1, Some((w("Do something for a change"), "en".into())), t);
        let o = a.finish(2, Some((w("Man, you know what?"), "en".into())), t);
        assert_eq!(
            sentences(&o),
            ["Do something for a change", "Man, you know what?"]
        );
        // Carried on: lower case, "I", a comma before, the boundary word heard twice, German.
        for (lang, first, second, joined) in [
            ("en", "we went to", "the store.", "we went to the store."),
            ("en", "and then", "I left.", "and then I left."),
            ("en", "Well,", "Lamar did it.", "Well, Lamar did it."),
            (
                "en",
                "we put the",
                "The whole thing.",
                "we put the whole thing.",
            ),
            (
                "de",
                "ich habe",
                "Hunger gehabt.",
                "ich habe Hunger gehabt.",
            ),
        ] {
            let mut a = Assembler::default();
            a.finish(1, Some((w(first), lang.into())), t);
            let o = a.finish(2, Some((w(second), lang.into())), t);
            assert_eq!(sentences(&o), [joined]);
        }
        // From partials too, once the start of the next sentence is stable.
        let mut a = Assembler::default();
        a.finish(1, Some((w("I bet you know some shit"), "en".into())), t);
        a.partial(2, w("You wouldn't believe it. Yeah"), "en", t);
        let o = a.partial(2, w("You wouldn't believe it. Yeah,"), "en", t);
        assert_eq!(
            sentences(&o),
            ["I bet you know some shit", "You wouldn't believe it."]
        );
    }

    #[test]
    fn partials_of_other_segment_do_not_agree() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.partial(1, w("Yes. No"), "en", t);
        let o = a.partial(2, w("Yes. No"), "en", t);
        assert!(sentences(&o).is_empty());
    }

    #[test]
    fn boundary_word_dedupe_and_flush() {
        let t = Instant::now();
        let mut a = Assembler::default();
        let o = a.finish(1, Some((w("we put the input the"), "en".into())), t);
        assert!(sentences(&o).is_empty());
        assert_eq!(live(&o), Some("we put the input the"));
        assert_eq!(a.flush_deadline(), Some(t + FLUSH_AFTER));

        let o = a.finish(2, Some((w("The whole thing. And then"), "en".into())), t);
        assert_eq!(sentences(&o), ["we put the input the whole thing."]);
        assert_eq!(live(&o), Some("And then"));

        assert!(a.tick(t + Duration::from_millis(1400)).is_empty());
        let o = a.tick(t + FLUSH_AFTER);
        assert_eq!(sentences(&o), ["And then"]);
        assert_eq!(live(&o), Some(""));
        assert_eq!(a.flush_deadline(), None);
    }

    #[test]
    fn long_carry_flushes() {
        let t = Instant::now();
        let mut a = Assembler::default();
        let long = vec!["word".to_string(); 31];
        let o = a.finish(1, Some((long, "en".into())), t);
        assert_eq!(sentences(&o).len(), 1);
        assert_eq!(live(&o), Some(""));
    }

    #[test]
    fn dedupe_within_5s() {
        let t = Instant::now();
        let mut a = Assembler::default();
        let o = a.finish(1, Some((w("Please. Please."), "en".into())), t);
        assert_eq!(sentences(&o), ["Please."]);
        let o = a.finish(
            2,
            Some((w("please."), "en".into())),
            t + Duration::from_secs(4),
        );
        assert!(sentences(&o).is_empty());
        let o = a.finish(
            3,
            Some((w("Please."), "en".into())),
            t + Duration::from_secs(10),
        );
        assert_eq!(sentences(&o), ["Please."]);
    }

    #[test]
    fn dropped_segment_uses_last_partial() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.partial(1, w("Good morning everyone."), "en", t);
        let o = a.finish(1, None, t);
        assert_eq!(sentences(&o), ["Good morning everyone."]);
        assert!(a.finish(2, None, t).is_empty());
    }

    #[test]
    fn partial_cancels_flush() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.finish(1, Some((w("so then"), "en".into())), t);
        let o = a.partial(2, w("then we"), "en", t);
        // "then" duplicates the carried boundary word.
        assert_eq!(live(&o), Some("so then we"));
        assert_eq!(a.flush_deadline(), None);
    }
}
