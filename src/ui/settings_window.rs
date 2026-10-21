#![allow(dead_code, unused_variables)]

use eframe::egui::Context;

use crate::audio::DeviceList;
use crate::models::ModelId;
use crate::settings::Settings;

/// Read-only data the settings window displays.
pub struct ModelsStatus {
    pub cached: Vec<ModelId>,
    /// (model, done, total) of the running download.
    pub downloading: Option<(ModelId, u64, Option<u64>)>,
    pub devices: DeviceList,
    /// Compute device label, e.g. "CPU".
    pub compute: String,
}

pub enum SettingsAction {
    Changed,
    Download(ModelId),
    OpenModelsFolder,
    RefreshDevices,
}

#[derive(Default)]
pub struct SettingsWindow {}

impl SettingsWindow {
    /// Draws the immediate child viewport; edits `settings` in place.
    pub fn show(
        &mut self,
        ctx: &Context,
        settings: &mut Settings,
        models: &ModelsStatus,
    ) -> Vec<SettingsAction> {
        Vec::new()
    }
}
