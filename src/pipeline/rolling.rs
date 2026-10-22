//! Turns a rolling caption window (Windows Live Captions) into completed sentences and a live tail.
//! The window keeps rewriting its last line, punctuates late and drops old text off the front.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A tail that stops changing this long is shown as a sentence anyway (the speaker paused).
const STALE_AFTER: Duration = Duration::from_millis(1500);
const RECENT: usize = 32;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub sentences: Vec<String>,
    /// Some when the live tail changed; empty clears it.
    pub tail: Option<String>,
}

#[derive(Default)]
pub struct RollingText {
    seeded: bool,
    /// Normalized words of recently emitted sentences, newest last.
    recent: VecDeque<Vec<String>>,
    /// Raw tail as last seen in the window, and when it last changed.
    raw_tail: String,
    tail_since: Option<Instant>,
    /// Tail as last reported (after removing what a stale flush already emitted).
    shown: String,
    /// Normalized words a stale flush emitted before the window finished that sentence.
    flushed: Vec<String>,
}

fn norm_word(w: &str) -> String {
    w.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn norm(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(norm_word)
        .filter(|w| !w.is_empty())
        .collect()
}

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？')
}

fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | '”' | '’' | ')' | ']')
}

/// Sentences in order; the last one may be unfinished.
fn split(text: &str) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        if is_terminator(chars[i]) {
            let mut end = i + 1;
            while end < chars.len() && (is_terminator(chars[end]) || is_closer(chars[end])) {
                end += 1;
            }
            // "3.5" and "e.g." mid-word are not sentence ends; CJK terminators need no space.
            let wide = matches!(chars[i], '。' | '！' | '？');
            if end == chars.len() || chars[end] == ' ' || wide {
                let s: String = chars[start..end].iter().collect();
                if !s.trim().is_empty() {
                    out.push(s.trim().to_string());
                }
                start = end;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    let rest: String = chars[start..].iter().collect();
    if !rest.trim().is_empty() {
        out.push(rest.trim().to_string());
    }
    out
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end_matches(is_closer).ends_with(is_terminator)
}

/// Same sentence, allowing the small rewording Live Captions does after the fact.
fn similar(a: &[String], b: &[String]) -> bool {
    if a == b {
        return true;
    }
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let common = a.iter().filter(|w| b.contains(w)).count();
    common * 10 >= a.len().max(b.len()) * 8
}

impl RollingText {
    pub fn update(&mut self, text: &str, now: Instant) -> Step {
        let mut pieces = split(text);
        let tail = match pieces.last() {
            Some(_) => pieces.pop().unwrap(),
            None => String::new(),
        };
        let mut step = Step::default();
        if !self.seeded {
            // Whatever the window already shows was said before we started.
            self.seeded = true;
            for p in pieces.iter().chain(std::iter::once(&tail)) {
                self.remember(norm(p));
            }
            self.raw_tail = tail;
            self.tail_since = None;
            return step;
        }

        let known = |r: &Self, n: &[String]| r.recent.iter().any(|x| similar(x, n));
        let anchor = pieces.iter().rposition(|p| known(self, &norm(p)));
        let fresh = match anchor {
            Some(j) => j + 1,
            // The oldest piece may be the cut-off end of a sentence that scrolled out.
            None => {
                let first = pieces.first().map(|p| norm(p)).unwrap_or_default();
                let cut = !first.is_empty() && self.recent.iter().any(|r| r.ends_with(&first));
                cut as usize
            }
        };
        for p in &pieces[fresh.min(pieces.len())..] {
            let n = norm(p);
            if n.is_empty() || known(self, &n) {
                continue;
            }
            let s = self.strip_flushed(p);
            self.flushed.clear();
            self.remember(n);
            if !s.is_empty() {
                step.sentences.push(s);
            }
        }

        if tail != self.raw_tail {
            self.raw_tail = tail.clone();
            self.tail_since = Some(now);
        }
        let n = norm(&tail);
        let shown = if n.is_empty() || (ends_sentence(&tail) && known(self, &n)) {
            String::new()
        } else {
            self.strip_flushed(&tail)
        };
        if shown != self.shown {
            self.shown = shown.clone();
            step.tail = Some(shown);
        }
        step
    }

    /// Takes everything the window shows as already said, without emitting any of it.
    pub fn reseed(&mut self, text: &str, now: Instant) {
        self.seeded = false;
        self.update(text, now);
    }

    /// Emits the tail once it has not changed for a while.
    pub fn tick(&mut self, now: Instant) -> Step {
        let mut step = Step::default();
        let stale = self
            .tail_since
            .is_some_and(|t| now.saturating_duration_since(t) >= STALE_AFTER);
        if !stale {
            return step;
        }
        self.tail_since = None;
        if self.shown.is_empty() {
            return step;
        }
        let n = norm(&self.raw_tail);
        step.sentences.push(std::mem::take(&mut self.shown));
        step.tail = Some(String::new());
        // A finished sentence is remembered whole; an unfinished one is stripped from what follows.
        if ends_sentence(&self.raw_tail) {
            self.flushed.clear();
            self.remember(n);
        } else {
            self.flushed = n;
        }
        step
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.tail_since.map(|t| t + STALE_AFTER)
    }

    fn remember(&mut self, n: Vec<String>) {
        if n.is_empty() {
            return;
        }
        self.recent.push_back(n);
        if self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    /// Drops the words a stale flush already showed from the front of `s`.
    fn strip_flushed(&self, s: &str) -> String {
        if self.flushed.is_empty() {
            return s.to_string();
        }
        let words: Vec<&str> = s.split_whitespace().collect();
        let mut fi = 0;
        let mut cut = 0;
        for (i, w) in words.iter().enumerate() {
            let n = norm_word(w);
            if n.is_empty() {
                continue;
            }
            if fi < self.flushed.len() && n == self.flushed[fi] {
                fi += 1;
                cut = i + 1;
            } else {
                break;
            }
        }
        if fi < self.flushed.len() {
            return s.to_string();
        }
        words[cut..].join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(r: &mut RollingText, text: &str, t: Instant) -> (Vec<String>, Option<String>) {
        let s = r.update(text, t);
        (s.sentences, s.tail)
    }

    #[test]
    fn splits_sentences() {
        assert_eq!(
            split("Hello there.  How are\nyou? Version 3.5 is out"),
            ["Hello there.", "How are you?", "Version 3.5 is out"]
        );
        assert_eq!(split("“Stop!” he said."), ["“Stop!”", "he said."]);
        assert_eq!(split("你好。谢谢"), ["你好。", "谢谢"]);
        assert!(split("   ").is_empty());
    }

    #[test]
    fn reseed_absorbs_text_heard_while_muted() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        run(&mut r, "Before. Our own", t);
        r.reseed("Before. Our own voice reading", t);
        assert_eq!(r.tick(t + STALE_AFTER), Step::default());
        let (s, tail) = run(&mut r, "Before. Our own voice reading this. Next", t);
        assert!(s.is_empty());
        assert_eq!(tail.as_deref(), Some("Next"));
        let (s, _) = run(
            &mut r,
            "Before. Our own voice reading this. Next one. Then",
            t,
        );
        assert_eq!(s, ["Next one."]);
    }

    #[test]
    fn ignores_text_present_at_start() {
        let t = Instant::now();
        let mut r = RollingText::default();
        assert_eq!(run(&mut r, "Old stuff. Older tail", t), (vec![], None));
        let (s, tail) = run(&mut r, "Old stuff. Older tail here. New one", t);
        assert_eq!(s, ["Older tail here."]);
        assert_eq!(tail.as_deref(), Some("New one"));
    }

    #[test]
    fn rolling_window_emits_each_sentence_once() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        let (s, tail) = run(&mut r, "hello there how", t);
        assert!(s.is_empty());
        assert_eq!(tail.as_deref(), Some("hello there how"));
        // Punctuation arrives late and splits the line.
        let (s, tail) = run(&mut r, "Hello there. How are you", t);
        assert_eq!(s, ["Hello there."]);
        assert_eq!(tail.as_deref(), Some("How are you"));
        // Same text again: nothing new.
        assert_eq!(run(&mut r, "Hello there. How are you", t), (vec![], None));
        let (s, tail) = run(&mut r, "Hello there. How are you? I am fine", t);
        assert_eq!(s, ["How are you?"]);
        assert_eq!(tail.as_deref(), Some("I am fine"));
    }

    #[test]
    fn front_trimming_does_not_reemit() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        run(
            &mut r,
            "We went to the market yesterday. Then we came home. And",
            t,
        );
        // The window scrolled: the first sentence is cut mid-way, then dropped entirely.
        let (s, tail) = run(
            &mut r,
            "the market yesterday. Then we came home. And slept well. The",
            t,
        );
        assert_eq!(s, ["And slept well."]);
        assert_eq!(tail.as_deref(), Some("The"));
        let (s, _) = run(&mut r, "And slept well. The end. Bye", t);
        assert_eq!(s, ["The end."]);
        // Only a cut-off fragment of an emitted sentence is left before the new one.
        let (s, tail) = run(&mut r, "well. The end. Bye now. See", t);
        assert_eq!(s, ["Bye now."]);
        assert_eq!(tail.as_deref(), Some("See"));
        let (s, _) = run(&mut r, "end. Bye now. See you soon. Ok", t);
        assert_eq!(s, ["See you soon."]);
    }

    #[test]
    fn late_rewording_is_not_a_new_sentence() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        let (s, _) = run(&mut r, "I want to go their today. Next", t);
        assert_eq!(s, ["I want to go their today."]);
        let (s, _) = run(&mut r, "I want to go there today. Next one", t);
        assert!(s.is_empty());
        let (s, _) = run(&mut r, "I want to go there today. Next one is here. So", t);
        assert_eq!(s, ["Next one is here."]);
    }

    #[test]
    fn stale_tail_flushes_once() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        run(&mut r, "Good morning everyone.", t);
        assert_eq!(r.deadline(), Some(t + STALE_AFTER));
        assert_eq!(r.tick(t + Duration::from_millis(1000)), Step::default());
        let s = r.tick(t + STALE_AFTER);
        assert_eq!(s.sentences, ["Good morning everyone."]);
        assert_eq!(s.tail.as_deref(), Some(""));
        assert_eq!(r.deadline(), None);
        // The same window text, now followed by more speech, does not repeat it.
        let t2 = t + Duration::from_secs(3);
        let (s, tail) = run(&mut r, "Good morning everyone. Today", t2);
        assert!(s.is_empty());
        assert_eq!(tail.as_deref(), Some("Today"));
    }

    #[test]
    fn stale_unfinished_tail_is_stripped_from_its_completion() {
        let t = Instant::now();
        let mut r = RollingText::default();
        run(&mut r, "", t);
        run(&mut r, "and then we", t);
        let s = r.tick(t + STALE_AFTER);
        assert_eq!(s.sentences, ["and then we"]);
        let t2 = t + Duration::from_secs(2);
        let (s, tail) = run(&mut r, "And then we went home", t2);
        assert!(s.is_empty());
        assert_eq!(tail.as_deref(), Some("went home"));
        let (s, tail) = run(&mut r, "And then we went home. Later", t2);
        assert_eq!(s, ["went home."]);
        assert_eq!(tail.as_deref(), Some("Later"));
    }
}
