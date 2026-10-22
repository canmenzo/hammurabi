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
    {
        check_macos_version()?;
        check_permission(crate::types::SourceKind::System)?;
    }
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

/// Fails when the user denied the permission `source` needs. Without it macOS delivers
/// silence instead of an error, so this is the only signal.
#[cfg(target_os = "macos")]
pub fn check_permission(source: crate::types::SourceKind) -> anyhow::Result<()> {
    use crate::types::SourceKind;
    let (service, msg) = match source {
        SourceKind::System => (
            c"kTCCServiceAudioCapture",
            "Hammurabi is not allowed to record system audio. Turn it on in System Settings > Privacy & Security > Screen & System Audio Recording (under System Audio Recording Only), then start captions again.",
        ),
        SourceKind::Microphone => (
            c"kTCCServiceMicrophone",
            "Hammurabi is not allowed to use the microphone. Turn it on in System Settings > Privacy & Security > Microphone, then start captions again.",
        ),
    };
    if tcc_preflight(service) == Some(1) {
        anyhow::bail!(msg);
    }
    Ok(())
}

/// TCCAccessPreflight from the private TCC framework: 0 allowed, 1 denied, 2 not asked yet.
/// None when the symbol is missing.
#[cfg(target_os = "macos")]
fn tcc_preflight(service: &std::ffi::CStr) -> Option<i32> {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            s: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }
    type Preflight = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;
    // SAFETY: plain C calls; the symbol has this signature on macOS 14 and newer.
    unsafe {
        let lib = dlopen(
            c"/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC".as_ptr(),
            1, // RTLD_LAZY
        );
        if lib.is_null() {
            return None;
        }
        let sym = dlsym(lib, c"TCCAccessPreflight".as_ptr());
        if sym.is_null() {
            return None;
        }
        let preflight: Preflight = std::mem::transmute(sym);
        let service = CFStringCreateWithCString(
            std::ptr::null(),
            service.as_ptr(),
            0x0800_0100, // kCFStringEncodingUTF8
        );
        let r = preflight(service, std::ptr::null());
        CFRelease(service);
        Some(r)
    }
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
