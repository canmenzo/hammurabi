use eframe::egui::{
    self, Align, Color32, ComboBox, Context, CursorIcon, Event, FontId, Key, Layout, Modifiers,
    ProgressBar, Rect, RichText, ScrollArea, Sense, Slider, Stroke, TextStyle, Theme, Ui,
    UiBuilder, Visuals, WidgetInfo, pos2, vec2,
};

use super::icons;

use crate::audio::{DeviceInfo, DeviceList};
use crate::i18n::{self, LANGS, T, t, tf};
use crate::models::{ModelId, spec};
use crate::settings::{
    AsrBackend, CaptionStyle, Edge, ModelChoice, SOURCE_LANGS, Settings, TARGET_LANGS, ThemeChoice,
    TranslatorChoice, lang_name,
};
use crate::tts::{self, Persona, Style, Voice, supertonic};
use crate::types::AudioSource;

const OLLAMA_URL: &str = "http://localhost:11434/v1";
const OLLAMA_MODEL: &str = "qwen2.5:3b";
const SIDEBAR: f32 = 150.0;
const WARN: Color32 = Color32::from_rgb(230, 160, 60);

/// Read-only data the settings window displays.
pub struct ModelsStatus {
    pub cached: Vec<ModelId>,
    /// (model, done, total) of each running download.
    pub downloading: Vec<(ModelId, u64, Option<u64>)>,
    pub devices: DeviceList,
    /// Compute device label, e.g. "CPU".
    pub compute: String,
    /// The graphics card, whatever this build computes on.
    pub gpu: Option<crate::device::Gpu>,
    /// How long the speech model takes over a run and the translator over a caption, in
    /// milliseconds, smoothed. None while captions are off.
    pub asr_ms: Option<f32>,
    pub tr_ms: Option<f32>,
    /// Real OS state, not a setting.
    pub autostart: bool,
    pub voices: Vec<Voice>,
    pub running: bool,
    pub loading: bool,
    /// Latest status line, and whether it is a warning or an error.
    pub status: String,
    pub alert: bool,
    /// The voice reading right now.
    pub reading: Option<String>,
    /// The persona whose sample is playing.
    pub previewing: Option<&'static str>,
    /// Speaker's voice has learned a voice to read in.
    pub voice_learned: bool,
    pub update: super::update::Status,
}

pub enum SettingsAction {
    Changed,
    Download(ModelId),
    DeleteModel(ModelId),
    OpenModelsFolder,
    RefreshDevices,
    ToggleAutostart,
    OpenLogs,
    /// Read the persona's sample aloud.
    Preview(&'static str),
    StopPreview,
    ToggleRunning,
    ResetPosition,
    #[cfg(omnivoice)]
    RelearnVoice,
    CheckUpdates,
    OpenUpdate,
    /// Install the CUDA build over this one.
    SwitchToGpu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Listen,
    Translate,
    ReadAloud,
    Captions,
    General,
}

const PAGES: [(Page, T); 5] = [
    (Page::Listen, T::PageListen),
    (Page::Translate, T::PageTranslate),
    (Page::ReadAloud, T::PageReadAloud),
    (Page::Captions, T::PageCaptions),
    (Page::General, T::PageGeneral),
];

#[derive(Default)]
pub struct SettingsWindow {
    pub page: Page,
    /// First launch: explain where the app lives.
    pub welcome: bool,
    /// The shortcut is waiting for its new key press.
    recording: bool,
}

/// The website's colors and a little more room, for both OS themes.
pub fn apply_theme(ctx: &Context) {
    let rgb = Color32::from_rgb;
    let palettes = [
        (
            Theme::Dark,
            Visuals::dark(),
            rgb(24, 24, 24),
            rgb(31, 31, 31),
            rgb(49, 49, 49),
            rgb(94, 234, 212),
            rgb(19, 18, 9),
            rgb(208, 205, 195),
            rgb(150, 148, 138),
        ),
        (
            Theme::Light,
            Visuals::light(),
            rgb(255, 255, 255),
            rgb(244, 244, 242),
            rgb(226, 224, 216),
            rgb(15, 118, 110),
            rgb(255, 255, 255),
            rgb(60, 58, 50),
            rgb(92, 90, 82),
        ),
    ];
    for (theme, mut v, bg, surface, border, accent, on_accent, text, weak) in palettes {
        v.panel_fill = bg;
        v.window_fill = surface;
        v.faint_bg_color = surface;
        v.hyperlink_color = accent;
        v.selection.bg_fill = accent;
        v.selection.stroke = Stroke::new(1.0, on_accent);
        v.slider_trailing_fill = true;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
        v.widgets.noninteractive.fg_stroke.color = text;
        v.weak_text_color = Some(weak);
        v.window_stroke = Stroke::new(1.0, border);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = 6.into();
        }
        ctx.set_visuals_of(theme, v);
    }
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        for (style, size) in [
            (TextStyle::Body, 14.0),
            (TextStyle::Button, 14.0),
            (TextStyle::Small, 12.0),
            (TextStyle::Heading, 16.0),
        ] {
            if let Some(font) = s.text_styles.get_mut(&style) {
                font.size = size;
            }
        }
    });
}

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
        self.record_hotkey(ctx, settings, &mut changed);
        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::unique("settings_root"))
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                let frame = egui::Frame::central_panel(ui.style());
                let inner = rect.size() - frame.total_margin().sum();
                frame.show(ui, |ui| {
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    let area = ui.max_rect();
                    let (nav, body) = area.split_left_right_at_x(area.left() + SIDEBAR);
                    ui.scope_builder(UiBuilder::new().max_rect(nav.shrink(4.0)), |ui| {
                        self.sidebar(ui, models, &mut actions);
                    });
                    let x = body.left() - 1.0;
                    ui.painter().vline(
                        x,
                        area.y_range(),
                        ui.visuals().widgets.noninteractive.bg_stroke,
                    );
                    let body = body.with_min_x(body.left() + 8.0);
                    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                        self.header(ui, settings, models, &mut actions);
                        ui.separator();
                        ScrollArea::vertical()
                            .id_salt(self.page as u8)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width() - 12.0);
                                let (s, m, c, a) = (settings, models, &mut changed, &mut actions);
                                match self.page {
                                    Page::Listen => listen(ui, s, m, c, a),
                                    Page::Translate => translation(ui, s, m, c, a),
                                    Page::ReadAloud => read_aloud(ui, s, m, c, a),
                                    Page::Captions => captions(ui, s, c, a),
                                    Page::General => self.general(ui, s, m, c, a),
                                }
                                ui.add_space(8.0);
                            });
                    });
                });
            });
        if changed {
            actions.insert(0, SettingsAction::Changed);
        }
        actions
    }

    fn sidebar(&mut self, ui: &mut Ui, m: &ModelsStatus, actions: &mut Vec<SettingsAction>) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(t(T::SettingsHeading)).strong().size(16.0));
        });
        ui.add_space(10.0);
        for (page, label) in PAGES {
            if nav_item(ui, t(label), self.page == page) {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.weak(concat!("v", env!("CARGO_PKG_VERSION")));
            });
            if let Some(text) = update_text(&m.update) {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    if let super::update::Status::Available(_) = m.update {
                        if ui.link(text).clicked() {
                            actions.push(SettingsAction::OpenUpdate);
                        }
                    } else {
                        ui.label(
                            RichText::new(text)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
            }
        });
    }

    /// Captions on or off, the button for it, and what is going on now.
    fn header(
        &mut self,
        ui: &mut Ui,
        s: &Settings,
        m: &ModelsStatus,
        actions: &mut Vec<SettingsAction>,
    ) {
        ui.add_space(2.0);
        let accent = ui.visuals().selection.bg_fill;
        let (dot, state) = match (m.running, m.loading) {
            (false, _) => (ui.visuals().weak_text_color(), T::CaptionsOff),
            (true, true) => (WARN, T::GettingReady),
            (true, false) => (accent, T::CaptionsOn),
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, dot);
            ui.label(RichText::new(t(state)).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let button = if m.running {
                    egui::Button::new(t(T::StopCaptions))
                } else {
                    let text = ui.visuals().selection.stroke.color;
                    egui::Button::new(RichText::new(t(T::StartCaptions)).strong().color(text))
                        .fill(accent)
                };
                if ui
                    .add(button)
                    .on_hover_text(hotkey_label(&s.hotkey_toggle))
                    .clicked()
                {
                    actions.push(SettingsAction::ToggleRunning);
                    self.welcome = false;
                }
            });
        });
        if self.welcome {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(6)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width() - 8.0);
                    ui.label(t(T::Welcome));
                    if ui.button(t(T::GotIt)).clicked() {
                        self.welcome = false;
                    }
                });
        }
        if !m.status.is_empty() {
            let text = RichText::new(&m.status);
            ui.add(
                egui::Label::new(if m.alert {
                    text.color(WARN)
                } else {
                    text.weak()
                })
                .wrap(),
            );
        }
        if !m.running {
            let pending = pending_downloads(s, m);
            if !pending.is_empty() {
                ui.weak(download_note(&pending));
            }
        }
    }

    /// While recording, the next key press with a modifier becomes the shortcut.
    fn record_hotkey(&mut self, ctx: &Context, s: &mut Settings, changed: &mut bool) {
        // Off its page, or with the window closing, the next key press is meant for something else.
        if self.page != Page::General || ctx.input(|i| i.viewport().close_requested()) {
            self.recording = false;
        }
        if !self.recording {
            return;
        }
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, mods)) = pressed else {
            return;
        };
        if key == Key::Escape {
            self.recording = false;
            return;
        }
        let Some(text) = hotkey_text(mods, key) else {
            return;
        };
        s.hotkey_toggle = text;
        *changed = true;
        self.recording = false;
    }

    fn hotkey_row(&mut self, ui: &mut Ui, label: &str, value: &str) {
        ui.label(label);
        let recording = self.recording;
        let text = if recording {
            t(T::PressKeys).to_string()
        } else {
            hotkey_label(value)
        };
        let hint = t(if recording {
            T::HotkeyRules
        } else {
            T::HotkeyClick
        });
        if ui
            .add(egui::Button::new(text).selected(recording))
            .on_hover_text(hint)
            .clicked()
        {
            self.recording = !recording;
        }
        ui.end_row();
    }

    fn general(
        &mut self,
        ui: &mut Ui,
        s: &mut Settings,
        m: &ModelsStatus,
        changed: &mut bool,
        actions: &mut Vec<SettingsAction>,
    ) {
        section(ui, t(T::SectionApp));
        grid(ui, "app", |ui| {
            ui.label(t(T::Language));
            language_combo(ui, &mut s.ui_language, changed);
            ui.end_row();

            ui.label(t(T::Theme));
            ui.horizontal(|ui| {
                for (v, label) in [
                    (ThemeChoice::System, T::ThemeSystem),
                    (ThemeChoice::Dark, T::ThemeDark),
                    (ThemeChoice::Light, T::ThemeLight),
                ] {
                    *changed |= ui.radio_value(&mut s.theme, v, t(label)).changed();
                }
            });
            ui.end_row();
        });
        let mut autostart = m.autostart;
        if ui
            .checkbox(&mut autostart, t(T::StartAtLoginLong))
            .changed()
        {
            actions.push(SettingsAction::ToggleAutostart);
        }
        // The Microsoft Store updates a copy installed from it.
        if !super::platform::packaged() {
            *changed |= ui
                .checkbox(&mut s.check_updates, t(T::UpdateAutomatically))
                .on_hover_text(t(T::UpdateAutomaticallyHint))
                .changed();
            ui.horizontal_wrapped(|ui| {
                let busy = matches!(
                    m.update,
                    super::update::Status::Checking | super::update::Status::Downloading(..)
                );
                if ui
                    .add_enabled(!busy, egui::Button::new(t(T::CheckUpdates)))
                    .clicked()
                {
                    actions.push(SettingsAction::CheckUpdates);
                }
                if let Some(text) = update_text(&m.update) {
                    ui.weak(text);
                }
            });
        }

        section(ui, t(T::Shortcuts));
        let toggle = s.hotkey_toggle.clone();
        grid(ui, "hotkeys", |ui| {
            self.hotkey_row(ui, t(T::StartStopCaptions), &toggle);
        });

        section(ui, t(T::Models));
        // The Natural translator is offered on Windows only.
        let translation: &[ModelId] = if cfg!(windows) {
            &[ModelId::Madlad3bQ4k, ModelId::Gemma12b, ModelId::GemmaE4b]
        } else {
            &[ModelId::Madlad3bQ4k]
        };
        let groups: [(T, &[ModelId]); 3] = [
            (
                T::GroupSpeech,
                &[
                    ModelId::WhisperTiny,
                    ModelId::WhisperBase,
                    ModelId::WhisperSmall,
                    ModelId::WhisperTurbo,
                    ModelId::WhisperLarge,
                ],
            ),
            (T::GroupTranslation, translation),
            (
                T::GroupVoices,
                &[
                    ModelId::Supertonic,
                    #[cfg(omnivoice)]
                    ModelId::OmniVoice,
                ],
            ),
        ];
        gpu_build(ui, m, actions);
        let recommended = recommended(m);
        let accent = ui.visuals().selection.bg_fill;
        grid(ui, "models", |ui| {
            for (heading, ids) in groups {
                ui.weak(t(heading));
                ui.end_row();
                for &id in ids {
                    let size = size_label(spec(id).approx_bytes);
                    ui.horizontal(|ui| {
                        ui.label(format!("{} ({size})", model_name(id)));
                        if recommended.contains(&id) {
                            ui.label(RichText::new(t(T::Recommended)).small().color(accent));
                        }
                    });
                    ui.horizontal(|ui| {
                        // Read aloud asks for its licence before it offers the download.
                        #[cfg(omnivoice)]
                        if id == ModelId::OmniVoice
                            && !s.omnivoice_accepted
                            && !m.cached.contains(&id)
                        {
                            ui.weak(t(T::AgreeLicenceFirst));
                            return;
                        }
                        model_state(ui, id, m, in_use(id, s, m), actions);
                    });
                    ui.end_row();
                }
            }
        });

        section(ui, t(T::Files));
        ui.horizontal(|ui| {
            if ui.button(t(T::OpenModelsFolder)).clicked() {
                actions.push(SettingsAction::OpenModelsFolder);
            }
            if ui.button(t(T::OpenLogs)).clicked() {
                actions.push(SettingsAction::OpenLogs);
            }
        });
    }
}

/// The app's language, each in its own name; the first entry follows the system.
fn language_combo(ui: &mut Ui, value: &mut Option<i18n::Lang>, changed: &mut bool) {
    let system = tf(T::SystemLanguage, &[&i18n::system().native_name()]);
    let current = value.map_or_else(|| system.clone(), |l| l.native_name().to_string());
    ComboBox::from_id_salt("ui-language")
        .selected_text(current)
        .width(200.0)
        .show_ui(ui, |ui| {
            *changed |= ui.selectable_value(value, None, system).changed();
            for l in LANGS {
                *changed |= ui
                    .selectable_value(value, Some(l), l.native_name())
                    .changed();
            }
        });
}

fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Tab, true, selected, label));
    let v = ui.visuals();
    let (accent, text) = (v.selection.bg_fill, v.text_color());
    let weak = v.weak_text_color();
    if selected {
        ui.painter()
            .rect_filled(rect, 6, accent.gamma_multiply(0.14));
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 7.0), vec2(3.0, 16.0));
        ui.painter().rect_filled(bar, 2, accent);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6, v.faint_bg_color);
    }
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        if selected || resp.hovered() {
            text
        } else {
            weak
        },
    );
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).heading().strong());
    ui.add_space(2.0);
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, add);
}

/// The models the first start of captions has to download.
pub fn pending_downloads(s: &Settings, m: &ModelsStatus) -> Vec<ModelId> {
    let lc = cfg!(all(windows, feature = "win-livecaptions"))
        && s.asr_backend == AsrBackend::WindowsLiveCaptions;
    let screen = cfg!(windows) && s.asr_backend == AsrBackend::ScreenText;
    let mut out = Vec::new();
    if !lc && !screen {
        out.push(ModelId::whisper(s.model, m.compute != "CPU"));
    }
    if s.translate {
        match s.translator {
            TranslatorChoice::Builtin => out.push(ModelId::Madlad3bQ4k),
            TranslatorChoice::Natural { light } => out.push(ModelId::natural(light)),
            TranslatorChoice::Llm { .. } => {}
        }
    }
    out.retain(|id| !m.cached.contains(id) && !m.downloading.iter().any(|d| d.0 == *id));
    out
}

fn download_note(pending: &[ModelId]) -> String {
    let translation = |id: &ModelId| {
        matches!(
            id,
            ModelId::Madlad3bQ4k | ModelId::Gemma12b | ModelId::GemmaE4b
        )
    };
    let key = match (
        pending.iter().any(translation),
        pending.iter().all(translation),
    ) {
        (true, true) => T::DownloadNoteTranslation,
        (true, false) => T::DownloadNoteBoth,
        (false, _) => T::DownloadNoteSpeech,
    };
    let bytes: u64 = pending.iter().map(|&id| spec(id).approx_bytes).sum();
    tf(key, &[&size_label(bytes)])
}

fn device_combo(
    ui: &mut Ui,
    id: &str,
    list: &[DeviceInfo],
    value: &mut Option<String>,
    changed: &mut bool,
) {
    let current = match value {
        None => t(T::Default).to_string(),
        Some(v) => list
            .iter()
            .find(|d| &d.id == v)
            .map_or_else(|| tf(T::Missing, &[v]), |d| d.name.clone()),
    };
    ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(240.0)
        .show_ui(ui, |ui| {
            *changed |= ui.selectable_value(value, None, t(T::Default)).changed();
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
    section(ui, t(T::WhatToCaption));
    ui.horizontal(|ui| {
        for (v, label) in [
            (AudioSource::System, T::SystemAudio),
            (AudioSource::Microphone, T::Microphone),
            (AudioSource::Both, T::Both),
        ] {
            *changed |= ui.radio_value(&mut s.source, v, t(label)).changed();
        }
    });
    if let Err(e) = &m.devices.system_audio {
        ui.colored_label(WARN, tf(T::SystemAudioUnavailable, &[e]));
    }
    ui.add_space(4.0);
    grid(ui, "audio", |ui| {
        ui.add_enabled_ui(s.source != AudioSource::Microphone, |ui| {
            ui.label(t(T::SystemAudioFrom))
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(s.source != AudioSource::Microphone, |ui| {
                device_combo(ui, "out", &m.devices.outputs, &mut s.output_device, changed);
            });
            if ui.button(t(T::Refresh)).clicked() {
                actions.push(SettingsAction::RefreshDevices);
            }
        });
        ui.end_row();
        ui.add_enabled_ui(s.source != AudioSource::System, |ui| {
            ui.label(t(T::MicOrCaptureCard))
        });
        ui.add_enabled_ui(s.source != AudioSource::System, |ui| {
            device_combo(ui, "mic", &m.devices.inputs, &mut s.input_device, changed);
        });
        ui.end_row();
    });
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{} GB", i18n::decimal(bytes as f64 / 1e9, 1))
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

/// The name a model goes by in the app.
fn model_name(id: ModelId) -> &'static str {
    match id {
        ModelId::Madlad3bQ4k => t(T::FastTranslator),
        ModelId::Gemma12b => t(T::NaturalTranslator),
        ModelId::GemmaE4b => t(T::NaturalTranslatorSmaller),
        ModelId::Supertonic => t(T::NaturalVoices),
        #[cfg(omnivoice)]
        ModelId::OmniVoice => t(T::SpeakersVoice),
        _ => spec(id).label,
    }
}

/// While captions run, a model that is not downloaded cannot be picked: that would stop them
/// for a download of gigabytes.
fn can_pick(id: ModelId, m: &ModelsStatus) -> bool {
    !(m.running || m.loading) || m.cached.contains(&id)
}

/// Whether picking the Natural translator chooses its smaller model: only when the larger
/// cannot be picked. None when neither can.
fn natural_light(m: &ModelsStatus) -> Option<bool> {
    if can_pick(ModelId::natural(false), m) {
        Some(false)
    } else {
        can_pick(ModelId::natural(true), m).then_some(true)
    }
}

/// The language of the subtitles, where they are read from, and the part of that picture they
/// are in.
#[cfg(windows)]
fn screen_rows(ui: &mut Ui, s: &mut Settings, changed: &mut bool) {
    // No auto-detect when reading: none chosen is English.
    ui.label(t(T::SubtitleLanguage));
    let current = s.spoken_language.clone().unwrap_or_else(|| "en".into());
    ComboBox::from_id_salt("subtitle-language")
        .selected_text(lang_name(&current))
        .width(200.0)
        .show_ui(ui, |ui| {
            for c in SOURCE_LANGS {
                if ui.selectable_label(current == c, lang_name(c)).clicked() && current != c {
                    s.spoken_language = Some(c.to_string());
                    *changed = true;
                }
            }
        });
    ui.end_row();

    // Windows is asked once per language, not every frame, and again every five seconds while
    // it cannot read it: the language may have been added in Windows since.
    let lang = s.spoken_language.as_deref().unwrap_or("en");
    let key = egui::Id::unique("ocr-reads");
    let now = ui.input(|i| i.time);
    let known = ui
        .data(|d| d.get_temp::<(String, bool, f64)>(key))
        .filter(|(l, reads, at)| l == lang && (*reads || now - at < 5.0));
    let reads = match known {
        Some((_, reads, _)) => reads,
        None => {
            let reads = crate::screen::ocr::reads(lang);
            ui.data_mut(|d| d.insert_temp(key, (lang.to_string(), reads, now)));
            reads
        }
    };
    if !reads {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(5));
        ui.label("");
        let text = tf(T::WindowsCannotRead, &[&lang_name(lang)]);
        ui.add(egui::Label::new(RichText::new(text).color(WARN)).wrap());
        ui.end_row();
    }

    ui.label(t(T::ReadFrom));
    let current = if s.screen_window.is_empty() {
        t(T::MainDisplay).to_string()
    } else {
        s.screen_window.clone()
    };
    ComboBox::from_id_salt("screen-window")
        .selected_text(current)
        .width(260.0)
        .show_ui(ui, |ui| {
            *changed |= ui
                .selectable_value(&mut s.screen_window, String::new(), t(T::MainDisplay))
                .changed();
            for (_, title) in crate::screen::windows::list() {
                let short: String = title.chars().take(60).collect();
                *changed |= ui
                    .selectable_value(&mut s.screen_window, title, short)
                    .changed();
            }
        })
        .response
        .on_hover_text(t(T::ReadFromHint));
    ui.end_row();

    ui.label(t(T::SubtitleArea));
    ui.horizontal(|ui| {
        let (mut top, mut bottom) = (s.screen_top * 100.0, s.screen_bottom * 100.0);
        ui.label(t(T::AreaFrom));
        let a = ui.add(
            egui::Slider::new(&mut top, 0.0..=95.0)
                .fixed_decimals(0)
                .suffix("%"),
        );
        ui.label(t(T::AreaTo));
        let b = ui.add(
            egui::Slider::new(&mut bottom, 5.0..=100.0)
                .fixed_decimals(0)
                .suffix("%"),
        );
        if a.changed() || b.changed() {
            s.screen_top = (top / 100.0).min(0.95);
            s.screen_bottom = (bottom / 100.0).max(s.screen_top + 0.05);
            *changed = true;
        }
    })
    .response
    .on_hover_text(t(T::SubtitleAreaHint));
    ui.end_row();

    ui.label("");
    screen_preview(ui, s, changed);
    ui.end_row();
}

/// What is read, as a small picture with the subtitle area lit: its edges can be dragged. The
/// picture and the line under it are a row of the grid each, as a tall cell of several things
/// leaves the rows after it too much room.
#[cfg(windows)]
fn screen_preview(ui: &mut Ui, s: &mut Settings, changed: &mut bool) {
    use crate::screen::preview::Preview;
    use crossbeam_channel::TryRecvError;

    struct Live {
        window: String,
        preview: Preview,
        texture: Option<egui::TextureHandle>,
        problem: Option<String>,
        /// The edge being dragged: left, top, right or bottom.
        edge: usize,
    }
    // One settings window, and its pages are drawn by functions that keep nothing.
    static LIVE: std::sync::Mutex<Option<Live>> = std::sync::Mutex::new(None);

    let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    if live.as_ref().is_some_and(|l| l.window != s.screen_window) {
        *live = None;
    }
    let l = live.get_or_insert_with(|| {
        let (ctx, id) = (ui.ctx().clone(), ui.ctx().viewport_id());
        Live {
            window: s.screen_window.clone(),
            preview: Preview::spawn(s.screen_window.clone(), move || ctx.request_repaint_of(id)),
            texture: None,
            problem: None,
            edge: 1,
        }
    });
    l.preview.seen();
    let ended = loop {
        match l.preview.pictures.try_recv() {
            Ok(Ok(p)) => {
                let image = egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba);
                l.texture = Some(ui.ctx().load_texture(
                    "screen-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
                l.problem = None;
            }
            Ok(Err(e)) => l.problem = Some(e),
            Err(TryRecvError::Empty) => break false,
            Err(TryRecvError::Disconnected) => break true,
        }
    };
    match (&l.problem, &l.texture) {
        (Some(problem), _) => {
            ui.colored_label(WARN, problem);
        }
        (None, None) => {
            ui.weak(t(T::WaitingForPicture));
        }
        (None, Some(texture)) => {
            let full = texture.size_vec2();
            let size = full * (400.0 / full.x).min(1.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
            // The lit part in the picture's points.
            let lit = |s: &Settings| {
                Rect::from_min_max(
                    rect.lerp_inside(vec2(s.screen_left, s.screen_top)),
                    rect.lerp_inside(vec2(s.screen_right, s.screen_bottom)),
                )
            };
            if let Some(p) = resp.interact_pointer_pos() {
                let was = s.screen_area();
                if resp.drag_started() {
                    let r = lit(s);
                    let far = [
                        p.x - r.left(),
                        p.y - r.top(),
                        p.x - r.right(),
                        p.y - r.bottom(),
                    ];
                    l.edge = (0..4)
                        .min_by(|a, b| far[*a].abs().total_cmp(&far[*b].abs()))
                        .unwrap_or(1);
                }
                // To a whole percent, as the sliders give.
                let at = ((p - rect.min) / rect.size() * 100.0).round() / 100.0;
                match l.edge {
                    0 => s.screen_left = at.x.clamp(0.0, s.screen_right - 0.2),
                    1 => s.screen_top = at.y.clamp(0.0, s.screen_bottom - 0.05),
                    2 => s.screen_right = at.x.clamp(s.screen_left + 0.2, 1.0),
                    _ => s.screen_bottom = at.y.clamp(s.screen_top + 0.05, 1.0),
                }
                *changed |= was != s.screen_area();
            }
            let painter = ui.painter_at(rect);
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, whole, Color32::WHITE);
            let r = lit(s);
            let shade = Color32::from_black_alpha(170);
            for dark in [
                rect.with_max_y(r.top()),
                rect.with_min_y(r.bottom()),
                r.with_min_x(rect.left()).with_max_x(r.left()),
                r.with_min_x(r.right()).with_max_x(rect.right()),
            ] {
                painter.rect_filled(dark, 0, shade);
            }
            painter.rect_stroke(
                r,
                0,
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
                egui::StrokeKind::Inside,
            );
            resp.on_hover_cursor(CursorIcon::Grab);
            ui.end_row();
            ui.label("");
            // A label in a grid is one line unless it is told to wrap.
            ui.add(egui::Label::new(RichText::new(t(T::LitPartNote)).weak()).wrap());
        }
    }
    if ended {
        // It ended while this page was not shown: a new one is started.
        *live = None;
        ui.ctx().request_repaint();
    }
}

/// A model's name and size, and `model_state`.
fn model_status(
    ui: &mut Ui,
    id: ModelId,
    m: &ModelsStatus,
    used: bool,
    actions: &mut Vec<SettingsAction>,
) {
    ui.horizontal(|ui| {
        let size = size_label(spec(id).approx_bytes);
        ui.label(format!("{} ({size})", model_name(id)));
        model_state(ui, id, m, used, actions);
    });
}

/// Do the captions that run now have this model loaded? Its files cannot be deleted then.
fn in_use(id: ModelId, s: &Settings, m: &ModelsStatus) -> bool {
    if !(m.running || m.loading) {
        return false;
    }
    let listens = s.asr_backend == AsrBackend::Whisper;
    match id {
        ModelId::Madlad3bQ4k => s.translate && s.translator == TranslatorChoice::Builtin,
        ModelId::Gemma12b | ModelId::GemmaE4b => {
            s.translate
                && matches!(s.translator, TranslatorChoice::Natural { light } if id == ModelId::natural(light))
        }
        // Large hands over to Turbo when it falls behind.
        ModelId::WhisperTurbo if listens && s.model == ModelChoice::Large => true,
        ModelId::WhisperTiny
        | ModelId::WhisperBase
        | ModelId::WhisperSmall
        | ModelId::WhisperTurbo
        | ModelId::WhisperLarge => listens && id == ModelId::whisper(s.model, m.compute != "CPU"),
        // The voices of Read aloud.
        _ => s.speak,
    }
}

/// Whether a model is downloaded, with what can be done about it: a Download button, how far
/// its download is, or a Delete button that asks once more before it deletes. One that is
/// `used` by the captions running now says so instead.
fn model_state(
    ui: &mut Ui,
    id: ModelId,
    m: &ModelsStatus,
    used: bool,
    actions: &mut Vec<SettingsAction>,
) {
    match m.downloading.iter().find(|d| d.0 == id) {
        // A thin bar in the row of a button, so the row keeps its height.
        Some(&(_, done, total)) => match total.filter(|all| *all > 0) {
            Some(all) => {
                let part = done as f32 / all as f32;
                ui.add(
                    ProgressBar::new(part)
                        .desired_width(120.0)
                        .desired_height(8.0)
                        .corner_radius(4),
                );
                ui.label(tf(T::Percent, &[&((part * 100.0) as u32)]));
                ui.weak(tf(T::MbOf, &[&(done / 1_000_000), &(all / 1_000_000)]));
            }
            None => {
                ui.spinner();
                ui.weak(t(T::StartingDownload));
            }
        },
        _ if m.cached.contains(&id) => {
            ui.colored_label(Color32::from_rgb(90, 190, 110), t(T::Downloaded));
            if used {
                ui.weak(t(T::InUse)).on_hover_text(t(T::InUseHint));
                return;
            }
            let key = egui::Id::unique("delete-model");
            if ui.button(t(T::Delete)).clicked() {
                ui.data_mut(|d| d.insert_temp(key, id));
            }
            if ui.data(|d| d.get_temp::<ModelId>(key)) != Some(id) {
                return;
            }
            let modal = egui::Modal::new(key).show(ui.ctx(), |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(tf(T::DeleteModelQuestion, &[&model_name(id)])).strong());
                ui.label(tf(T::DeleteFrees, &[&size_label(spec(id).approx_bytes)]));
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t(T::Cancel)).clicked() {
                        ui.close();
                    }
                    if ui.button(t(T::Delete)).clicked() {
                        actions.push(SettingsAction::DeleteModel(id));
                        ui.close();
                    }
                });
            });
            if modal.should_close() {
                ui.data_mut(|d| d.remove::<ModelId>(key));
            }
        }
        _ => {
            if ui.button(t(T::Download)).clicked() {
                actions.push(SettingsAction::Download(id));
            }
        }
    }
}

/// How long a line takes, and whether that keeps up with speech: up to `limits[0]`
/// milliseconds it does, over `limits[1]` it does not, and `then` says what helps.
fn keeping_up(ui: &mut Ui, ms: Option<f32>, verb: T, limits: [f32; 2], then: &str) {
    let Some(ms) = ms else {
        ui.weak(t(T::ShownWhileRunning));
        return;
    };
    let secs = tf(T::Seconds, &[&i18n::decimal(f64::from(ms) / 1000.0, 1)]);
    let verb = t(verb);
    if ms <= limits[0] {
        ui.label(tf(T::KeepUpYes, &[&secs, &verb]));
    } else if ms <= limits[1] {
        ui.label(tf(T::KeepUpNearly, &[&secs, &verb]));
    } else {
        let text = RichText::new(tf(T::KeepUpNo, &[&secs, &verb, &then])).color(WARN);
        ui.add(egui::Label::new(text).wrap());
    }
}

/// What helps the speech model keep up.
fn listen_advice(s: &Settings, m: &ModelsStatus) -> &'static str {
    t(
        if ModelId::whisper(s.model, m.compute != "CPU") != ModelId::WhisperTiny {
            T::AdviceSmallerModel
        } else if cfg!(windows) {
            T::AdviceTooSlowScreen
        } else {
            T::AdviceTooSlow
        },
    )
}

/// What helps the translator keep up.
fn translate_advice(s: &Settings, m: &ModelsStatus) -> &'static str {
    t(match s.translator {
        TranslatorChoice::Natural { light: false } => T::AdviceGpuBusySmaller,
        TranslatorChoice::Natural { light: true } => T::AdviceGpuBusy,
        TranslatorChoice::Builtin if m.compute == "CPU" => T::AdviceCpuSlow,
        TranslatorChoice::Builtin => T::AdviceGpuBusy,
        TranslatorChoice::Llm { .. } => T::AdviceServerSlow,
    })
}

/// What suits this computer: Auto's speech model, and the Natural translator when the graphics
/// card has room for it beside the speech model.
fn recommended(m: &ModelsStatus) -> [ModelId; 2] {
    const GB: u64 = 1 << 30;
    let gpu = m.compute != "CPU";
    // Turbo in half precision and its working memory.
    let speech = if gpu { 5 * GB / 2 } else { 0 };
    let free = m.gpu.as_ref().map_or(0, |g| g.vram).saturating_sub(speech);
    let translator = if !cfg!(windows) {
        ModelId::Madlad3bQ4k
    } else if free >= 9 * GB {
        ModelId::Gemma12b
    } else if free >= 13 * GB / 2 {
        ModelId::GemmaE4b
    } else {
        ModelId::Madlad3bQ4k
    };
    [ModelId::whisper(ModelChoice::Auto, gpu), translator]
}

/// Offers the CUDA build on an RTX 50 card that runs the processor build; the updater installs
/// it over this one.
fn gpu_build(ui: &mut Ui, m: &ModelsStatus, actions: &mut Vec<SettingsAction>) {
    use super::update::{self, Status};
    let Some(gpu) = m.gpu.as_ref().filter(|g| update::gpu_build_helps(Some(g))) else {
        return;
    };
    ui.add(egui::Label::new(tf(T::GpuBuildHelps, &[&gpu.name])).wrap());
    ui.horizontal(|ui| match &m.update {
        Status::Checking | Status::Downloading(..) | Status::Ready(..) => {
            ui.weak(update_text(&m.update).unwrap_or_default());
        }
        _ if update::installed() => {
            if ui.button(t(T::SwitchToGpu)).clicked() {
                actions.push(SettingsAction::SwitchToGpu);
            }
        }
        _ => {
            ui.hyperlink_to(t(T::GetGpuBuild), "https://hammurabi.menzo.dev/#download");
        }
    });
    ui.add_space(8.0);
}

/// What the speech model runs on, in words.
fn runs_on(compute: &str) -> &'static str {
    t(match compute {
        "CPU" => T::RunsOnCpu,
        "Metal" => T::RunsOnMetal,
        _ => T::RunsOnNvidia,
    })
}

/// The Listen page: where captions come from, then what that needs. Listening needs the audio
/// and the speech model; subtitles on screen need where to read them; Windows Live Captions
/// keeps its settings to itself.
fn listen(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    if cfg!(windows) {
        section(ui, t(T::CaptionsFrom));
        ui.horizontal(|ui| {
            // Listening starts the speech model.
            let id = ModelId::whisper(s.model, m.compute != "CPU");
            let free = s.asr_backend == AsrBackend::Whisper || can_pick(id, m);
            *changed |= ui
                .add_enabled_ui(free, |ui| {
                    ui.radio_value(&mut s.asr_backend, AsrBackend::Whisper, t(T::Listening))
                })
                .inner
                .changed();
            if !free {
                ui.weak(t(T::DownloadFirst));
                model_state(ui, id, m, false, actions);
            }
            if cfg!(feature = "win-livecaptions") {
                *changed |= ui
                    .radio_value(
                        &mut s.asr_backend,
                        AsrBackend::WindowsLiveCaptions,
                        t(T::WindowsLiveCaptions),
                    )
                    .changed();
            }
            *changed |= ui
                .radio_value(
                    &mut s.asr_backend,
                    AsrBackend::ScreenText,
                    t(T::SubtitlesOnScreen),
                )
                .on_hover_text(t(T::ScreenHint))
                .changed();
        });
    }
    let lc = cfg!(all(windows, feature = "win-livecaptions"))
        && s.asr_backend == AsrBackend::WindowsLiveCaptions;
    let screen = cfg!(windows) && s.asr_backend == AsrBackend::ScreenText;
    if cfg!(windows) {
        ui.weak(t(if screen {
            T::ReadsSubtitlesNote
        } else if lc {
            T::LiveCaptionsNote
        } else {
            T::ListensNote
        }));
    }
    #[cfg(windows)]
    if screen {
        ui.add_space(4.0);
        grid(ui, "screen", |ui| screen_rows(ui, s, changed));
        return;
    }
    if lc {
        return;
    }
    audio(ui, s, m, changed, actions);
    speech(ui, s, m, changed, actions);
}

fn speech(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, t(T::SpeechRecognition));
    let gpu = m.compute != "CPU";
    grid(ui, "speech", |ui| {
        ui.label(t(T::SpokenLanguage));
        let current = s
            .spoken_language
            .as_deref()
            .map_or(t(T::AutoDetect), lang_name)
            .to_string();
        ComboBox::from_id_salt("spoken")
            .selected_text(current)
            .width(200.0)
            .show_ui(ui, |ui| {
                *changed |= ui
                    .selectable_value(&mut s.spoken_language, None, t(T::AutoDetect))
                    .changed();
                for c in SOURCE_LANGS {
                    *changed |= ui
                        .selectable_value(&mut s.spoken_language, Some(c.to_string()), lang_name(c))
                        .changed();
                }
            });
        ui.end_row();

        ui.label(t(T::WordsToExpect));
        hint_tags(ui, &mut s.hint, changed);
        ui.end_row();

        ui.label(t(T::Model));
        let label = |c: ModelChoice| match c {
            ModelChoice::Auto => tf(
                T::AutoModel,
                &[&spec(ModelId::whisper(ModelChoice::Auto, gpu)).label],
            ),
            c => {
                let id = ModelId::whisper(c, gpu);
                format!("{} ({})", spec(id).label, size_label(spec(id).approx_bytes))
            }
        };
        ui.horizontal(|ui| {
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
                        ModelChoice::Large,
                    ] {
                        let id = ModelId::whisper(c, gpu);
                        if can_pick(id, m) {
                            *changed |= ui.selectable_value(&mut s.model, c, label(c)).changed();
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(false, |ui| {
                                ui.selectable_label(s.model == c, label(c))
                            });
                            ui.weak(t(T::DownloadFirst));
                            model_state(ui, id, m, false, actions);
                        });
                    }
                });
            let id = ModelId::whisper(s.model, gpu);
            model_state(ui, id, m, in_use(id, s, m), actions);
        });
        ui.end_row();

        ui.label(t(T::KeepingUp));
        keeping_up(
            ui,
            m.asr_ms,
            T::VerbHear,
            [400.0, 1000.0],
            listen_advice(s, m),
        );
        ui.end_row();

        ui.label(t(T::Speed));
        *changed |= ui
            .checkbox(&mut s.trim_audio, t(T::SkipSilence))
            .on_hover_text(t(T::SkipSilenceHint))
            .changed();
        ui.end_row();

        ui.label(t(T::RunsOn));
        // A label in a grid is one line unless it is told to wrap.
        ui.add(egui::Label::new(runs_on(&m.compute)).wrap());
        ui.end_row();
    });
}

/// The words to expect (kept as one text, commas between them) as tags: Enter, a comma or
/// leaving the field adds what was typed, the cross on a tag removes it, and their count opens and closes them. Each
/// line of tags is a row of the grid `ui` lays out.
fn hint_tags(ui: &mut Ui, hint: &mut String, changed: &mut bool) {
    /// As wide as a line of tags gets.
    const WIDTH: f32 = 380.0;
    let mut words = hint_words(hint);
    let id = ui.scope_id().with("hint");
    let (mut new, mut open): (String, bool) = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        let field = ui
            .add(
                egui::TextEdit::singleline(&mut new)
                    .hint_text(t(T::HintPlaceholder))
                    .desired_width(200.0),
            )
            .on_hover_text(t(T::HintHover));
        // Clicking elsewhere adds what was typed too, and only Enter keeps the field for more.
        let done = field.lost_focus();
        let enter = done && ui.input(|i| i.key_pressed(Key::Enter));
        if done || new.contains(',') {
            // What stands after the last comma is still being typed.
            let cut = if done {
                new.len()
            } else {
                new.rfind(',').map_or(0, |i| i + 1)
            };
            for word in hint_words(&new[..cut]) {
                if !words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                    words.push(word);
                    open = true;
                }
            }
            new = new[cut..].trim_start().to_string();
            if enter {
                field.request_focus();
            }
        }
        if !words.is_empty() && opener(ui, open, words.len()) {
            open = !open;
        }
    });
    let mut gone = None;
    if open {
        let mut from = 0;
        while from < words.len() {
            // As many as fit, and one at least.
            let mut wide = 0.0;
            let line = words[from..]
                .iter()
                .take_while(|w| {
                    wide += tag_width(ui, w) + 6.0;
                    wide <= WIDTH + 6.0
                })
                .count()
                .max(1);
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, word) in words.iter().enumerate().skip(from).take(line) {
                    if tag(ui, i, word) {
                        gone = Some(i);
                    }
                }
            });
            from += line;
        }
    }
    if let Some(i) = gone {
        words.remove(i);
    }
    ui.data_mut(|d| d.insert_temp(id, (new, open)));
    if words != hint_words(hint) {
        *hint = words.join(", ");
        *changed = true;
    }
}

/// How many words there are, with the triangle that opens them; true when clicked.
fn opener(ui: &mut Ui, open: bool, n: usize) -> bool {
    let text = if n == 1 {
        t(T::OneWord).to_string()
    } else {
        tf(T::NWords, &[&n])
    };
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font, Color32::PLACEHOLDER);
    let size = vec2(20.0 + galley.size().x, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, open, &text));
    let icon = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(14.0, 14.0));
    let openness = if open { 1.0 } else { 0.0 };
    egui::collapsing_header::paint_default_icon(ui, openness, &resp.clone().with_new_rect(icon));
    let at = pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0);
    ui.painter()
        .galley(at, galley, ui.style().interact(&resp).text_color());
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn hint_words(hint: &str) -> Vec<String> {
    hint.split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// The room a tag's cross takes, and the room left of its word.
const TAG_CROSS: f32 = 22.0;
const TAG_PAD: f32 = 10.0;

fn tag_width(ui: &Ui, word: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(word.to_string(), font, Color32::PLACEHOLDER);
    TAG_PAD + text.size().x + TAG_CROSS
}

/// A word with a cross to its right; true when the cross was clicked.
fn tag(ui: &mut Ui, i: usize, word: &str) -> bool {
    let v = ui.visuals().clone();
    let font = TextStyle::Body.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(word.to_string(), font, v.text_color());
    let size = vec2(tag_width(ui, word), 24.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let cross = rect.with_min_x(rect.right() - TAG_CROSS);
    let resp = ui
        .interact(cross, ui.scope_id().with(("tag", i)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, tf(T::RemoveWord, &[&word])));
    let painter = ui.painter();
    painter.rect_filled(rect, 12, v.widgets.inactive.weak_bg_fill);
    let at = pos2(rect.left() + TAG_PAD, rect.center().y - text.size().y / 2.0);
    painter.galley(at, text, v.text_color());
    let stroke = Stroke::new(
        1.5,
        if resp.hovered() {
            v.strong_text_color()
        } else {
            v.weak_text_color()
        },
    );
    let (c, d) = (cross.center() - vec2(2.0, 0.0), 3.5);
    painter.line_segment([c - vec2(d, d), c + vec2(d, d)], stroke);
    painter.line_segment([c - vec2(d, -d), c + vec2(d, -d)], stroke);
    resp.clicked()
}

fn translation(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, t(T::GroupTranslation));
    // Turning it on starts the chosen translator's model.
    let model = match s.translator {
        TranslatorChoice::Builtin => Some(ModelId::Madlad3bQ4k),
        TranslatorChoice::Natural { light } => Some(ModelId::natural(light)),
        TranslatorChoice::Llm { .. } => None,
    };
    let free = s.translate || model.is_none_or(|id| can_pick(id, m));
    ui.horizontal(|ui| {
        *changed |= ui
            .add_enabled(
                free,
                egui::Checkbox::new(&mut s.translate, t(T::TranslateCaptions)),
            )
            .changed();
        if let Some(id) = model.filter(|_| !free) {
            ui.weak(t(T::DownloadFirst));
            model_state(ui, id, m, false, actions);
        }
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(s.translate, |ui| {
        grid(ui, "translation", |ui| {
            ui.label(t(T::TranslateTo));
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
        });

        // The radios with a line under each are outside the grid: tall cells leave the rows
        // after them too much room.
        section(ui, t(T::Translator));
        // 0 = MADLAD, 1 = Natural, 2 = the user's own server.
        let was = match s.translator {
            TranslatorChoice::Builtin => 0,
            TranslatorChoice::Natural { .. } => 1,
            TranslatorChoice::Llm { .. } => 2,
        };
        let mut now = was;
        for (v, title, model, about) in [
            (0, T::Fast, Some(ModelId::Madlad3bQ4k), T::FastNote),
            (1, T::Natural, Some(ModelId::natural(false)), T::NaturalNote),
            (2, T::OwnServer, None, T::OwnServerNote),
        ] {
            if v == 1 && !cfg!(windows) {
                continue;
            }
            let free = v == was
                || (v == 1 && natural_light(m).is_some())
                || (v != 1 && model.is_none_or(|id| can_pick(id, m)));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(free, egui::RadioButton::new(now == v, t(title)))
                    .clicked()
                {
                    now = v;
                }
                if let Some(id) = model.filter(|_| !free) {
                    ui.weak(t(T::DownloadFirst));
                    model_state(ui, id, m, false, actions);
                }
            });
            indented(ui, |ui| {
                ui.weak(t(about));
            });
        }
        if now != was {
            s.translator = match now {
                0 => TranslatorChoice::Builtin,
                1 => TranslatorChoice::Natural {
                    light: natural_light(m).unwrap_or(false),
                },
                _ => TranslatorChoice::Llm {
                    base_url: OLLAMA_URL.into(),
                    model: OLLAMA_MODEL.into(),
                },
            };
            *changed = true;
        }
        ui.add_space(4.0);

        grid(ui, "translator", |ui| {
            let used = [ModelId::Madlad3bQ4k, ModelId::Gemma12b, ModelId::GemmaE4b]
                .map(|id| in_use(id, s, m));
            match &mut s.translator {
                TranslatorChoice::Builtin => {
                    ui.label(t(T::Model));
                    model_status(ui, ModelId::Madlad3bQ4k, m, used[0], actions);
                    ui.end_row();
                }
                TranslatorChoice::Natural { light } => {
                    ui.label(t(T::Model));
                    let used = used[1 + usize::from(*light)];
                    model_status(ui, ModelId::natural(*light), m, used, actions);
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        let other = ModelId::natural(!*light);
                        let free = can_pick(other, m);
                        *changed |= ui
                            .add_enabled(free, egui::Checkbox::new(light, t(T::SmallerModel)))
                            .on_hover_text(t(T::SmallerModelHint))
                            .changed();
                        if !free {
                            ui.weak(t(T::DownloadFirst));
                            model_state(ui, other, m, false, actions);
                        }
                    });
                    ui.end_row();
                }
                TranslatorChoice::Llm { base_url, model } => {
                    ui.label(t(T::Address));
                    *changed |= ui
                        .add(egui::TextEdit::singleline(base_url).desired_width(260.0))
                        .changed();
                    ui.end_row();
                    ui.label(t(T::ModelName));
                    *changed |= ui
                        .add(egui::TextEdit::singleline(model).desired_width(260.0))
                        .changed();
                    ui.end_row();
                }
            }

            ui.label(t(T::KeepingUp));
            keeping_up(
                ui,
                m.tr_ms,
                T::VerbTranslate,
                [700.0, 1500.0],
                translate_advice(s, m),
            );
            ui.end_row();
        });
        ui.add_space(4.0);
        *changed |= ui
            .checkbox(&mut s.instant, t(T::TranslateWhileSaid))
            .changed();
        indented(ui, |ui| {
            ui.weak(t(T::SoonerNote));
        });
    });
}

/// A selectable persona row with a play or stop button; returns (row clicked, button clicked).
fn persona_card(
    ui: &mut Ui,
    p: &Persona,
    voice: &str,
    selected: bool,
    playing: bool,
) -> (bool, bool) {
    let mut play = false;
    let card = ui.scope_builder(UiBuilder::new().id_salt(p.id).sense(Sense::click()), |ui| {
        let hovered = ui.response().hovered();
        let v = ui.visuals();
        let (fill, stroke) = if selected {
            let accent = v.selection.bg_fill;
            (accent.gamma_multiply(0.14), Stroke::new(1.5, accent))
        } else if hovered {
            (v.widgets.hovered.weak_bg_fill, v.widgets.hovered.bg_stroke)
        } else {
            (v.faint_bg_color, v.widgets.noninteractive.bg_stroke)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong(p.name());
                    ui.label(t(p.description));
                });
                ui.label(RichText::new(voice).small().weak());
                // Frameless, text-height play button centred on the row's right edge.
                let row = ui.min_rect();
                let size = ui.text_style_height(&TextStyle::Body) + 6.0;
                let rect = Rect::from_center_size(
                    pos2(row.right() - size / 2.0, row.center().y),
                    vec2(size, size),
                );
                let button = ui
                    .interact(rect, ui.scope_id().with("play"), Sense::click())
                    .on_hover_text(t(if playing { T::Stop } else { T::PlaySample }))
                    .on_hover_cursor(CursorIcon::PointingHand);
                let v = ui.visuals();
                if button.hovered() {
                    ui.painter()
                        .rect_filled(rect, 4, v.widgets.hovered.weak_bg_fill);
                }
                let color = ui.style().interact(&button).fg_stroke.color;
                if playing {
                    icons::paint_stop(ui.painter(), rect.shrink(4.0), color);
                } else {
                    icons::paint_play(ui.painter(), rect.shrink(4.0), color);
                }
                play = button.clicked();
            });
    });
    let clicked = card
        .response
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked();
    (clicked, play)
}

/// Lines up with the text of the radio button above.
fn indented(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 24,
            ..Default::default()
        })
        .show(ui, |ui| ui.horizontal_wrapped(add));
}

/// A radio button with its explanation under it.
fn engine_row(ui: &mut Ui, s: &mut Settings, v: tts::Engine, title: T, about: T) -> bool {
    let changed = ui.radio_value(&mut s.speak_engine, v, t(title)).changed();
    indented(ui, |ui| {
        ui.weak(t(about));
    });
    changed
}

fn read_aloud(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    section(ui, t(T::PageReadAloud));
    *changed |= ui
        .checkbox(&mut s.speak, t(T::ReadCaptionsAloud))
        .on_hover_text(t(T::ReadCaptionsAloudHint))
        .changed();
    if let Some(voice) = &m.reading {
        ui.weak(tf(T::ReadingWith, &[voice]));
    }
    let lang = s.speak_lang().map(str::to_string);
    // Previews are in English while the spoken language is auto-detected.
    let sample_lang = lang.clone().unwrap_or_else(|| "en".into());
    let voices: Vec<&Voice> = m
        .voices
        .iter()
        .filter(|v| {
            lang.as_deref()
                .is_none_or(|l| tts::lang_matches(&v.lang, l))
        })
        .collect();
    ui.add_enabled_ui(s.speak, |ui| {
        section(ui, t(T::VoiceEngine));
        *changed |= engine_row(
            ui,
            s,
            tts::Engine::System,
            T::SystemVoices,
            T::SystemVoicesNote,
        );
        *changed |= engine_row(
            ui,
            s,
            tts::Engine::Supertonic,
            T::NaturalVoices,
            T::NaturalVoicesNote,
        );
        if s.speak_engine == tts::Engine::Supertonic {
            let used = in_use(ModelId::Supertonic, s, m);
            indented(ui, |ui| {
                model_status(ui, ModelId::Supertonic, m, used, actions)
            });
            if !m.cached.contains(&ModelId::Supertonic) {
                indented(ui, |ui| {
                    ui.colored_label(WARN, t(T::SystemVoicesUntilDownloaded));
                });
            }
        }
        #[cfg(not(omnivoice))]
        {
            ui.add_enabled(false, egui::RadioButton::new(false, t(T::SpeakersVoice)))
                .on_disabled_hover_text(t(T::GpuDownloadOnly));
            indented(ui, |ui| {
                ui.weak(t(T::SpeakerVoiceNoteGpu));
            });
        }
        #[cfg(omnivoice)]
        {
            *changed |= engine_row(
                ui,
                s,
                tts::Engine::Speaker,
                T::SpeakersVoice,
                T::SpeakerVoiceNote,
            );
            if s.speak_engine == tts::Engine::Speaker {
                speaker_voice(ui, s, m, changed, actions);
            }
        }

        let cloning = s.speak_engine == tts::Engine::Speaker;
        section(ui, t(if cloning { T::BackupVoice } else { T::Voice }));
        if cloning {
            ui.weak(t(T::BackupVoiceNote));
            ui.add_space(2.0);
        }
        let neural = m.cached.contains(&ModelId::Supertonic);
        let current = tts::persona(&s.speak_persona).id;
        for p in &tts::PERSONAS {
            let style = Style {
                persona: p.id.into(),
                ..s.speak_style()
            };
            let reader = tts::neural(&style, &sample_lang)
                .or_else(|| tts::neural_fallback(&style, &m.voices, &sample_lang));
            let voice = match (
                reader.filter(|_| neural),
                style.resolve(&m.voices, &sample_lang),
            ) {
                (Some((_, v, _)), _) => tf(T::NaturalVoiceN, &[&v]),
                (_, Ok((Some(v), ..))) => format!("{} ({})", v.name, v.lang),
                (_, Ok((None, ..))) => t(T::SystemVoice).into(),
                (_, Err(_)) => tf(T::NoVoiceFor, &[&lang_name(&sample_lang)]),
            };
            let playing = m.previewing == Some(p.id);
            let (select, play) = persona_card(ui, p, &voice, p.id == current, playing);
            if select && s.speak_persona != p.id {
                s.speak_persona = p.id.into();
                *changed = true;
            }
            if play {
                actions.push(if playing {
                    SettingsAction::StopPreview
                } else {
                    SettingsAction::Preview(p.id)
                });
            }
        }
        if current == tts::CUSTOM {
            ui.add_space(4.0);
            custom_voice(ui, s, m, &voices, &sample_lang, changed);
        }
        if let Some(l) = &lang
            && s.speak
            && !m.voices.is_empty()
            && voices.is_empty()
            && !(neural
                && (tts::neural(&s.speak_style(), l).is_some()
                    || tts::neural_fallback(&s.speak_style(), &m.voices, l).is_some()))
        {
            ui.colored_label(WARN, tts::missing_voice(l));
        }

        section(ui, t(T::Playback));
        grid(ui, "playback", |ui| {
            ui.label(t(T::Volume));
            *changed |= ui
                .add(
                    Slider::new(&mut s.speak_volume, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .changed();
            ui.end_row();
            ui.label(t(T::PlayOn));
            device_combo(
                ui,
                "speak_out",
                &m.devices.outputs,
                &mut s.speak_device,
                changed,
            );
            ui.end_row();
            if crate::audio::duck::SUPPORTED {
                ui.label(t(T::OtherAppsWhileReading));
                *changed |= ui
                    .add(
                        Slider::new(&mut s.speak_duck, 0.1..=1.0).custom_formatter(|v, _| {
                            if v >= 1.0 {
                                t(T::NotTurnedDown).into()
                            } else {
                                format!("{:.0}%", v * 100.0)
                            }
                        }),
                    )
                    .changed();
                ui.end_row();
            }
        });
        if s.translate {
            let label = tf(T::AlsoReadSame, &[&lang_name(&s.target_language)]);
            *changed |= ui
                .checkbox(&mut s.speak_same_lang, label)
                .on_hover_text(t(T::AlsoReadSameHint))
                .changed();
        }
        ui.weak(t(T::ListenNote));
        ui.weak(t(T::PileUpNote));
    });
}

#[cfg(omnivoice)]
fn speaker_voice(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    changed: &mut bool,
    actions: &mut Vec<SettingsAction>,
) {
    if !s.omnivoice_accepted {
        indented(ui, |ui| {
            *changed |= ui
                .checkbox(&mut s.omnivoice_accepted, t(T::PersonalUseAgree))
                .changed();
        });
    } else {
        let used = in_use(ModelId::OmniVoice, s, m);
        indented(ui, |ui| {
            model_status(ui, ModelId::OmniVoice, m, used, actions)
        });
        if m.cached.contains(&ModelId::OmniVoice) {
            indented(ui, |ui| {
                ui.label(t(if m.voice_learned {
                    T::ReadingInSpeakersVoice
                } else {
                    T::ListeningToLearn
                }));
                if m.voice_learned && ui.button(t(T::LearnAgain)).clicked() {
                    actions.push(SettingsAction::RelearnVoice);
                }
            });
        }
    }
    indented(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.weak(t(T::LicenceBefore));
        ui.hyperlink_to(
            t(T::Licence),
            "https://huggingface.co/k2-fsa/OmniVoice#license",
        );
        ui.weak(t(T::LicenceAfter));
    });
}

fn custom_voice(
    ui: &mut Ui,
    s: &mut Settings,
    m: &ModelsStatus,
    voices: &[&Voice],
    sample_lang: &str,
    changed: &mut bool,
) {
    // Supertonic reads this language with this engine.
    let neural = tts::neural(
        &Style {
            voice: None,
            ..s.speak_style()
        },
        sample_lang,
    )
    .is_some();
    grid(ui, "read_aloud", |ui| {
        ui.label(t(T::Voice));
        let label = |v: &Voice| format!("{} ({})", v.name, v.lang);
        let current = match &s.speak_voice {
            None if neural => tf(T::NaturalVoiceN, &[&s.speak_neural_voice]),
            None => t(T::Automatic).to_string(),
            Some(id) => m
                .voices
                .iter()
                .find(|v| &v.id == id)
                .map_or_else(|| tf(T::Missing, &[id]), label),
        };
        ComboBox::from_id_salt("voice")
            .selected_text(current)
            .width(260.0)
            .show_ui(ui, |ui| {
                if neural {
                    for v in supertonic::STYLES {
                        let on = s.speak_voice.is_none() && s.speak_neural_voice == v;
                        if ui
                            .selectable_label(on, tf(T::NaturalVoiceN, &[&v]))
                            .clicked()
                        {
                            s.speak_voice = None;
                            s.speak_neural_voice = v.into();
                            *changed = true;
                        }
                    }
                    ui.separator();
                } else {
                    *changed |= ui
                        .selectable_value(&mut s.speak_voice, None, t(T::Automatic))
                        .on_hover_text(t(T::AutomaticHint))
                        .changed();
                }
                for v in voices {
                    *changed |= ui
                        .selectable_value(&mut s.speak_voice, Some(v.id.clone()), label(v))
                        .changed();
                }
            });
        ui.end_row();

        ui.label(t(T::Speed));
        *changed |= ui
            .add(
                Slider::new(&mut s.speak_rate, 0.5..=2.0)
                    .custom_formatter(|v, _| format!("{v:.1}x")),
            )
            .changed();
        ui.end_row();
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

fn captions(ui: &mut Ui, s: &mut Settings, changed: &mut bool, actions: &mut Vec<SettingsAction>) {
    section(ui, t(T::Position));
    ui.weak(t(T::PositionNote));
    if ui
        .add_enabled(
            s.overlay_pos.is_some(),
            egui::Button::new(t(T::ResetPosition)),
        )
        .clicked()
    {
        actions.push(SettingsAction::ResetPosition);
    }
    *changed |= ui
        .checkbox(&mut s.overlay_locked, t(T::LockInPlace))
        .on_hover_text(t(T::LockHint))
        .changed();

    section(ui, t(T::Streaming));
    *changed |= ui
        .checkbox(&mut s.browser_source, t(T::CaptionsAsWebPage))
        .on_hover_text(t(T::WebPageHint))
        .changed();
    if s.browser_source {
        ui.weak(t(T::ObsNote));
        ui.horizontal(|ui| {
            let url = super::web::Web::url();
            let key = egui::Id::unique("url-copied");
            let now = ui.input(|i| i.time);
            if ui.link(&url).on_hover_text(t(T::Copy)).clicked() {
                ui.ctx().copy_text(url);
                ui.data_mut(|d| d.insert_temp(key, now + 2.0));
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs(2));
            }
            if ui
                .data(|d| d.get_temp::<f64>(key))
                .is_some_and(|until| now < until)
            {
                ui.weak(t(T::Copied));
            }
        });
        ui.weak(t(T::ScaleNote));
    }

    section(ui, t(T::Look));
    if cfg!(windows) && s.asr_backend == AsrBackend::ScreenText {
        *changed |= ui
            .checkbox(&mut s.screen_cover, t(T::CoverSubtitles))
            .on_hover_text(t(T::CoverSubtitlesHint))
            .changed();
    }
    // A cover shows the translation alone, in a box that hides the subtitle under it.
    let covers = s.covers();
    if covers {
        ui.weak(t(T::CoverLookNote));
    }
    let c = &mut s.caption;
    grid(ui, "captions", |ui| {
        ui.label(t(T::TranslatedText));
        ui.horizontal(|ui| {
            *changed |= ui
                .add(Slider::new(&mut c.trans_size, 12.0..=72.0).suffix(" px"))
                .changed();
            color_row(ui, &mut c.trans_color, changed);
        });
        ui.end_row();

        ui.add_enabled_ui(!covers, |ui| ui.label(t(T::OriginalText)));
        ui.add_enabled_ui(!covers, |ui| {
            ui.horizontal(|ui| {
                *changed |= ui
                    .add(Slider::new(&mut c.orig_size, 10.0..=60.0).suffix(" px"))
                    .changed();
                color_row(ui, &mut c.orig_color, changed);
            });
        });
        ui.end_row();

        // A cover has an opacity of its own, so that one over the game's subtitle hides it
        // whatever the caption boxes are set to.
        ui.label(t(T::Background));
        ui.horizontal(|ui| {
            let opacity = if covers {
                &mut c.cover_opacity
            } else {
                &mut c.bg_opacity
            };
            *changed |= ui
                .add(
                    Slider::new(opacity, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .changed();
            color_row(ui, &mut c.bg_color, changed);
        });
        ui.end_row();
        if covers && c.cover_opacity < 1.0 {
            ui.label("");
            ui.weak(t(T::CoverSeeThrough));
            ui.end_row();
        }

        ui.add_enabled_ui(!covers, |ui| ui.label(t(T::CaptionsAtOnce)));
        *changed |= ui
            .add_enabled(!covers, Slider::new(&mut c.cues, 1..=3))
            .changed();
        ui.end_row();

        ui.label(t(T::KeepOnScreen));
        *changed |= ui
            .add(
                Slider::new(&mut c.hold_secs, 1.0..=15.0)
                    .step_by(0.5)
                    .custom_formatter(|v, _| tf(T::Seconds, &[&i18n::decimal(v, 1)])),
            )
            .changed();
        ui.end_row();

        ui.label(t(T::TextEdge));
        ui.horizontal(|ui| {
            for (v, label) in [
                (Edge::None, T::EdgeNone),
                (Edge::Shadow, T::Shadow),
                (Edge::Outline, T::Outline),
            ] {
                *changed |= ui.radio_value(&mut c.edge, v, t(label)).changed();
            }
        });
        ui.end_row();

        if covers {
            for (label, value) in [
                (T::CoverRight, &mut c.cover_x),
                (T::CoverUp, &mut c.cover_y),
            ] {
                ui.label(t(label));
                *changed |= ui
                    .add(Slider::new(value, -30.0..=30.0).step_by(0.5).suffix("%"))
                    .changed();
                ui.end_row();
            }

            ui.label(t(T::CoverPadding));
            *changed |= ui
                .add(
                    Slider::new(&mut c.cover_pad, 0.0..=3.0)
                        .step_by(0.1)
                        .custom_formatter(|v, _| format!("{}×", i18n::decimal(v, 1))),
                )
                .changed();
            ui.end_row();

            ui.label(t(T::CoverMaxWidth));
            *changed |= ui
                .add(
                    Slider::new(&mut c.cover_max_width, 30.0..=100.0)
                        .step_by(1.0)
                        .suffix("%"),
                )
                .changed();
            ui.end_row();
        }
    });
    *changed |= ui
        .add_enabled(
            !covers,
            egui::Checkbox::new(&mut c.show_original, t(T::ShowOriginal)),
        )
        .changed();
    ui.add_enabled_ui(c.show_original && !covers, |ui| {
        *changed |= ui
            .checkbox(&mut c.original_first, t(T::OriginalFirst))
            .changed();
    });
    ui.add_space(4.0);
    if ui
        .add_enabled(
            *c != CaptionStyle::default(),
            egui::Button::new(t(T::ResetLook)),
        )
        .clicked()
    {
        *c = CaptionStyle::default();
        *changed = true;
    }
}

/// A key press as a `global_hotkey` string, e.g. "alt+shift+KeyT". None without a modifier a
/// shortcut can hang on, or for a key that makes a poor shortcut.
fn hotkey_text(mods: Modifiers, key: Key) -> Option<String> {
    let name = key.name();
    let f_key = matches!(name.as_bytes(), [b'F', rest @ ..]
        if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit));
    let held = [mods.ctrl, mods.alt, mods.shift, mods.mac_cmd]
        .into_iter()
        .filter(|&on| on)
        .count();
    // One modifier and a letter, digit or Space is the apps' own, like Ctrl+C: taken
    // system-wide it would stop working everywhere.
    if !(mods.ctrl || mods.alt || mods.mac_cmd) || (held < 2 && !f_key) {
        return None;
    }
    let code = match name.as_bytes() {
        [c] if c.is_ascii_uppercase() => format!("Key{name}"),
        [c] if c.is_ascii_digit() => format!("Digit{name}"),
        _ if f_key => name.to_string(),
        _ if key == Key::Space => "Space".into(),
        _ => return None,
    };
    let mut parts = Vec::new();
    for (on, part) in [
        (mods.ctrl, "ctrl"),
        (mods.alt, "alt"),
        (mods.shift, "shift"),
        (mods.mac_cmd, "cmd"),
    ] {
        if on {
            parts.push(part);
        }
    }
    parts.push(&code);
    Some(parts.join("+"))
}

/// "alt+shift+KeyT" -> "Alt+Shift+T".
fn hotkey_label(s: &str) -> String {
    s.split('+')
        .map(|t| {
            let t = t.trim();
            let t = t
                .strip_prefix("Key")
                .or(t.strip_prefix("Digit"))
                .unwrap_or(t);
            let mut c = t.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join("+")
}

/// The update check's state in a few words; None before the first check.
fn update_text(status: &super::update::Status) -> Option<String> {
    use super::update::Status;
    Some(match status {
        Status::Idle => return None,
        Status::Checking => t(T::CheckingUpdates).into(),
        Status::UpToDate => t(T::UpToDate).into(),
        Status::Downloading(v, pct) => tf(T::DownloadingUpdate, &[v, pct]),
        Status::Ready(r, _) => tf(T::UpdateReady, &[&r.version]),
        Status::Available(r) => tf(T::UpdateOut, &[&r.version]),
        Status::Failed => t(T::UpdateCheckFailed).into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(cached: Vec<ModelId>, compute: &str) -> ModelsStatus {
        ModelsStatus {
            cached,
            downloading: Vec::new(),
            devices: DeviceList {
                inputs: Vec::new(),
                outputs: Vec::new(),
                system_audio: Ok(()),
            },
            compute: compute.into(),
            gpu: None,
            asr_ms: None,
            tr_ms: None,
            autostart: false,
            voices: Vec::new(),
            running: false,
            loading: false,
            status: String::new(),
            alert: false,
            reading: None,
            previewing: None,
            voice_learned: false,
            update: super::super::update::Status::Idle,
        }
    }

    #[test]
    fn recommends_by_graphics_card() {
        let with = |compute: &str, mib: u64| {
            let mut m = status(Vec::new(), compute);
            m.gpu = (mib > 0).then(|| crate::device::Gpu {
                name: "card".into(),
                vram: mib << 20,
            });
            recommended(&m)
        };
        let natural = |id| {
            if cfg!(windows) {
                id
            } else {
                ModelId::Madlad3bQ4k
            }
        };
        use ModelId::*;
        // An RTX 5080 (16 GB) with the CUDA build, and with the processor build.
        assert_eq!(with("CUDA 0", 16303), [WhisperTurbo, natural(Gemma12b)]);
        assert_eq!(with("CPU", 16303), [WhisperBase, natural(Gemma12b)]);
        // 12 GB with the speech model on it too, and 8 GB.
        assert_eq!(with("CUDA 0", 12227), [WhisperTurbo, natural(Gemma12b)]);
        assert_eq!(with("CUDA 0", 8151), [WhisperTurbo, Madlad3bQ4k]);
        assert_eq!(with("CPU", 8151), [WhisperBase, natural(GemmaE4b)]);
        // No card, or one with little memory of its own.
        assert_eq!(with("CPU", 0), [WhisperBase, Madlad3bQ4k]);
        assert_eq!(with("CPU", 128), [WhisperBase, Madlad3bQ4k]);
    }

    #[test]
    fn hotkey_label() {
        assert_eq!(super::hotkey_label("alt+shift+KeyT"), "Alt+Shift+T");
        assert_eq!(super::hotkey_label("ctrl+Digit1"), "Ctrl+1");
    }

    #[test]
    fn records_hotkeys_the_hotkey_parser_accepts() {
        let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
        let text = |m, k| hotkey_text(m, k);
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(text(alt_shift, Key::T).unwrap(), "alt+shift+KeyT");
        assert_eq!(text(ctrl_shift, Key::Num1).unwrap(), "ctrl+shift+Digit1");
        assert_eq!(text(Modifiers::ALT, Key::F5).unwrap(), "alt+F5");
        assert_eq!(
            text(Modifiers::CTRL | Modifiers::ALT, Key::Space).unwrap(),
            "ctrl+alt+Space"
        );
        assert_eq!(text(Modifiers::NONE, Key::T), None);
        assert_eq!(text(Modifiers::SHIFT, Key::T), None);
        assert_eq!(text(Modifiers::SHIFT, Key::F5), None);
        assert_eq!(text(Modifiers::ALT, Key::Enter), None);
        // One modifier is not enough unless the key is a function key.
        assert_eq!(text(Modifiers::CTRL, Key::C), None);
        assert_eq!(text(Modifiers::ALT, Key::Num1), None);
        assert_eq!(text(Modifiers::CTRL, Key::Space), None);
        for k in [Key::T, Key::Num1, Key::F5, Key::Space] {
            let s = text(alt_shift, k).unwrap();
            assert!(s.parse::<global_hotkey::hotkey::HotKey>().is_ok(), "{s}");
        }
    }

    #[test]
    fn records_the_hotkey_only_on_its_page() {
        let ctx = Context::default();
        let mut s = Settings::default();
        let was = s.hotkey_toggle.clone();
        let press = egui::RawInput {
            events: vec![Event::Key {
                key: Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            }],
            ..Default::default()
        };
        let mut win = SettingsWindow {
            page: Page::Listen,
            recording: true,
            ..Default::default()
        };
        let mut changed = false;
        let mut frame = |win: &mut SettingsWindow, s: &mut Settings| {
            ctx.run_pass(press.clone(), |ctx| win.record_hotkey(ctx, s, &mut changed))
                .drop_without_applying_deltas();
        };
        frame(&mut win, &mut s);
        assert!(!win.recording);
        assert_eq!(s.hotkey_toggle, was);

        win.page = Page::General;
        frame(&mut win, &mut s);
        assert_eq!(s.hotkey_toggle, was, "the button was not clicked again");
        win.recording = true;
        frame(&mut win, &mut s);
        assert!(!win.recording);
        assert_eq!(s.hotkey_toggle, "ctrl+shift+KeyK");
        assert!(changed);
    }

    #[test]
    fn a_model_not_downloaded_is_not_picked_while_captions_run() {
        let mut m = status(vec![ModelId::WhisperTurbo], "CUDA 0");
        assert!(can_pick(ModelId::WhisperLarge, &m));
        m.running = true;
        assert!(can_pick(ModelId::WhisperTurbo, &m));
        assert!(!can_pick(ModelId::WhisperLarge, &m));
        m.running = false;
        m.loading = true;
        assert!(!can_pick(ModelId::Gemma12b, &m));
        // The Natural translator can be picked with either of its models.
        assert_eq!(natural_light(&m), None);
        m.cached.push(ModelId::GemmaE4b);
        assert_eq!(natural_light(&m), Some(true));
        m.cached.push(ModelId::Gemma12b);
        assert_eq!(natural_light(&m), Some(false));
        m.loading = false;
        m.cached.clear();
        assert_eq!(natural_light(&m), Some(false));
    }

    #[test]
    fn says_what_helps_a_model_keep_up() {
        let gpu = status(Vec::new(), "CUDA 0");
        let cpu = status(Vec::new(), "CPU");
        let s = Settings {
            translator: TranslatorChoice::Natural { light: false },
            model: ModelChoice::Tiny,
            ..Settings::default()
        };
        assert_eq!(
            translate_advice(&s, &gpu),
            "The graphics card is busy. The smaller model needs less."
        );
        let light = Settings {
            translator: TranslatorChoice::Natural { light: true },
            ..s.clone()
        };
        assert_eq!(translate_advice(&light, &gpu), "The graphics card is busy.");
        let fast = Settings {
            translator: TranslatorChoice::Builtin,
            ..s.clone()
        };
        assert_eq!(
            translate_advice(&fast, &cpu),
            "This computer is slow for it."
        );
        assert_eq!(translate_advice(&fast, &gpu), "The graphics card is busy.");
        let own = Settings {
            translator: TranslatorChoice::Llm {
                base_url: OLLAMA_URL.into(),
                model: OLLAMA_MODEL.into(),
            },
            ..s.clone()
        };
        assert_eq!(translate_advice(&own, &gpu), "Your server is slow.");

        let too_slow = if cfg!(windows) {
            "This computer is too slow to listen. Try Subtitles on screen."
        } else {
            "This computer is too slow to listen."
        };
        assert_eq!(listen_advice(&s, &cpu), too_slow);
        assert_eq!(listen_advice(&s, &gpu), too_slow);
        for model in [ModelChoice::Auto, ModelChoice::Base, ModelChoice::Large] {
            let bigger = Settings { model, ..s.clone() };
            assert_eq!(
                listen_advice(&bigger, &cpu),
                "A smaller model keeps up better."
            );
        }
    }

    #[test]
    fn a_model_is_in_use_only_while_captions_use_it() {
        let mut s = Settings {
            translate: true,
            translator: TranslatorChoice::Natural { light: false },
            asr_backend: AsrBackend::Whisper,
            model: ModelChoice::Turbo,
            speak: false,
            ..Settings::default()
        };
        let mut m = status(Vec::new(), "CUDA 0");
        assert!(!in_use(ModelId::Gemma12b, &s, &m));
        m.running = true;
        assert!(in_use(ModelId::Gemma12b, &s, &m));
        assert!(!in_use(ModelId::GemmaE4b, &s, &m));
        // The Natural translator does not load the Fast one.
        assert!(!in_use(ModelId::Madlad3bQ4k, &s, &m));
        assert!(in_use(ModelId::WhisperTurbo, &s, &m));
        assert!(!in_use(ModelId::WhisperLarge, &s, &m));
        assert!(!in_use(ModelId::Supertonic, &s, &m));

        s.translator = TranslatorChoice::Builtin;
        assert!(in_use(ModelId::Madlad3bQ4k, &s, &m));
        assert!(!in_use(ModelId::Gemma12b, &s, &m));
        s.translate = false;
        assert!(!in_use(ModelId::Madlad3bQ4k, &s, &m));

        s.asr_backend = AsrBackend::ScreenText;
        assert!(!in_use(ModelId::WhisperTurbo, &s, &m));
    }

    #[test]
    fn says_what_the_first_start_downloads() {
        let base = ModelId::WhisperBase;
        let s = Settings {
            translate: true,
            translator: TranslatorChoice::Builtin,
            model: ModelChoice::Auto,
            ..Settings::default()
        };
        let m = status(Vec::new(), "CPU");
        assert_eq!(pending_downloads(&s, &m), [base, ModelId::Madlad3bQ4k]);
        assert_eq!(
            download_note(&pending_downloads(&s, &m)),
            "Starting downloads the speech and translation models first, 2.0 GB, once."
        );
        assert_eq!(
            pending_downloads(&s, &status(Vec::new(), "CUDA 0"))[0],
            ModelId::WhisperTurbo
        );
        let m = status(vec![ModelId::Madlad3bQ4k], "CPU");
        assert_eq!(
            download_note(&pending_downloads(&s, &m)),
            "Starting downloads the speech model first, 290 MB, once."
        );
        assert!(pending_downloads(&s, &status(vec![base, ModelId::Madlad3bQ4k], "CPU")).is_empty());
        let off = Settings {
            translate: false,
            ..s.clone()
        };
        assert_eq!(pending_downloads(&off, &status(Vec::new(), "CPU")), [base]);
        let mut downloading = status(Vec::new(), "CPU");
        downloading.downloading.push((base, 1, None));
        assert_eq!(pending_downloads(&off, &downloading), Vec::<ModelId>::new());
    }
}
