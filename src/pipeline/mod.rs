#![allow(dead_code, unused_variables)]

pub mod assembler;
pub mod scheduler;
pub mod segmenter;
pub mod text;

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::types::{EngineCommand, EngineEvent, Notifier};

/// Thread "hm-engine".
pub struct Engine;

impl Engine {
    /// `notifier` is called after every event is queued (= `ctx.request_repaint()`).
    pub fn spawn(notifier: Notifier) -> EngineHandle {
        let (cmd_tx, _cmd_rx) = crossbeam_channel::unbounded();
        let (_event_tx, event_rx) = crossbeam_channel::unbounded();
        EngineHandle { cmd_tx, event_rx }
    }
}

pub struct EngineHandle {
    cmd_tx: Sender<EngineCommand>,
    event_rx: Receiver<EngineEvent>,
}

impl EngineHandle {
    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Sends Shutdown and joins the engine thread, giving up after `timeout`.
    pub fn shutdown(self, timeout: Duration) {}
}
