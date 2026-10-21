#![allow(dead_code, unused_variables)]

pub mod languages;
#[cfg(all(windows, feature = "win-livecaptions"))]
pub mod livecaptions;
pub mod mel;
pub mod whisper;

use crossbeam_channel::Receiver;

use crate::settings::ModelChoice;
use crate::types::{AsrEvent, AsrRequest};

/// Thread "hm-asr": ensures + loads the model, then serves requests.
pub struct AsrWorker {
    pub events: Receiver<AsrEvent>,
}

impl AsrWorker {
    pub fn spawn(choice: ModelChoice) -> AsrWorker {
        let (_tx, events) = crossbeam_channel::unbounded();
        AsrWorker { events }
    }

    pub fn submit(&self, req: AsrRequest) {}
}
