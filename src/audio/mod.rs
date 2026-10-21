pub mod capture;
pub mod loopback;
pub mod resample;

pub use resample::Frames16k;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, DeviceId, Host};
use crossbeam_channel::Sender;

use crate::types::{AudioChunk, AudioSource, SourceKind};

/// Owns the cpal streams; create and drop it on the engine thread (streams are not Send everywhere).
pub struct AudioCapture {
    _streams: Vec<cpal::Stream>,
}

impl AudioCapture {
    /// `input`/`output` are cpal device ids; None = OS default.
    pub fn start(
        source: AudioSource,
        input: Option<&str>,
        output: Option<&str>,
        tx: Sender<AudioChunk>,
    ) -> anyhow::Result<AudioCapture> {
        let host = cpal::default_host();
        let mut streams = Vec::new();
        if matches!(source, AudioSource::System | AudioSource::Both) {
            let (device, config) = loopback::system_device(&host, output)?;
            streams.push(capture::start_stream(
                &device,
                config,
                SourceKind::System,
                tx.clone(),
            )?);
        }
        if matches!(source, AudioSource::Microphone | AudioSource::Both) {
            let device = find_device(&host, input, false)?;
            let config = device.default_input_config()?;
            streams.push(capture::start_stream(
                &device,
                config,
                SourceKind::Microphone,
                tx,
            )?);
        }
        Ok(AudioCapture { _streams: streams })
    }

    pub fn stop(self) {}
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
