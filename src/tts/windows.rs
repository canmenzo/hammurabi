use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FLAGS, SND_MEMORY, SND_NODEFAULT};
use windows::core::{HSTRING, PCWSTR};

use super::{Gender, POLL, Voice, lang_matches};

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

/// Scales the samples of a 16-bit PCM WAV file in place; PlaySound has no volume of its own.
fn scale_wav16(wav: &mut [u8], volume: f32) {
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size = u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]) as usize;
        let start = at + 8;
        if &wav[at..at + 4] == b"data" {
            let end = start.saturating_add(size).min(wav.len());
            for s in wav[start..end].as_chunks_mut::<2>().0 {
                let v = f32::from(i16::from_le_bytes(*s)) * volume;
                s.copy_from_slice(&(v as i16).to_le_bytes());
            }
            return;
        }
        // Chunks are padded to an even size.
        at = start.saturating_add(size + (size & 1));
    }
}

pub fn speak(
    text: &str,
    _lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    pitch: f32,
    volume: f32,
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
    let r = hound::WavReader::new(Cursor::new(&wav))?;
    let length = Duration::from_secs_f64(f64::from(r.duration()) / f64::from(r.spec().sample_rate));
    let bits = r.spec().bits_per_sample;
    if bits == 16 {
        scale_wav16(&mut wav, volume);
    }

    // SAFETY: `wav` is a complete WAV file and outlives the sound, which is stopped below.
    unsafe {
        PlaySoundW(
            PCWSTR(wav.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        )
    }
    .ok()?;
    // PlaySound has no completion event; the WAV's length (plus device latency) says when it ends.
    let end = Instant::now() + length + Duration::from_millis(150);
    while Instant::now() < end && !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
    }
    // SAFETY: a null sound stops whatever this process is playing.
    let _ = unsafe { PlaySoundW(PCWSTR::null(), None, SND_FLAGS::default()) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_only_the_data_chunk() {
        let mut wav = b"RIFF....WAVEfmt \x02\0\0\0\x10\x20data\x04\0\0\0".to_vec();
        wav.extend([100i16, -100].iter().flat_map(|s| s.to_le_bytes()));
        let head = wav[..wav.len() - 4].to_vec();
        scale_wav16(&mut wav, 0.5);
        assert_eq!(&wav[..head.len()], &head[..]);
        assert_eq!(&wav[head.len()..head.len() + 2], &50i16.to_le_bytes());
        assert_eq!(&wav[head.len() + 2..], &(-50i16).to_le_bytes());
    }

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
