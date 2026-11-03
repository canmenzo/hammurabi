//! Windows system audio without this process's own sound: WASAPI process loopback, so a voice
//! reading aloud does not end up in the captions and listening need not pause for it. Needs
//! Windows 10 build 20348 or newer; older builds fail to activate and use plain loopback.

use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, anyhow};
use crossbeam_channel::{Sender, bounded};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    BLOB, COINIT_MULTITHREADED, CoInitializeEx, IAgileObject, IAgileObject_Impl,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{IUnknown, Interface, implement};

use super::Failure;
use super::capture::downmix;
use crate::types::{AudioChunk, SourceKind};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// 200 ms, in the 100 ns units of WASAPI.
const BUFFER: i64 = 2_000_000;
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(Sender<()>);

impl IAgileObject_Impl for Activated_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _op: windows_core::Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

/// An audio client on the mix of everything but this process and its children.
fn activate() -> anyhow::Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let mut blob = ManuallyDrop::new(PROPVARIANT::default());
    // SAFETY: a VT_BLOB variant pointing at `params`, which outlives the call; the variant owns
    // nothing, so it is never cleared.
    unsafe {
        let v = &mut *blob.Anonymous.Anonymous;
        v.vt = VT_BLOB;
        v.Anonymous.blob = BLOB {
            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: (&raw mut params).cast(),
        };
    }
    let (tx, rx) = bounded(1);
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
    // SAFETY: the arguments are valid for the call; the system copies the blob.
    let op = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&raw const *blob),
            &handler,
        )
    }?;
    rx.recv_timeout(ACTIVATE_TIMEOUT)
        .map_err(|_| anyhow!("activation timed out"))?;
    let mut status = windows::core::HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: both out pointers are valid; the operation has completed.
    unsafe { op.GetActivateResult(&mut status, &mut unknown) }?;
    status.ok()?;
    Ok(unknown.context("no audio interface")?.cast()?)
}

/// Runs until `stop`. `started` turns true once `ready` has been told it works.
fn capture(
    tx: &Sender<AudioChunk>,
    stop: &AtomicBool,
    ready: &Sender<anyhow::Result<()>>,
    started: &mut bool,
) -> anyhow::Result<()> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: CHANNELS,
        nSamplesPerSec: RATE,
        wBitsPerSample: 32,
        nBlockAlign: CHANNELS * 4,
        nAvgBytesPerSec: RATE * u32::from(CHANNELS) * 4,
        cbSize: 0,
    };
    let client = activate()?;
    // SAFETY: COM calls on a live client with valid arguments; a packet's data is read only
    // between its GetBuffer and ReleaseBuffer, and the event outlives the client's use of it.
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            BUFFER,
            0,
            &format,
            None,
        )?;
        let event = CreateEventW(None, false, false, None)?;
        let result = (|| -> anyhow::Result<()> {
            client.SetEventHandle(event)?;
            let packets: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            *started = true;
            let _ = ready.send(Ok(()));
            while !stop.load(Ordering::Relaxed) {
                if WaitForSingleObject(event, 100) != WAIT_OBJECT_0 {
                    continue;
                }
                while packets.GetNextPacketSize()? > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                    packets.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        vec![0.0; frames as usize]
                    } else {
                        let n = frames as usize * usize::from(CHANNELS);
                        downmix(
                            std::slice::from_raw_parts(data.cast::<f32>(), n),
                            usize::from(CHANNELS),
                        )
                    };
                    packets.ReleaseBuffer(frames)?;
                    // Full channel means the engine is behind; dropping is the intended backpressure.
                    let _ = tx.try_send(AudioChunk {
                        source: SourceKind::System,
                        samples,
                        sample_rate: RATE,
                    });
                }
            }
            let _ = client.Stop();
            Ok(())
        })();
        let _ = CloseHandle(event);
        result
    }
}

/// Captures on its own thread until dropped.
pub struct OwnExcluded {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OwnExcluded {
    /// Err when this Windows cannot leave a process out, for the caller to fall back on.
    pub fn start(tx: Sender<AudioChunk>, failure: Failure) -> anyhow::Result<OwnExcluded> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready) = bounded(1);
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("hm-loopback".into())
            .spawn(move || {
                // SAFETY: called once on this new thread, before any COM use.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut started = false;
                if let Err(e) = capture(&tx, &flag, &ready_tx, &mut started) {
                    if started {
                        log::warn!("System capture lost: {e:#}");
                        let mut f = failure.lock().unwrap_or_else(|p| p.into_inner());
                        f.get_or_insert_with(|| format!("{e:#}"));
                    } else {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })?;
        ready
            .recv_timeout(ACTIVATE_TIMEOUT * 2)
            .map_err(|_| anyhow!("timed out"))??;
        log::info!("System capture: everything but Hammurabi, {RATE} Hz, {CHANNELS} ch");
        Ok(OwnExcluded {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for OwnExcluded {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
