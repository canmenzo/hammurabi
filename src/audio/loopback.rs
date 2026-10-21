//! Per-OS pick of the device and config that captures system audio.

use cpal::{Device, Host, SupportedStreamConfig};

/// `output` is a cpal device id of a playback device; None = OS default.
#[cfg(any(windows, target_os = "macos"))]
pub fn system_device(
    host: &Host,
    output: Option<&str>,
) -> anyhow::Result<(Device, SupportedStreamConfig)> {
    use cpal::traits::DeviceTrait;

    #[cfg(target_os = "macos")]
    check_macos_version()?;
    // Windows: an input stream on a render device is WASAPI loopback.
    // macOS 14.6+: cpal wraps an output-only device in a Core Audio process tap aggregate.
    let device = super::find_device(host, output, true)?;
    let config = device.default_output_config()?;
    Ok((device, config))
}

#[cfg(target_os = "macos")]
fn check_macos_version() -> anyhow::Result<()> {
    let out = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()?;
    let v = String::from_utf8_lossy(&out.stdout);
    let mut parts = v.trim().split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let ver = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    if ver < (14, 6) {
        anyhow::bail!(
            "System audio capture needs macOS 14.6 or newer (this is {}). Install BlackHole and pick it as the microphone instead.",
            v.trim()
        );
    }
    Ok(())
}

/// Linux: the monitor source of the chosen (or default) PulseAudio sink. Works on PipeWire via pipewire-pulse.
#[cfg(target_os = "linux")]
pub fn system_device(
    host: &Host,
    output: Option<&str>,
) -> anyhow::Result<(Device, SupportedStreamConfig)> {
    use cpal::traits::{DeviceTrait, HostTrait};
    use cpal::{DeviceId, HostId};

    if host.id() != HostId::PulseAudio {
        anyhow::bail!(
            "System audio needs a PulseAudio or PipeWire (pipewire-pulse) server, which is not running. Only the microphone is available."
        );
    }
    let sink = super::find_device(host, output, true)?;
    let name = sink.id()?.id().to_string();
    let monitor = if name.ends_with(".monitor") {
        name
    } else {
        format!("{name}.monitor")
    };
    let device = host
        .device_by_id(&DeviceId::new(HostId::PulseAudio, &monitor))
        .ok_or_else(|| anyhow::anyhow!("No monitor source {monitor} for the audio output"))?;
    let config = device.default_input_config()?;
    Ok((device, config))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn system_device(
    _host: &Host,
    _output: Option<&str>,
) -> anyhow::Result<(Device, SupportedStreamConfig)> {
    anyhow::bail!("System audio capture is not supported on this OS")
}
