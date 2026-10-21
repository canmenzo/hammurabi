#![allow(dead_code, unused_variables)]

pub mod llm;
pub mod madlad;

use crossbeam_channel::Receiver;

use crate::settings::TranslatorChoice;
use crate::types::{TranslateRequest, TranslatorEvent};

/// Thread "hm-tr": ensures MADLAD or pings the LLM endpoint, then serves requests.
pub struct TranslatorWorker {
    pub events: Receiver<TranslatorEvent>,
}

impl TranslatorWorker {
    pub fn spawn(choice: TranslatorChoice) -> TranslatorWorker {
        let (_tx, events) = crossbeam_channel::unbounded();
        TranslatorWorker { events }
    }

    pub fn submit(&self, req: TranslateRequest) {}
}
