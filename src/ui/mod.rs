#![allow(dead_code, unused_variables)]

pub mod hotkey;
pub mod icons;
pub mod overlay;
pub mod platform;
pub mod settings_window;
pub mod tray;

use crate::settings::Settings;

pub struct App {
    settings: Settings,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        Self { settings }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {}

    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}
