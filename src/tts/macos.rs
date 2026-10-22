use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesisVoiceGender, AVSpeechSynthesizer,
    AVSpeechUtterance,
};
use objc2_foundation::NSString;

use super::{Gender, POLL, Voice};

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
                gender: match v.gender() {
                    AVSpeechSynthesisVoiceGender::Female => Some(Gender::Female),
                    AVSpeechSynthesisVoiceGender::Male => Some(Gender::Male),
                    _ => None,
                },
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

/// AVSpeechUtterance rate for a speed multiplier. The scale is not linear: 0.5 is normal speed,
/// 0.0 half and 1.0 about 4x. (speed, rate) points measured with Samantha on macOS 26.
fn av_rate(speed: f32) -> f32 {
    const POINTS: [(f32, f32); 7] = [
        (0.5, 0.0),
        (1.0, 0.5),
        (1.3, 0.55),
        (1.6, 0.6),
        (2.1, 0.7),
        (2.8, 0.8),
        (4.0, 1.0),
    ];
    let s = speed.clamp(0.5, 4.0);
    let i = POINTS.iter().position(|p| s <= p.0).unwrap_or(0).max(1);
    let ((a, ra), (b, rb)) = (POINTS[i - 1], POINTS[i]);
    ra + (rb - ra) * (s - a) / (b - a)
}

/// pitchMultiplier: 0.5..2.0, 1.0 = normal.
fn pitch_multiplier(pitch: f32) -> f32 {
    pitch.clamp(0.5, 2.0)
}

pub fn speak(
    text: &str,
    _lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    pitch: f32,
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
        utterance.setRate(av_rate(rate));
        utterance.setPitchMultiplier(pitch_multiplier(pitch));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rate_and_pitch() {
        assert_eq!(av_rate(1.0), 0.5);
        assert_eq!(av_rate(0.5), 0.0);
        assert_eq!(av_rate(0.1), 0.0);
        assert_eq!(av_rate(4.0), 1.0);
        assert_eq!(av_rate(9.0), 1.0);
        assert!((av_rate(0.8) - 0.3).abs() < 1e-6);
        let rates = [0.8, 0.95, 1.0, 1.05, 1.35, 2.0];
        for w in rates.windows(2) {
            assert!(av_rate(w[0]) < av_rate(w[1]), "{w:?}");
        }
        assert_eq!(pitch_multiplier(1.15), 1.15);
        assert_eq!(pitch_multiplier(0.1), 0.5);
    }
}
