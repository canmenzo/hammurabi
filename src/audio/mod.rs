#![allow(dead_code, unused_variables)]

pub mod capture;
pub mod loopback;
pub mod resample;

pub use resample::Frames16k;

use crossbeam_channel::Sender;

use crate::types::{AudioChunk, AudioSource};

/// Owns the cpal streams; create and drop it on the engine thread (streams are not Send everywhere).
pub struct AudioCapture {}

impl AudioCapture {
    /// `input`/`output` are cpal device ids; None = OS default.
    pub fn start(
        source: AudioSource,
        input: Option<&str>,
        output: Option<&str>,
        tx: Sender<AudioChunk>,
    ) -> anyhow::Result<AudioCapture> {
        anyhow::bail!("unimplemented")
    }

    pub fn stop(self) {}
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
    DeviceList {
        inputs: Vec::new(),
        outputs: Vec::new(),
        system_audio: Err("unimplemented".into()),
    }
}
