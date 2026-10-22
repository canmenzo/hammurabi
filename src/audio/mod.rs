pub mod capture;
pub mod loopback;
pub mod resample;

pub use resample::Frames16k;

use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, DeviceId, Host};
use crossbeam_channel::Sender;

use crate::types::{AudioChunk, AudioSource, SourceKind};

/// Set by a stream's error callback when its device went away; the capture must be restarted.
pub type Failure = Arc<Mutex<Option<String>>>;

/// Owns the cpal streams; create and drop it on the engine thread (streams are not Send everywhere).
pub struct AudioCapture {
    _streams: Vec<cpal::Stream>,
    failure: Failure,
    /// What was used instead of the requested source or device, for the user.
    pub notes: Vec<String>,
}

impl AudioCapture {
    /// `input`/`output` are cpal device ids; None = OS default. A device that is gone or fails
    /// is replaced by the default one; system audio that fails falls back to the microphone,
    /// and `Both` keeps whichever source works.
    pub fn start(
        source: AudioSource,
        input: Option<&str>,
        output: Option<&str>,
        tx: Sender<AudioChunk>,
    ) -> anyhow::Result<AudioCapture> {
        let host = cpal::default_host();
        let failure = Failure::default();
        let mut notes = Vec::new();
        let (streams, more) = open_sources(source, |kind| {
            let (id, what) = match kind {
                SourceKind::System => (output, "audio output"),
                SourceKind::Microphone => (input, "microphone"),
            };
            let open = |id| open_stream(&host, kind, id, tx.clone(), failure.clone());
            let Some(id) = id else {
                return open(None);
            };
            let found = id
                .parse::<DeviceId>()
                .ok()
                .and_then(|d| host.device_by_id(&d))
                .is_some();
            let r = if found {
                open(Some(id))
            } else {
                Err(anyhow!("not connected"))
            };
            r.or_else(|e| {
                log::warn!("{what} {id}: {e:#}");
                let s = open(None)?;
                notes.push(format!(
                    "The selected {what} is not available, using the default"
                ));
                Ok(s)
            })
        })?;
        notes.extend(more);
        Ok(AudioCapture {
            _streams: streams,
            failure,
            notes,
        })
    }

    /// Why a stream stopped delivering audio (device unplugged or default device changed).
    pub fn failure(&self) -> Option<String> {
        self.failure.lock().ok()?.clone()
    }

    pub fn stop(self) {}
}

fn open_stream(
    host: &Host,
    kind: SourceKind,
    id: Option<&str>,
    tx: Sender<AudioChunk>,
    failure: Failure,
) -> anyhow::Result<cpal::Stream> {
    let (device, config) = match kind {
        SourceKind::System => loopback::system_device(host, id)?,
        SourceKind::Microphone => {
            let device = find_device(host, id, false)?;
            let config = device.default_input_config()?;
            (device, config)
        }
    };
    let stream = capture::start_stream(&device, config, kind, tx, failure)?;
    // On macOS building the stream waits for the permission prompt; a denial only shows here.
    #[cfg(target_os = "macos")]
    loopback::check_permission(kind, true)?;
    Ok(stream)
}

fn source_name(kind: SourceKind, start: bool) -> &'static str {
    match (kind, start) {
        (SourceKind::System, true) => "System audio",
        (SourceKind::System, false) => "system audio",
        (SourceKind::Microphone, true) => "Microphone",
        (SourceKind::Microphone, false) => "the microphone",
    }
}

/// Opens the sources of `source` with `open`, applying the fallbacks described on `start`.
/// Returns the opened streams and notes about what was substituted.
fn open_sources<T>(
    source: AudioSource,
    mut open: impl FnMut(SourceKind) -> anyhow::Result<T>,
) -> anyhow::Result<(Vec<T>, Vec<String>)> {
    let kinds: &[SourceKind] = match source {
        AudioSource::System => &[SourceKind::System],
        AudioSource::Microphone => &[SourceKind::Microphone],
        AudioSource::Both => &[SourceKind::System, SourceKind::Microphone],
    };
    let mut opened = Vec::new();
    let mut failed = Vec::new();
    for &kind in kinds {
        match open(kind) {
            Ok(s) => opened.push(s),
            Err(e) => failed.push((kind, format!("{e:#}"))),
        }
    }
    if opened.is_empty() && source == AudioSource::System {
        let e = &failed[0].1;
        let s = open(SourceKind::Microphone)
            .map_err(|m| anyhow!("system audio: {e}; microphone: {m:#}"))?;
        let note = format!("System audio unavailable ({e}), using the microphone");
        return Ok((vec![s], vec![note]));
    }
    if opened.is_empty() {
        let all: Vec<String> = failed
            .iter()
            .map(|(k, e)| format!("{}: {e}", source_name(*k, true)))
            .collect();
        anyhow::bail!("{}", all.join("; "));
    }
    let notes = failed
        .into_iter()
        .map(|(k, e)| {
            let other = match k {
                SourceKind::System => SourceKind::Microphone,
                SourceKind::Microphone => SourceKind::System,
            };
            format!(
                "{} unavailable ({e}), using {} only",
                source_name(k, true),
                source_name(other, false)
            )
        })
        .collect();
    Ok((opened, notes))
}

/// Device by cpal id, falling back to the OS default when the id is gone (unplugged headset).
fn find_device(host: &Host, id: Option<&str>, output: bool) -> anyhow::Result<Device> {
    if let Some(id) = id {
        match id
            .parse::<DeviceId>()
            .ok()
            .and_then(|d| host.device_by_id(&d))
        {
            Some(d) => return Ok(d),
            None => log::warn!("audio device {id} not found, using the default"),
        }
    }
    let device = if output {
        host.default_output_device()
    } else {
        host.default_input_device()
    };
    device.ok_or_else(|| {
        anyhow::anyhow!(if output {
            "No audio output device found"
        } else {
            "No microphone found"
        })
    })
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    /// cpal device id.
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceList {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
    /// Err explains why system audio capture is unavailable on this machine.
    pub system_audio: Result<(), String>,
}

pub fn list_devices() -> DeviceList {
    let host = cpal::default_host();
    let default_in = host.default_input_device().and_then(|d| d.id().ok());
    let default_out = host.default_output_device().and_then(|d| d.id().ok());
    let info = |d: Device, default: &Option<DeviceId>| {
        let id = d.id().ok()?;
        Some(DeviceInfo {
            name: d
                .description()
                .map(|desc| desc.name().to_string())
                .unwrap_or_else(|_| id.id().to_string()),
            is_default: default.as_ref() == Some(&id),
            id: id.to_string(),
        })
    };
    DeviceList {
        inputs: host
            .input_devices()
            .map(|it| it.filter_map(|d| info(d, &default_in)).collect())
            .unwrap_or_default(),
        outputs: host
            .output_devices()
            .map(|it| it.filter_map(|d| info(d, &default_out)).collect())
            .unwrap_or_default(),
        system_audio: loopback::system_device(&host, None)
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        source: AudioSource,
        ok: &[SourceKind],
    ) -> anyhow::Result<(Vec<SourceKind>, Vec<String>)> {
        open_sources(source, |k| {
            if ok.contains(&k) {
                Ok(k)
            } else {
                Err(anyhow!("broken"))
            }
        })
    }

    #[test]
    fn source_fallbacks() {
        use SourceKind::{Microphone as M, System as S};
        let all = [S, M];
        assert_eq!(run(AudioSource::System, &all).unwrap(), (vec![S], vec![]));
        assert_eq!(run(AudioSource::Both, &all).unwrap(), (vec![S, M], vec![]));

        let (got, notes) = run(AudioSource::System, &[M]).unwrap();
        assert_eq!(got, vec![M]);
        assert_eq!(
            notes,
            vec!["System audio unavailable (broken), using the microphone"]
        );

        let (got, notes) = run(AudioSource::Both, &[S]).unwrap();
        assert_eq!(got, vec![S]);
        assert_eq!(
            notes,
            vec!["Microphone unavailable (broken), using system audio only"]
        );
        let (got, notes) = run(AudioSource::Both, &[M]).unwrap();
        assert_eq!(got, vec![M]);
        assert_eq!(
            notes,
            vec!["System audio unavailable (broken), using the microphone only"]
        );

        let e = run(AudioSource::System, &[]).unwrap_err().to_string();
        assert_eq!(e, "system audio: broken; microphone: broken");
        let e = run(AudioSource::Both, &[]).unwrap_err().to_string();
        assert_eq!(e, "System audio: broken; Microphone: broken");
        assert!(run(AudioSource::Microphone, &[S]).is_err());
    }
}
