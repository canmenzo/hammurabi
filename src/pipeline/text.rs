//! Port of Babil's `clean()`. Word-level because `regex` has no backrefs or lookahead.

use std::sync::LazyLock;

use regex::Regex;

/// Whisper's no-speech probability: real speech < 0.01, fillers ~0.2, silence/noise/music > 0.8.
const NO_SPEECH: f32 = 0.5;
/// Above this, one- or two-word results are treated as noise.
const UNSURE: f32 = 0.12;
/// large-v3-turbo has no working no-speech score; its language confidence (top two) is ~1 on
/// speech, 0.5-0.6 on noise and music, ~0.75 on "hmm".
const LANG_MIN: f32 = 0.7;
const LANG_UNSURE: f32 = 0.85;

fn re(s: &str) -> Regex {
    Regex::new(s).unwrap()
}

static BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\[[^\]]*\]|\([^)]*\)|\*[^*]*\*|[♪♫]"));
static FILLER_EN: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:u+m+|u+h+m*|e+r+m+|h+m+|hu+m+|m{2,}|mhm|mm-hmm|uh-huh|a+h+|e+h+|hu+h)$")
});
static FILLER_TR: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:ı{2,}|e{2,}|a{2,}|hı+m*|h+m+|m{2,})$"));
static FILLER_OTHER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:h+m+|m{2,})$"));
static COMMA_BEFORE_END: LazyLock<Regex> = LazyLock::new(|| re(r",+([.!?…])"));
static SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| re(r"\s+([,.!?…])"));
static LEADING_PUNCT: LazyLock<Regex> = LazyLock::new(|| re(r"^[\s,.!?…]+"));
static HALLUCINATIONS: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        re(r"(?i)altyaz[ıi].*m\.?\s?k\.?"),
        re(r"(?i)amara\.org"),
        re(r"(?i)^(thanks|thank you) for watching[.!]?$"),
        re(r"(?i)^abone ol"),
    ]
});
static INTERJ_EN: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(oh|ah|yeah|yep|yup|okay|ok|right|so|well|huh|mhm)[.!?,…]*$"));
static INTERJ_TR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(hı+|şey|yani)[.!?,…]*$"));

const PUNCT: [char; 5] = [',', '.', '!', '?', '…'];

/// Returns the cleaned text, or an empty string when the result should be dropped.
pub fn clean(text: &str, language: &str, no_speech: f32, lang_conf: f32) -> String {
    if no_speech > NO_SPEECH || lang_conf < LANG_MIN {
        return String::new();
    }
    let stripped = BRACKETS.replace_all(text, " ");

    // A word repeated more than 3 times in a row collapses to 3.
    let mut words: Vec<&str> = Vec::new();
    let mut run = 0;
    for w in stripped.split_whitespace() {
        run = match words.last() {
            Some(prev) if prev.to_lowercase() == w.to_lowercase() => run + 1,
            _ => 1,
        };
        if run <= 3 {
            words.push(if run == 1 { w } else { words[words.len() - 1] });
        }
    }

    // Fillers are dropped, keeping any sentence-ending punctuation they carried.
    let filler = match language {
        "en" => &*FILLER_EN,
        "tr" => &*FILLER_TR,
        _ => &*FILLER_OTHER,
    };
    let mut out = String::new();
    for w in words {
        let core = w.trim_end_matches(PUNCT);
        if !core.is_empty() && filler.is_match(core) {
            let punct = &w[core.len()..];
            if punct.contains(['.', '!', '?', '…']) {
                out.push_str(&punct.replace(',', ""));
            }
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    let t = COMMA_BEFORE_END.replace_all(&out, "$1");
    let t = SPACE_BEFORE_PUNCT.replace_all(&t, "$1");
    let t = LEADING_PUNCT.replace(&t, "");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");

    if !t.chars().any(char::is_alphabetic) {
        return String::new();
    }
    let interjection = match language {
        "en" => INTERJ_EN.is_match(&t),
        "tr" => INTERJ_TR.is_match(&t),
        _ => false,
    };
    if interjection || HALLUCINATIONS.iter().any(|r| r.is_match(&t)) {
        return String::new();
    }
    if (no_speech > UNSURE || lang_conf < LANG_UNSURE) && t.split(' ').count() <= 2 {
        return String::new();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(t: &str) -> String {
        clean(t, "en", 0.0, 1.0)
    }

    #[test]
    fn thresholds() {
        assert_eq!(clean("Hello there my friend.", "en", 0.6, 1.0), "");
        assert_eq!(clean("Hello there my friend.", "en", 0.0, 0.6), "");
        assert_eq!(clean("Hello there.", "en", 0.2, 1.0), "");
        assert_eq!(clean("Hello there.", "en", 0.0, 0.8), "");
        assert_eq!(
            clean("Hello there my friend.", "en", 0.2, 0.8),
            "Hello there my friend."
        );
        assert_eq!(c("Hello there."), "Hello there.");
    }

    #[test]
    fn strips_annotations() {
        assert_eq!(
            c("[Music] Hello (laughs) world *applause* ♪"),
            "Hello world"
        );
        assert_eq!(c("[Music]"), "");
        assert_eq!(c("♪ ♪ ♪"), "");
    }

    #[test]
    fn fillers() {
        assert_eq!(
            c("Um, I think, uh, we should go."),
            "I think, we should go."
        );
        assert_eq!(c("So we went there um."), "So we went there.");
        assert_eq!(c("We went there, uhm."), "We went there.");
        assert_eq!(c("Hmm hmm mm"), "");
        assert_eq!(
            clean("Şey ıı bence eee güzel.", "tr", 0.0, 1.0),
            "Şey bence güzel."
        );
        // "um" is not an English-only filler elsewhere, but "hmm" is.
        assert_eq!(
            clean("hmm Hola amigo mío", "es", 0.0, 1.0),
            "Hola amigo mío"
        );
    }

    #[test]
    fn repeats() {
        assert_eq!(c("no no no no no no stop"), "no no no stop");
        assert_eq!(c("No no NO no"), "No No No");
        assert_eq!(c("go go go now"), "go go go now");
    }

    #[test]
    fn hallucinations_and_interjections() {
        assert_eq!(c("Thanks for watching!"), "");
        assert_eq!(c("Subtitles by amara.org community"), "");
        assert_eq!(clean("Altyazı M.K.", "tr", 0.0, 1.0), "");
        assert_eq!(clean("Abone olmayı unutmayın", "tr", 0.0, 1.0), "");
        assert_eq!(c("Okay."), "");
        assert_eq!(c("Yeah!"), "");
        assert_eq!(clean("Yani...", "tr", 0.0, 1.0), "");
        assert_eq!(c("Okay, let's start."), "Okay, let's start.");
        assert_eq!(c("..."), "");
    }
}
