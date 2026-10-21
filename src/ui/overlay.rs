#![allow(dead_code, unused_variables)]

use std::time::Instant;

use eframe::egui::{Pos2, Ui, Vec2, ViewportBuilder};

use crate::settings::{CaptionStyle, Settings};
use crate::types::{CaptionUpdate, Status};

#[derive(Default)]
pub struct OverlayState {}

#[derive(Default)]
pub struct OverlayResponse {
    /// Primary press in move mode: caller sends `ViewportCommand::StartDrag`.
    pub start_drag: bool,
    /// Double-click in move mode: caller leaves move mode and saves the position.
    pub finish_move: bool,
}

impl OverlayState {
    pub fn apply(&mut self, update: CaptionUpdate, settings: &Settings, now: Instant) {}

    pub fn set_status(&mut self, status: Status, now: Instant) {}

    /// Next time a cue or status expires or fades; the caller schedules a repaint for it.
    pub fn next_deadline(&self) -> Option<Instant> {
        None
    }

    pub fn paint(&mut self, ui: &mut Ui, style: &CaptionStyle, move_mode: bool) -> OverlayResponse {
        OverlayResponse::default()
    }

    pub fn is_empty(&self) -> bool {
        true
    }

    pub fn has_status(&self) -> bool {
        false
    }
}

/// Root viewport: borderless, transparent, on top, click-through, no taskbar entry, hidden.
pub fn viewport_builder(settings: &Settings) -> ViewportBuilder {
    ViewportBuilder::default()
}

/// Outer position and inner size (logical px) for a monitor of `monitor` size.
/// `saved` is `Settings::overlay_pos`, which wins over the default bottom-centre spot.
pub fn layout_for_monitor(
    monitor: Vec2,
    style: &CaptionStyle,
    saved: Option<[f32; 2]>,
) -> (Pos2, Vec2) {
    (Pos2::ZERO, monitor)
}
