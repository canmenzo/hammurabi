//! End-to-end pipeline over synthetic frames with a fake recognizer and translator.

use std::collections::HashMap;

use hammurabi::pipeline::simulate;
use hammurabi::settings::Settings;
use hammurabi::types::{
    CaptionUpdate, EngineEvent, FRAME_SAMPLES, Recognizer, Transcript, Translate,
};

const WORDS: &[&str] = &[
    "Hello", "world.", "How", "are", "you", "today?", "this", "has", "no", "ending",
];
const FRAMES_PER_WORD: usize = 3;

/// Loud frame whose first sample encodes a word index; the rest is a ±0.2 square wave.
fn word_frame(idx: usize) -> Vec<f32> {
    let mut f: Vec<f32> = (0..FRAME_SAMPLES)
        .map(|i| if i % 2 == 0 { 0.2 } else { -0.2 })
        .collect();
    f[0] = idx as f32 / 1000.0;
    f
}

fn utterance(words: std::ops::Range<usize>) -> Vec<Vec<f32>> {
    words
        .flat_map(|w| std::iter::repeat_n(word_frame(w), FRAMES_PER_WORD))
        .collect()
}

fn silence(n: usize) -> Vec<Vec<f32>> {
    vec![vec![0.0; FRAME_SAMPLES]; n]
}

/// Reads back the word markers of loud frames.
#[derive(Default)]
struct FakeAsr {
    priors: Vec<HashMap<String, f32>>,
}

impl Recognizer for FakeAsr {
    fn transcribe(
        &mut self,
        audio: &[f32],
        _language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript> {
        self.priors.push(prior.clone());
        let mut words: Vec<&str> = Vec::new();
        for f in audio.chunks(FRAME_SAMPLES) {
            if f[1].abs() < 0.1 {
                continue;
            }
            let w = WORDS[(f[0] * 1000.0).round() as usize];
            if words.last() != Some(&w) {
                words.push(w);
            }
        }
        Ok(Transcript {
            text: words.join(" "),
            language: "en".into(),
            no_speech: 0.0,
            lang_conf: 1.0,
        })
    }
}

struct Upper;

impl Translate for Upper {
    fn translate(
        &mut self,
        text: &str,
        _: &str,
        _: &str,
        _: &[(String, String)],
    ) -> anyhow::Result<String> {
        Ok(text.to_uppercase())
    }
}

fn frames() -> Vec<Vec<f32>> {
    let mut f = silence(3);
    f.extend(utterance(0..6));
    f.extend(silence(10));
    f.extend(utterance(6..10));
    f.extend(silence(5));
    f
}

fn settings(target: &str, translate: bool) -> Settings {
    Settings {
        target_language: target.into(),
        translate,
        ..Settings::default()
    }
}

type Final = (u64, String, Option<String>);

fn split(events: Vec<EngineEvent>) -> (Vec<Final>, Vec<String>, Vec<CaptionUpdate>) {
    let mut finals = Vec::new();
    let mut lives = Vec::new();
    let mut all = Vec::new();
    for e in events {
        if let EngineEvent::Caption(c) = e {
            match &c {
                CaptionUpdate::Final {
                    id,
                    original,
                    translated,
                    lang,
                } => {
                    assert_eq!(lang, "en");
                    finals.push((*id, original.clone(), translated.clone()));
                }
                CaptionUpdate::Live {
                    original,
                    translated,
                    ..
                } => {
                    assert!(
                        translated.is_none(),
                        "live line not translated outside instant mode"
                    );
                    lives.push(original.clone());
                }
                CaptionUpdate::Clear => {}
            }
            all.push(c);
        }
    }
    (finals, lives, all)
}

fn f(id: u64, o: &str, t: Option<&str>) -> Final {
    (id, o.into(), t.map(String::from))
}

#[test]
fn captions_end_to_end_with_translation() {
    let mut asr = FakeAsr::default();
    let mut tr = Upper;
    let events = simulate(frames(), &mut asr, Some(&mut tr), &settings("tr", true));
    let (finals, lives, all) = split(events);
    assert_eq!(
        finals,
        vec![
            f(1, "Hello world.", None),
            f(1, "Hello world.", Some("HELLO WORLD.")),
            f(2, "How are you today?", None),
            f(2, "How are you today?", Some("HOW ARE YOU TODAY?")),
            f(3, "this has no ending", None),
            f(3, "this has no ending", Some("THIS HAS NO ENDING")),
        ]
    );
    // The live line grows with partials, shrinks after each emitted sentence, and ends cleared.
    assert_eq!(lives.first().map(String::as_str), Some("Hello"));
    assert!(lives.iter().any(|l| l == "How are you"));
    assert!(lives.iter().any(|l| l == "this has no ending"));
    assert_eq!(lives.last().map(String::as_str), Some(""));
    // "Hello world." is shown (stable partial) before the segment closes.
    let first_final = all
        .iter()
        .position(|c| matches!(c, CaptionUpdate::Final { .. }))
        .unwrap();
    assert!(
        all[..first_final]
            .iter()
            .any(|c| matches!(c, CaptionUpdate::Live { .. }))
    );

    // Language prior: after the first final with >= 3 words, English is favoured.
    let last = asr.priors.last().unwrap();
    assert!((last["en"] - 0.3).abs() < 1e-6, "{last:?}");
    assert!(asr.priors[0].is_empty());
}

#[test]
fn same_language_and_no_translator() {
    let mut asr = FakeAsr::default();
    let (finals, _, _) = split(simulate(frames(), &mut asr, None, &settings("en", true)));
    assert_eq!(finals[0], f(1, "Hello world.", None));
    assert_eq!(finals[1], f(1, "Hello world.", Some("Hello world.")));
    assert_eq!(finals.len(), 6);

    // No translator loaded yet: captions go out untranslated, once each.
    let mut asr = FakeAsr::default();
    let (finals, _, _) = split(simulate(frames(), &mut asr, None, &settings("tr", true)));
    assert_eq!(
        finals.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
        ["Hello world.", "How are you today?", "this has no ending"]
    );
    assert!(finals.iter().all(|x| x.2.is_none()));
}

#[test]
fn instant_mode_translates_live_line() {
    let mut asr = FakeAsr::default();
    let mut tr = Upper;
    let s = Settings {
        instant: true,
        ..settings("tr", true)
    };
    let events = simulate(frames(), &mut asr, Some(&mut tr), &s);
    let live_translations: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            EngineEvent::Caption(CaptionUpdate::Live {
                original,
                translated: Some(t),
                ..
            }) => {
                assert_eq!(t, original.to_uppercase());
                Some(t)
            }
            _ => None,
        })
        .collect();
    assert!(live_translations.contains(&"HELLO".to_string()));
}

#[test]
fn silence_and_noise_produce_nothing() {
    let mut asr = FakeAsr::default();
    let events = simulate(silence(100), &mut asr, None, &settings("tr", true));
    assert!(events.is_empty());
    assert!(asr.priors.is_empty());
}
