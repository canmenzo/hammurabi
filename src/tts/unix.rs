//! speech-dispatcher through `spd-say`, else `espeak-ng`. Both are spawned instead of linked so
//! Hammurabi starts on systems without them.

use std::io::ErrorKind;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Gender, Output, POLL, Voice, decode_wav, play_rendered};

pub const VOICE_HINT: &str = "Install a voice for it in speech-dispatcher or espeak-ng.";

pub fn voices() -> Vec<Voice> {
    Command::new("spd-say")
        .arg("-L")
        .stderr(Stdio::null())
        .output()
        .map(|o| parse_voices(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// `spd-say -L`: a "NAME LANGUAGE VARIANT" header, then one voice per line; names may have spaces.
/// Only espeak-style variants ("f2", "m3", "female1") tell the gender.
fn parse_voices(out: &str) -> Vec<Voice> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().rev();
            let (variant, lang) = (it.next()?, it.next()?);
            let name = it.rev().collect::<Vec<_>>().join(" ");
            (!name.is_empty() && name != "NAME").then(|| Voice {
                id: name.clone(),
                name,
                lang: lang.into(),
                gender: match variant.trim_end_matches(|c: char| c.is_ascii_digit()) {
                    "f" | "female" => Some(Gender::Female),
                    "m" | "male" => Some(Gender::Male),
                    _ => None,
                },
            })
        })
        .collect()
}

/// speech-dispatcher's -100..100 for a multiplier: 100 = double, -100 = half.
fn spd_value(m: f32) -> String {
    ((m.log2() * 100.0).round() as i32)
        .clamp(-100, 100)
        .to_string()
}

/// espeak-ng words per minute: 175 = normal, 80..450.
fn espeak_speed(rate: f32) -> String {
    ((175.0 * rate).round() as u32).clamp(80, 450).to_string()
}

/// espeak-ng pitch: 50 = normal, 0..99.
fn espeak_pitch(pitch: f32) -> String {
    ((50.0 * pitch).round() as u32).clamp(0, 99).to_string()
}

pub fn default_voice(_lang: &str) -> Option<String> {
    None
}

fn espeak(lang: &str, rate: f32, pitch: f32, volume: f32) -> Command {
    let mut cmd = Command::new("espeak-ng");
    cmd.args(["-s", &espeak_speed(rate), "-p", &espeak_pitch(pitch)]);
    // 0..200, where 100 is the default.
    cmd.args(["-a", &(volume * 100.0).round().to_string()]);
    if let Some(l) = lang.split('-').next().filter(|l| !l.is_empty()) {
        cmd.args(["-v", l]);
    }
    cmd
}

/// speech-dispatcher only plays on its own output, so another device takes espeak-ng's audio.
fn speak_on_device(
    text: &str,
    lang: &str,
    rate: f32,
    pitch: f32,
    out: Output,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let rendered = espeak(lang, rate, pitch, 1.0)
        .arg("--stdout")
        .arg("--")
        .arg(text)
        .stderr(Stdio::null())
        .output();
    let rendered = match rendered {
        Err(e) if e.kind() == ErrorKind::NotFound => {
            anyhow::bail!("reading on another device needs espeak-ng")
        }
        r => r?,
    };
    anyhow::ensure!(
        rendered.status.success(),
        "espeak-ng exited with {}",
        rendered.status
    );
    let (mut pcm, sample_rate) = decode_wav(&rendered.stdout)?;
    play_rendered(&mut pcm, sample_rate, out, cancel)
}

fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

pub fn speak(
    text: &str,
    lang: &str,
    voice: Option<&Voice>,
    rate: f32,
    pitch: f32,
    out: Output,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    if out.device.is_some() {
        return speak_on_device(text, lang, rate, pitch, out, cancel);
    }
    let volume = out.volume;
    let mut spd = Command::new("spd-say");
    // -w waits until the text is spoken.
    spd.args(["-w", "-r", &spd_value(rate), "-p", &spd_value(pitch)]);
    // -100..100, where 0 is the default.
    spd.args(["-i", &((volume - 1.0) * 100.0).round().to_string()]);
    if !lang.is_empty() {
        spd.args(["-l", lang]);
    }
    if let Some(v) = voice {
        spd.args(["-y", &v.id]);
    }
    spd.arg("--").arg(text);
    let (mut child, is_spd) = match spawn(&mut spd) {
        Ok(c) => (c, true),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            match spawn(espeak(lang, rate, pitch, volume).arg("--").arg(text)) {
                Ok(c) => (c, false),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    anyhow::bail!("install speech-dispatcher (spd-say) or espeak-ng")
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) => return Err(e.into()),
    };
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(status.success(), "speech program exited with {status}");
            return Ok(());
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            if is_spd {
                // The daemon keeps speaking after its client is gone.
                let _ = spawn(Command::new("spd-say").arg("-C")).and_then(|mut c| c.wait());
            }
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spd_voices() {
        let out = "     NAME     LANGUAGE     VARIANT\n         Turkish           tr        none\nChinese (Mandarin)  cmn  none\nTurkish+f2  tr  f2\nTurkish+m3  tr  m3\n\n";
        let v = parse_voices(out);
        assert_eq!(v.len(), 4);
        assert_eq!((v[0].id.as_str(), v[0].lang.as_str()), ("Turkish", "tr"));
        assert_eq!(v[1].name, "Chinese (Mandarin)");
        assert_eq!(v[0].gender, None);
        assert_eq!(v[2].gender, Some(Gender::Female));
        assert_eq!(v[3].gender, Some(Gender::Male));
    }

    #[test]
    fn maps_rate_and_pitch() {
        assert_eq!(spd_value(1.0), "0");
        assert_eq!(spd_value(2.0), "100");
        assert_eq!(spd_value(0.5), "-100");
        assert_eq!(spd_value(1.15), "20");
        assert_eq!(spd_value(0.8), "-32");
        assert_eq!(spd_value(9.0), "100");
        assert_eq!(espeak_speed(1.0), "175");
        assert_eq!(espeak_speed(1.35), "236");
        assert_eq!(espeak_speed(0.1), "80");
        assert_eq!(espeak_pitch(1.0), "50");
        assert_eq!(espeak_pitch(0.8), "40");
        assert_eq!(espeak_pitch(3.0), "99");
    }
}
