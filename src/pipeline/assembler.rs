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
    /// Finished audio whose sentence has not ended yet.
    carry: Vec<String>,
    carry_lang: Option<String>,
    flush_at: Option<Instant>,
    last_sentence: Option<(String, Instant)>,
    next_id: u64,
}

impl Assembler {
    /// Forgets segment state and the unfinished sentence; sentence ids keep counting.
    pub fn reset(&mut self) {
        *self = Self {
            next_id: self.next_id,
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

    /// Words two consecutive partials agree on are stable; complete sentences inside them are
    /// shown right away instead of waiting for the pause that closes the segment.
    pub fn partial(&mut self, seg: u64, words: Vec<String>, lang: &str, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let prev = match self.prev_partial.take() {
            Some((id, w)) if id == seg => w,
            _ => Vec::new(),
        };
        let stable = words.iter().zip(&prev).take_while(|(a, b)| a == b).count();
        let st = self.segs.entry(seg).or_default();
        st.words = words.clone();
        st.language = Some(lang.to_string());
        let emitted = st.emitted;
        let end = (emitted..stable).rev().find(|&i| ends_sentence(&words[i]));
        if let Some(end) = end {
            let joined = self.join_carry(&words[emitted..=end]);
            self.emit(&mut out, joined, Some(lang.to_string()), now);
            self.carry.clear();
            let st = self.segs.get_mut(&seg).unwrap();
            st.last_emitted = words[end].clone();
            st.emitted = end + 1;
        }
        self.flush_at = None;
        let emitted = self.segs[&seg].emitted;
        let live = self.join_carry(&words[emitted..]);
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
        let all = self.join_carry(&words[skip.min(words.len())..]);
        let mut out = Vec::new();
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

    fn emit(&mut self, out: &mut Vec<Out>, words: Vec<String>, lang: Option<String>, now: Instant) {
        let text = words.join(" ").trim().to_string();
        if text.is_empty() {
            return;
        }
        // "Please. Please." or a sentence the final repeats: one caption is enough.
        let lower = text.to_lowercase();
        if let Some((prev, t)) = &self.last_sentence
            && *prev == lower
            && now - *t < DEDUPE_WINDOW
        {
            return;
        }
        self.last_sentence = Some((lower, now));
        self.next_id += 1;
        out.push(Out::Sentence {
            id: self.next_id,
            text,
            lang,
        });
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
        let mut a = Assembler::default();
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
