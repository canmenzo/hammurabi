use eframe::egui::{
    self, Color32, ComboBox, Context, DragValue, ProgressBar, RichText, ScrollArea, Slider, Ui,
};

use crate::audio::{DeviceInfo, DeviceList};
use crate::models::{ModelId, spec};
use crate::settings::{
    Edge, ModelChoice, SOURCE_LANGS, Settings, TARGET_LANGS, TranslatorChoice, lang_name,
};
use crate::types::AudioSource;

const OLLAMA_URL: &str = "http://localhost:11434/v1";
const OLLAMA_MODEL: &str = "qwen2.5:3b";

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
    /// Draws the settings page into the whole of `ctx`; edits `settings` in place.
    pub fn show(
        &mut self,
        ctx: &Context,
        settings: &mut Settings,
        models: &ModelsStatus,
    ) -> Vec<SettingsAction> {
        let mut actions = Vec::new();
        let mut changed = false;
        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::unique("settings_root"))
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                    ui.set_min_size(rect.size() - egui::vec2(36.0, 36.0));
                    ui.set_max_size(rect.size() - egui::vec2(36.0, 36.0));
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            audio(ui, settings, models, &mut changed, &mut actions);
                            speech(ui, settings, models, &mut changed, &mut actions);
                            translation(ui, settings, models, &mut changed, &mut actions);
                            captions(ui, settings, &mut changed);
                            section(ui, "Hotkeys");
                            ui.label(format!("Start / stop captions: {}", settings.hotkey_toggle));
                            ui.label(format!("Move captions: {}", settings.hotkey_move));
                            ui.add_space(12.0);
                            if ui.button("Open models folder").clicked() {
                                actions.push(SettingsAction::OpenModelsFolder);
                            }
                        });
                });
            });
        if changed {
            actions.insert(0, SettingsAction::Changed);
        }
        actions
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title).heading());
    ui.separator();
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, add);
}

fn device_combo(
    ui: &mut Ui,
    id: &str,
    list: &[DeviceInfo],
    value: &mut Option<String>,
    changed: &mut bool,
) {
    let current = match value {
        None => "Default".to_string(),
        Some(v) => list
            .iter()
            .find(|d| &d.id == v)
            .map_or_else(|| format!("{v} (missing)"), |d| d.name.clone()),
    };
    ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(260.0)
        .show_ui(ui, |ui| {
            *changed |= ui.selectable_value(value, None, "Default").changed();
            for d in list {
                *changed |= ui
                    .selectable_value(value, Some(d.id.clone()), &d.name)
                    .changed();
            }
        });
}

fn audio(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, "Audio");
    ui.horizontal(|ui| {
        for (v, label) in [
            (AudioSource::System, "System audio"),
            (AudioSource::Microphone, "Microphone"),
            (AudioSource::Both, "Both"),
        ] {
            *changed |= ui.radio_value(&mut s.source, v, label).changed();
        }
    });
    if let Err(e) = &m.devices.system_audio {
        ui.colored_label(
            Color32::from_rgb(230, 160, 60),
            format!("System audio unavailable: {e}"),
        );
    }
    ui.add_space(4.0);
    grid(ui, "audio", |ui| {
        ui.label("Microphone");
        device_combo(ui, "mic", &m.devices.inputs, &mut s.input_device, changed);
        ui.end_row();
        ui.label("Output device");
        ui.horizontal(|ui| {
            device_combo(ui, "out", &m.devices.outputs, &mut s.output_device, changed);
            if ui.button("Refresh").clicked() {
                actions.push(SettingsAction::RefreshDevices);
            }
        });
        ui.end_row();
    });
}

fn size_text(id: ModelId) -> String {
    format!("{} MB", spec(id).approx_bytes / 1_000_000)
}

/// Check mark, Download button or progress bar for one model.
fn model_status(ui: &mut Ui, id: ModelId, m: &ModelsStatus, actions: &mut Vec<SettingsAction>) {
    ui.horizontal(|ui| {
        ui.label(format!("{} ({})", spec(id).label, size_text(id)));
        match m.downloading {
            Some((d, done, total)) if d == id => {
                let bar = match total {
                    Some(t) if t > 0 => ProgressBar::new(done as f32 / t as f32).show_percentage(),
                    _ => ProgressBar::new(0.0).animate(true),
                };
                ui.add(bar.desired_width(160.0));
            }
            _ if m.cached.contains(&id) => {
                ui.label(
                    RichText::new("\u{2714} Downloaded").color(Color32::from_rgb(90, 190, 110)),
                );
            }
            _ => {
                if ui
                    .add_enabled(m.downloading.is_none(), egui::Button::new("Download"))
                    .clicked()
                {
                    actions.push(SettingsAction::Download(id));
                }
            }
        }
    });
}

fn speech(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, "Speech");
    let gpu = m.compute != "CPU";
    grid(ui, "speech", |ui| {
        ui.label("Spoken language");
        let current = s
            .spoken_language
            .as_deref()
            .map_or("Auto-detect", lang_name)
            .to_string();
        ComboBox::from_id_salt("spoken")
            .selected_text(current)
            .width(200.0)
            .show_ui(ui, |ui| {
                *changed |= ui
                    .selectable_value(&mut s.spoken_language, None, "Auto-detect")
                    .changed();
                for c in SOURCE_LANGS {
                    *changed |= ui
                        .selectable_value(&mut s.spoken_language, Some(c.to_string()), lang_name(c))
                        .changed();
                }
            });
        ui.end_row();

        ui.label("Model");
        let label = |c: ModelChoice| match c {
            ModelChoice::Auto => {
                format!(
                    "Auto ({})",
                    spec(ModelId::whisper(ModelChoice::Auto, gpu)).label
                )
            }
            c => {
                let id = ModelId::whisper(c, gpu);
                format!("{} ({})", spec(id).label, size_text(id))
            }
        };
        ComboBox::from_id_salt("model")
            .selected_text(label(s.model))
            .width(200.0)
            .show_ui(ui, |ui| {
                for c in [
                    ModelChoice::Auto,
                    ModelChoice::Tiny,
                    ModelChoice::Base,
                    ModelChoice::Small,
                    ModelChoice::Turbo,
                ] {
                    *changed |= ui.selectable_value(&mut s.model, c, label(c)).changed();
                }
            });
        ui.end_row();

        ui.label("Status");
        model_status(ui, ModelId::whisper(s.model, gpu), m, actions);
        ui.end_row();

        ui.label("Compute");
        ui.label(&m.compute);
        ui.end_row();
    });
}

fn translation(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, "Translation");
    *changed |= ui
        .checkbox(&mut s.translate, "Translate captions")
        .changed();
    ui.add_enabled_ui(s.translate, |ui| {
        grid(ui, "translation", |ui| {
            ui.label("Translate to");
            ComboBox::from_id_salt("target")
                .selected_text(lang_name(&s.target_language))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for c in TARGET_LANGS {
                        *changed |= ui
                            .selectable_value(&mut s.target_language, c.to_string(), lang_name(c))
                            .changed();
                    }
                });
            ui.end_row();

            ui.label("Engine");
            let mut llm = matches!(s.translator, TranslatorChoice::Llm { .. });
            ui.horizontal(|ui| {
                let a = ui.radio_value(&mut llm, false, "Built-in (MADLAD, 1.7 GB)");
                let b = ui.radio_value(&mut llm, true, "Local LLM");
                if a.changed() || b.changed() {
                    s.translator = if llm {
                        TranslatorChoice::Llm {
                            base_url: OLLAMA_URL.into(),
                            model: OLLAMA_MODEL.into(),
                        }
                    } else {
                        TranslatorChoice::Builtin
                    };
                    *changed = true;
                }
            });
            ui.end_row();

            match &mut s.translator {
                TranslatorChoice::Builtin => {
                    ui.label("Model");
                    model_status(ui, ModelId::Madlad3bQ4k, m, actions);
                    ui.end_row();
                }
                TranslatorChoice::Llm { base_url, model } => {
                    ui.label("Base URL");
                    *changed |= ui
                        .add(egui::TextEdit::singleline(base_url).desired_width(260.0))
                        .changed();
                    ui.end_row();
                    ui.label("Model name");
                    *changed |= ui
                        .add(egui::TextEdit::singleline(model).desired_width(260.0))
                        .changed();
                    ui.end_row();
                }
            }
        });
        *changed |= ui
            .checkbox(&mut s.instant, "Instant (translate the live line)")
            .changed();
    });
}

fn parse_hex(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn color_row(ui: &mut Ui, hex: &mut String, changed: &mut bool) {
    let mut c = parse_hex(hex).unwrap_or(Color32::WHITE);
    if ui.color_edit_button_srgba(&mut c).changed() {
        *hex = format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        *changed = true;
    }
}

fn captions(ui: &mut Ui, s: &mut Settings, changed: &mut bool) {
    section(ui, "Captions");
    let c = &mut s.caption;
    grid(ui, "captions", |ui| {
        ui.label("Translated text");
        ui.horizontal(|ui| {
            *changed |= ui
                .add(Slider::new(&mut c.trans_size, 12.0..=72.0).suffix(" px"))
                .changed();
            color_row(ui, &mut c.trans_color, changed);
        });
        ui.end_row();

        ui.label("Original text");
        ui.horizontal(|ui| {
            *changed |= ui
                .add(Slider::new(&mut c.orig_size, 10.0..=60.0).suffix(" px"))
                .changed();
            color_row(ui, &mut c.orig_color, changed);
        });
        ui.end_row();

        ui.label("Background");
        *changed |= ui
            .add(
                Slider::new(&mut c.bg_opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .changed();
        ui.end_row();

        ui.label("Lines");
        *changed |= ui.add(Slider::new(&mut c.cues, 1..=3)).changed();
        ui.end_row();

        ui.label("Hold time");
        *changed |= ui
            .add(
                DragValue::new(&mut c.hold_secs)
                    .range(1.0..=15.0)
                    .speed(0.1)
                    .suffix(" s"),
            )
            .changed();
        ui.end_row();

        ui.label("Edge");
        ui.horizontal(|ui| {
            for (v, label) in [
                (Edge::None, "None"),
                (Edge::Shadow, "Shadow"),
                (Edge::Outline, "Outline"),
            ] {
                *changed |= ui.radio_value(&mut c.edge, v, label).changed();
            }
        });
        ui.end_row();
    });
    *changed |= ui
        .checkbox(&mut c.show_original, "Show original text")
        .changed();
    ui.add_enabled_ui(c.show_original, |ui| {
        *changed |= ui
            .checkbox(&mut c.original_first, "Original first")
            .changed();
    });
}
