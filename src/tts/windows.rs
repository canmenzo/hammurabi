use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceInformation};
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FLAGS, SND_MEMORY, SND_NODEFAULT};
use windows::core::{HSTRING, PCWSTR};

use super::{POLL, Voice, lang_matches};

pub const VOICE_HINT: &str =
    "Add one in Windows Settings > Time & language > Speech > Manage voices.";

fn info(v: &VoiceInformation) -> windows::core::Result<Voice> {
    Ok(Voice {
        id: v.Id()?.to_string(),
        name: v.DisplayName()?.to_string(),
        lang: v.Language()?.to_string(),
    })
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
    synth.Options()?.SetSpeakingRate(f64::from(rate))?;
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
