pub mod languages;
#[cfg(all(windows, feature = "win-livecaptions"))]
pub mod livecaptions;
pub mod mel;
pub mod whisper;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};

use crate::models::{self, ModelId};
use crate::settings::ModelChoice;
use crate::types::{AsrEvent, AsrRequest, AsrResult, Progress, ProgressPhase, Recognizer};
use whisper::WhisperEngine;

/// Thread "hm-asr": ensures + loads the model, then serves requests one at a time.
/// Dropping it cancels a running download and ends the thread.
pub struct AsrWorker {
    pub events: Receiver<AsrEvent>,
    requests: Sender<AsrRequest>,
    cancel: Arc<AtomicBool>,
}

impl AsrWorker {
    pub fn spawn(choice: ModelChoice, trim: bool) -> AsrWorker {
        let (ev_tx, events) = crossbeam_channel::unbounded();
        let (requests, req_rx) = crossbeam_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::Builder::new()
            .name("hm-asr".into())
            .spawn(move || run(choice, trim, req_rx, ev_tx, &c))
            .expect("spawn hm-asr");
        AsrWorker {
            events,
            requests,
            cancel,
        }
    }

    pub fn submit(&self, req: AsrRequest) {
        let _ = self.requests.send(req);
    }
}

impl Drop for AsrWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn load(
    choice: ModelChoice,
    tx: &Sender<AsrEvent>,
    cancel: &AtomicBool,
) -> anyhow::Result<WhisperEngine> {
    let (device, label) = crate::device::best_device(true);
    let model = ModelId::whisper(choice, !device.is_cpu());
    let progress = |phase, done, total| Progress {
        model,
        phase,
        done,
        total,
    };
    let engine = models::ensure_and_load(
        model,
        &mut |done, total| {
            let _ = tx.send(AsrEvent::Progress(progress(
                ProgressPhase::Downloading,
                done,
                total,
            )));
        },
        cancel,
        |paths| {
            let _ = tx.send(AsrEvent::Progress(progress(
                ProgressPhase::Loading,
                0,
                None,
            )));
            WhisperEngine::load(paths, &device)
        },
    )?;
    let _ = tx.send(AsrEvent::Ready {
        model: models::spec(model).label.to_string(),
        device: label,
    });
    Ok(engine)
}

fn run(
    choice: ModelChoice,
    trim: bool,
    rx: Receiver<AsrRequest>,
    tx: Sender<AsrEvent>,
    cancel: &AtomicBool,
) {
    let mut engine = match load(choice, &tx, cancel) {
        Ok(mut e) => {
            e.trim = trim;
            Some(e)
        }
        Err(e) => {
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(AsrEvent::Error(format!("{e:#}")));
            }
            None
        }
    };
    // Without a model every request still gets an (error) result so the caller's in-flight slot clears.
    for req in rx {
        let t0 = Instant::now();
        let r = match engine.as_mut() {
            Some(e) => e.transcribe(&req.audio, req.language.as_deref(), &req.prior),
            None => Err(anyhow::anyhow!("speech model not loaded")),
        };
        let ms = t0.elapsed().as_millis() as u32;
        let result = match r {
            Ok(t) => AsrResult {
                id: req.id,
                text: t.text,
                language: t.language,
                no_speech: t.no_speech,
                lang_conf: t.lang_conf,
                ms,
                error: None,
            },
            Err(e) => AsrResult {
                id: req.id,
                text: String::new(),
                language: req.language.unwrap_or_default(),
                no_speech: 1.0,
                lang_conf: 0.0,
                ms,
                error: Some(format!("{e:#}")),
            },
        };
        if tx.send(AsrEvent::Result(result)).is_err() {
            break;
        }
    }
}
