//! speech-dispatcher through `spd-say`, else `espeak-ng`. Both are spawned instead of linked so
//! Hammurabi starts on systems without them.

use std::io::ErrorKind;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{POLL, Voice};

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
fn parse_voices(out: &str) -> Vec<Voice> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().rev();
            let (_variant, lang) = (it.next()?, it.next()?);
            let name = it.rev().collect::<Vec<_>>().join(" ");
            (!name.is_empty() && name != "NAME").then(|| Voice {
                id: name.clone(),
                name,
                lang: lang.into(),
            })
        })
        .collect()
}

pub fn default_voice(_lang: &str) -> Option<String> {
    None
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
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut spd = Command::new("spd-say");
    // -w waits until the text is spoken; -r is -100..100.
    spd.args([
        "-w",
        "-r",
        &(((rate - 1.0) * 100.0) as i32).clamp(-100, 100).to_string(),
    ]);
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
            let mut espeak = Command::new("espeak-ng");
            espeak.args(["-s", &((175.0 * rate) as u32).to_string()]);
            if let Some(l) = lang.split('-').next().filter(|l| !l.is_empty()) {
                espeak.args(["-v", l]);
            }
            espeak.arg("--").arg(text);
            match spawn(&mut espeak) {
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
        let out = "     NAME     LANGUAGE     VARIANT\n         Turkish           tr        none\nChinese (Mandarin)  cmn  none\n\n";
        let v = parse_voices(out);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].id.as_str(), v[0].lang.as_str()), ("Turkish", "tr"));
        assert_eq!(v[1].name, "Chinese (Mandarin)");
    }
}
