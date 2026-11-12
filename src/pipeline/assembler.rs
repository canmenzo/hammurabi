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
/// Partials that repeat the one before them before the sentence they end in counts as over.
const SETTLED: u32 = 2;

static TERMINATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[.!?…。！？]["'”’)\]]*$"#).unwrap());

fn ends_sentence(w: &str) -> bool {
    TERMINATOR.is_match(w)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Sentence {
        id: u64,
        text: String,
        lang: Option<String>,
    },
    /// Empty text clears the live line.
    Live { text: String, lang: Option<String> },
}

#[derive(Default)]
struct SegText {
    words: Vec<String>,
    language: Option<String>,
    emitted: usize,
    last_emitted: String,
}

#[derive(Default)]
pub struct Assembler {
    segs: HashMap<u64, SegText>,
    prev_partial: Option<(u64, Vec<String>)>,
    /// Partials in a row that said exactly what the one before said.
    same: u32,
    /// Finished audio whose sentence has not ended yet.
    carry: Vec<String>,
    carry_lang: Option<String>,
    flush_at: Option<Instant>,
    last_sentence: Option<(String, Instant)>,
    /// The caption emitted last: id, words, when, language.
    last_caption: Option<(u64, Vec<String>, Instant, Option<String>)>,
    /// Captions are read aloud as they come, so one must not come again with more in it.
    pub separate: bool,
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
        let norm = |w: &str| {
            w.to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
        };
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

    /// Words two consecutive partials agree on are stable; complete sentences inside them are
    /// shown right away instead of waiting for the pause that closes the segment.
    pub fn partial(&mut self, seg: u64, words: Vec<String>, lang: &str, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let prev = match self.prev_partial.take() {
            Some((id, w)) if id == seg => w,
            _ => Vec::new(),
        };
        let stable = words.iter().zip(&prev).take_while(|(a, b)| a == b).count();
        self.same = if prev == words { self.same + 1 } else { 0 };
        let st = self.segs.entry(seg).or_default();
        st.words = words.clone();
        st.language = Some(lang.to_string());
        let emitted = st.emitted;
        // Not the sentence the audio so far ends in, until nothing has been added to it for a
        // while: Whisper closes whatever it has with a full stop ("This is real." before "This
        // is real nice, Dad.").
        let mut start = emitted;
        let ended = if self.same >= SETTLED {
            stable
        } else {
            stable.min(words.len().saturating_sub(1))
        };
        for end in (emitted..ended).filter(|&i| ends_sentence(&words[i])) {
            self.end_carry(&mut out, words.get(start), lang, now);
            let joined = self.join_carry(&words[start..=end]);
            self.emit(&mut out, joined, Some(lang.to_string()), now);
            self.carry.clear();
            start = end + 1;
        }
        if start > emitted {
            let st = self.segs.get_mut(&seg).unwrap();
            st.last_emitted = words[start - 1].clone();
            st.emitted = start;
        }
        self.flush_at = None;
        // A newer partial can be shorter than what earlier ones already showed.
        let emitted = self.segs[&seg].emitted;
        let live = self.join_carry(words.get(emitted..).unwrap_or_default());
        out.push(live_line(&live, Some(lang.to_string())));
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
        let st = self.segs.remove(&seg).unwrap_or_default();
        let (words, lang) = match result {
            Some((w, l)) => (w, Some(l)),
            None if !st.words.is_empty() => (st.words.clone(), st.language.clone()),
            None => return Vec::new(),
        };
        // Skip what partials already emitted; the final may word it slightly differently, so
        // look for the last emitted word near where it should be.
        let mut skip = st.emitted;
        if st.emitted > 0 {
            let lo = st.emitted.saturating_sub(3);
            let near = words.get(lo..words.len().min(st.emitted + 3));
            if let Some(j) = near.and_then(|n| n.iter().rposition(|w| *w == st.last_emitted)) {
                skip = lo + j + 1;
            }
        }
        let rest = &words[skip.min(words.len())..];
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
        out.push(live_line(&self.carry, self.carry_lang.clone()));
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
        out.push(live_line(&[], None));
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

    fn emit(&mut self, out: &mut Vec<Out>, words: Vec<String>, lang: Option<String>, now: Instant) {
        if !self.fresh(&words, now) {
            return;
        }
        let chars = |w: &[String]| w.iter().map(|w| w.chars().count() + 1).sum::<usize>();
        if let Some((id, said, at, l)) = &mut self.last_caption
            && !self.separate
            && now.saturating_duration_since(*at) < JOIN_WITHIN
            // What was cut off by another speaker stays apart from what they said.
            && said.last().is_some_and(|w| ends_sentence(w))
            && *l == lang
            && chars(said) + chars(&words) <= CAPTION_CHARS
        {
            said.extend(words);
            *at = now;
            let grown = Out::Sentence {
                id: *id,
                text: said.join(" "),
                lang,
            };
            // Grown twice in one go: only the last is worth translating.
            match out.last_mut() {
                Some(Out::Sentence { id: last, .. }) if last == id => {
                    *out.last_mut().unwrap() = grown
                }
                _ => out.push(grown),
            }
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

fn live_line(words: &[String], lang: Option<String>) -> Out {
    Out::Live {
        text: words.join(" ").trim().to_string(),
        lang,
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
    fn sentences_that_follow_closely_share_a_caption() {
        let t = Instant::now();
        let mut a = Assembler::default();
        a.finish(1, Some((w("and then"), "en".into())), t);
        a.partial(2, w("we left. Go. Now! What"), "en", t);
        let o = a.partial(2, w("we left. Go. Now! What is"), "en", t);
        assert_eq!(sentences(&o), ["and then we left. Go. Now!"]);
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

        // Said again and again with nothing added, it is over.
        let later = t + Duration::from_secs(10);
        a.partial(2, w("Good times."), "en", later);
        assert!(sentences(&a.partial(2, w("Good times."), "en", later)).is_empty());
        let o = a.partial(2, w("Good times."), "en", later);
        assert_eq!(sentences(&o), ["Good times."]);
        assert_eq!(live(&o), Some(""));
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
