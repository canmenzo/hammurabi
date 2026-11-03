use std::sync::atomic::AtomicBool;

use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows::core::HSTRING;

use super::{Gender, Output, Voice, decode_wav, lang_matches, play_rendered};

pub const VOICE_HINT: &str =
    "Add one in Windows Settings > Time & language > Speech > Manage voices.";

fn info(v: &VoiceInformation) -> windows::core::Result<Voice> {
    Ok(Voice {
        id: v.Id()?.to_string(),
        name: v.DisplayName()?.to_string(),
        lang: v.Language()?.to_string(),
        gender: match v.Gender()? {
            VoiceGender::Female => Some(Gender::Female),
            VoiceGender::Male => Some(Gender::Male),
            _ => None,
        },
    })
}

/// SpeakingRate: 0.5..6.0, 1.0 = normal.
fn speaking_rate(rate: f32) -> f64 {
    f64::from(rate).clamp(0.5, 6.0)
}

/// AudioPitch: 0.0..2.0, 1.0 = normal.
fn audio_pitch(pitch: f32) -> f64 {
    f64::from(pitch).clamp(0.0, 2.0)
}

pub fn voices() -> Vec<Voice> {
    SpeechSynthesizer::AllVoices()
        .map(|all| all.into_iter().filter_map(|v| info(&v).ok()).collect())
        .unwrap_or_default()
}

/// The voice picked in Windows Settings, when it speaks `lang`.
pub fn default_voice(lang: &str) -> Option<String> {
    let v = info(&SpeechSynthesizer::DefaultVoice().ok()?).ok()?;
    lang_matches(&v.lang, lang).then_some(v.id)
}

pub fn speak(
    text: &str,
    _lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    pitch: f32,
    out: Output,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let synth = SpeechSynthesizer::new()?;
    if let Some(v) = voice
        && let Some(i) = SpeechSynthesizer::AllVoices()?
            .into_iter()
            .find(|i| i.Id().is_ok_and(|id| id == v.id.as_str()))
    {
        synth.SetVoice(&i)?;
    }
    let options = synth.Options()?;
    options.SetSpeakingRate(speaking_rate(rate))?;
    // Fails before Windows 10 1803, which then speaks at normal pitch.
    let _ = options.SetAudioPitch(audio_pitch(pitch));
    let stream = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))?
        .join()?;
    let size = u32::try_from(stream.Size()?)?;
    let buf = stream
        .ReadAsync(&Buffer::Create(size)?, size, InputStreamOptions::None)?
        .join()?;
    let mut wav = vec![0u8; buf.Length()? as usize];
    DataReader::FromBuffer(&buf)?.ReadBytes(&mut wav)?;
    let (mut pcm, sample_rate) = decode_wav(&wav)?;
    play_rendered(&mut pcm, sample_rate, out, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rate_and_pitch() {
        assert_eq!(speaking_rate(1.0), 1.0);
        assert_eq!(speaking_rate(1.35), f64::from(1.35f32));
        assert_eq!(speaking_rate(0.1), 0.5);
        assert_eq!(audio_pitch(1.0), 1.0);
        assert_eq!(audio_pitch(0.8), f64::from(0.8f32));
        assert_eq!(audio_pitch(3.0), 2.0);
    }
}
