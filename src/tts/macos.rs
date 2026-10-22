use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance,
    AVSpeechUtteranceDefaultSpeechRate, AVSpeechUtteranceMaximumSpeechRate,
};
use objc2_foundation::NSString;

use super::{POLL, Voice};

pub const VOICE_HINT: &str = "Add one in System Settings > Accessibility > Spoken Content (Read & Speak) > System voice > Manage Voices.";
const START_TIMEOUT: Duration = Duration::from_secs(5);

pub fn voices() -> Vec<Voice> {
    // SAFETY: AVSpeechSynthesisVoice class and property getters, callable from any thread.
    unsafe {
        AVSpeechSynthesisVoice::speechVoices()
            .iter()
            .map(|v| Voice {
                id: v.identifier().to_string(),
                name: v.name().to_string(),
                lang: v.language().to_string(),
            })
            .collect()
    }
}

/// The voice macOS uses for `lang` (the one picked in System Settings).
pub fn default_voice(lang: &str) -> Option<String> {
    // SAFETY: as in `voices`.
    unsafe {
        AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(lang)))
            .map(|v| v.identifier().to_string())
    }
}

pub fn speak(
    text: &str,
    _lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    // SAFETY: the synthesizer is created, used and dropped on this thread; it speaks without a
    // run loop and `isSpeaking` reflects its state.
    unsafe {
        let synth = AVSpeechSynthesizer::new();
        let utterance = AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));
        if let Some(v) = voice {
            utterance.setVoice(
                AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(&v.id)).as_deref(),
            );
        }
        utterance.setRate(
            (AVSpeechUtteranceDefaultSpeechRate * rate).min(AVSpeechUtteranceMaximumSpeechRate),
        );
        synth.speakUtterance(&utterance);
        let start = Instant::now();
        let mut started = false;
        loop {
            std::thread::sleep(POLL);
            if cancel.load(Ordering::Relaxed) {
                synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
                return Ok(());
            }
            if synth.isSpeaking() {
                started = true;
            } else if started {
                return Ok(());
            } else if start.elapsed() > START_TIMEOUT {
                anyhow::bail!("macOS speech did not start");
            }
        }
    }
}
