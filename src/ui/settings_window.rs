use eframe::egui::{
    self, Align, Color32, ComboBox, Context, CursorIcon, Event, FontId, Key, Layout, Modifiers,
    ProgressBar, Rect, RichText, ScrollArea, Sense, Slider, Stroke, TextStyle, Theme, Ui,
    UiBuilder, Visuals, WidgetInfo, pos2, vec2,
};

use super::icons;

use crate::audio::{DeviceInfo, DeviceList};
use crate::models::{ModelId, spec};
use crate::settings::{
    AsrBackend, CaptionStyle, Edge, ModelChoice, SOURCE_LANGS, Settings, TARGET_LANGS,
    TranslatorChoice, lang_name,
};
use crate::tts::{self, Persona, Style, Voice, supertonic};
use crate::types::AudioSource;

const OLLAMA_URL: &str = "http://localhost:11434/v1";
const OLLAMA_MODEL: &str = "qwen2.5:3b";
const NATURAL_NOTE: &str = "Understands slang, idiom and tone. Needs a graphics card with 8 GB of memory free, or 3.5 GB \
     with the smaller model.";
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
}

pub enum SettingsAction {
    Changed,
    Download(ModelId),
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

const PAGES: [(Page, &str); 5] = [
    (Page::Listen, "Listen"),
    (Page::Translate, "Translate"),
    (Page::ReadAloud, "Read aloud"),
    (Page::Captions, "Captions"),
    (Page::General, "General"),
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
                        self.sidebar(ui);
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
                                    Page::Listen => {
                                        audio(ui, s, m, c, a);
                                        speech(ui, s, m, c, a);
                                    }
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

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Hammurabi").strong().size(16.0));
        });
        ui.add_space(10.0);
        for (page, label) in PAGES {
            if nav_item(ui, label, self.page == page) {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.weak(concat!("v", env!("CARGO_PKG_VERSION")));
            });
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
            (false, _) => (ui.visuals().weak_text_color(), "Captions are off"),
            (true, true) => (WARN, "Getting ready\u{2026}"),
            (true, false) => (accent, "Captions are on"),
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, dot);
            ui.label(RichText::new(state).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let button = if m.running {
                    egui::Button::new("Stop captions")
                } else {
                    let text = ui.visuals().selection.stroke.color;
                    egui::Button::new(RichText::new("Start captions").strong().color(text))
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
                    ui.label("Hammurabi runs from the tray icon. Closing this window keeps it there. Pick what to listen to, then press Start captions.");
                    if ui.button("Got it").clicked() {
                        self.welcome = false;
                    }
                });
        }
        if !m.status.is_empty() {
            let text = RichText::new(&m.status);
            ui.label(if m.alert {
                text.color(WARN)
            } else {
                text.weak()
            });
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
            "Press the keys\u{2026}".to_string()
        } else {
            hotkey_label(value)
        };
        let hint = if recording {
            "Esc cancels"
        } else {
            "Click, then press the new shortcut"
        };
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
        section(ui, "App");
        let mut autostart = m.autostart;
        if ui
            .checkbox(&mut autostart, "Start Hammurabi at login")
            .changed()
        {
            actions.push(SettingsAction::ToggleAutostart);
        }
        *changed |= ui
            .checkbox(&mut s.check_updates, "Check for updates")
            .on_hover_text(
                "Asks GitHub once a day whether a newer version is out. Nothing else is sent.",
            )
            .changed();

        section(ui, "Shortcuts");
        let toggle = s.hotkey_toggle.clone();
        grid(ui, "hotkeys", |ui| {
            self.hotkey_row(ui, "Start / stop captions", &toggle);
        });

        section(ui, "Files");
        ui.horizontal(|ui| {
            if ui.button("Open models folder").clicked() {
                actions.push(SettingsAction::OpenModelsFolder);
            }
            if ui.button("Open logs").clicked() {
                actions.push(SettingsAction::OpenLogs);
            }
        });
    }
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
    let mut out = Vec::new();
    if !lc {
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
    let names: Vec<&str> = pending
        .iter()
        .map(|id| match id {
            ModelId::Madlad3bQ4k | ModelId::Gemma12b | ModelId::GemmaE4b => "translation",
            _ => "speech",
        })
        .collect();
    let bytes: u64 = pending.iter().map(|&id| spec(id).approx_bytes).sum();
    format!(
        "Starting downloads the {} model{} first, {}, once.",
        names.join(" and "),
        if pending.len() > 1 { "s" } else { "" },
        size_label(bytes)
    )
}

#[cfg(windows)]
const LISTEN_NOTE: &str = "System audio keeps being captioned while a voice reads, on Windows 11 with the default device, or when the voice plays on another device than the one captioned. Otherwise, and for the microphone, listening pauses so the voice is not captioned again.";
#[cfg(not(windows))]
const LISTEN_NOTE: &str = "System audio keeps being captioned while a voice reads when it plays on another device than the one captioned. Otherwise, and for the microphone, listening pauses so the voice is not captioned again.";

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
        .width(240.0)
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
    section(ui, "What to caption");
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
        ui.colored_label(WARN, format!("System audio unavailable: {e}"));
    }
    ui.add_space(4.0);
    grid(ui, "audio", |ui| {
        ui.add_enabled_ui(s.source != AudioSource::Microphone, |ui| {
            ui.label("System audio from")
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(s.source != AudioSource::Microphone, |ui| {
                device_combo(ui, "out", &m.devices.outputs, &mut s.output_device, changed);
            });
            if ui.button("Refresh").clicked() {
                actions.push(SettingsAction::RefreshDevices);
            }
        });
        ui.end_row();
        ui.add_enabled_ui(s.source != AudioSource::System, |ui| ui.label("Microphone"));
        ui.add_enabled_ui(s.source != AudioSource::System, |ui| {
            device_combo(ui, "mic", &m.devices.inputs, &mut s.input_device, changed);
        });
        ui.end_row();
    });
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("about {:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("about {} MB", bytes / 1_000_000)
    }
}

fn size_text(id: ModelId) -> String {
    format!("{} MB", spec(id).approx_bytes / 1_000_000)
}

/// Check mark, Download button or progress bar for one model.
fn model_status(ui: &mut Ui, id: ModelId, m: &ModelsStatus, actions: &mut Vec<SettingsAction>) {
    ui.horizontal(|ui| {
        ui.label(format!("{} ({})", spec(id).label, size_text(id)));
        match m.downloading.iter().find(|d| d.0 == id) {
            Some(&(_, done, total)) => {
                let bar = match total {
                    Some(t) if t > 0 => ProgressBar::new(done as f32 / t as f32).show_percentage(),
                    _ => ProgressBar::new(0.0).animate(true),
                };
                ui.add(bar.desired_width(160.0));
            }
            _ if m.cached.contains(&id) => {
                ui.colored_label(Color32::from_rgb(90, 190, 110), "Downloaded");
            }
            _ => {
                if ui.button("Download").clicked() {
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
    section(ui, "Speech recognition");
    let gpu = m.compute != "CPU";
    let lc = cfg!(all(windows, feature = "win-livecaptions"))
        && s.asr_backend == AsrBackend::WindowsLiveCaptions;
    grid(ui, "speech", |ui| {
        if cfg!(all(windows, feature = "win-livecaptions")) {
            ui.label("Speech engine");
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    for (v, label) in [
                        (AsrBackend::Whisper, "Whisper (local)"),
                        (AsrBackend::WindowsLiveCaptions, "Windows Live Captions"),
                    ] {
                        *changed |= ui.radio_value(&mut s.asr_backend, v, label).changed();
                    }
                });
                if lc {
                    ui.weak("Language and microphone come from Windows Live Captions settings.");
                }
            });
            ui.end_row();
        }

        ui.label("Spoken language");
        let current = s
            .spoken_language
            .as_deref()
            .map_or("Auto-detect", lang_name)
            .to_string();
        ui.add_enabled_ui(!lc, |ui| {
            ComboBox::from_id_salt("spoken")
                .selected_text(current)
                .width(200.0)
                .show_ui(ui, |ui| {
                    *changed |= ui
                        .selectable_value(&mut s.spoken_language, None, "Auto-detect")
                        .changed();
                    for c in SOURCE_LANGS {
                        *changed |= ui
                            .selectable_value(
                                &mut s.spoken_language,
                                Some(c.to_string()),
                                lang_name(c),
                            )
                            .changed();
                    }
                })
        });
        ui.end_row();

        ui.label("Words to expect");
        ui.add_enabled_ui(!lc, |ui| {
            *changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut s.hint)
                        .hint_text("Names and unusual words, e.g. Lamar, Los Santos")
                        .desired_width(260.0),
                )
                .on_hover_text("Helps the speech model hear and spell names, places and slang.")
                .changed();
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
        ui.add_enabled_ui(!lc, |ui| {
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
                })
        });
        ui.end_row();

        ui.label("Status");
        model_status(ui, ModelId::whisper(s.model, gpu), m, actions);
        ui.end_row();

        ui.label("Speed");
        *changed |= ui
            .checkbox(&mut s.trim_audio, "Fast mode (skip silence padding)")
            .on_hover_text("Encodes only the speech instead of a full 30 s window. Turn off if captions get worse.")
            .changed();
        ui.end_row();

        ui.label("Runs on");
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
    ui.add_space(4.0);
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
            // 0 = MADLAD, 1 = Natural, 2 = the user's own server.
            let was = match s.translator {
                TranslatorChoice::Builtin => 0,
                TranslatorChoice::Natural { .. } => 1,
                TranslatorChoice::Llm { .. } => 2,
            };
            let mut now = was;
            ui.vertical(|ui| {
                ui.radio_value(&mut now, 0, "Fast (MADLAD)")
                    .on_hover_text("Word for word. Runs on any computer.");
                if cfg!(windows) {
                    ui.radio_value(&mut now, 1, "Natural (language model)")
                        .on_hover_text(NATURAL_NOTE);
                }
                ui.radio_value(&mut now, 2, "Your own LLM server")
                    .on_hover_text("Ollama, LM Studio or any OpenAI-compatible server.");
            });
            if now != was {
                s.translator = match now {
                    0 => TranslatorChoice::Builtin,
                    1 => TranslatorChoice::Natural { light: false },
                    _ => TranslatorChoice::Llm {
                        base_url: OLLAMA_URL.into(),
                        model: OLLAMA_MODEL.into(),
                    },
                };
                *changed = true;
            }
            ui.end_row();

            match &mut s.translator {
                TranslatorChoice::Builtin => {
                    ui.label("Model");
                    model_status(ui, ModelId::Madlad3bQ4k, m, actions);
                    ui.end_row();
                }
                TranslatorChoice::Natural { light } => {
                    ui.label("Model");
                    ui.vertical(|ui| {
                        model_status(ui, ModelId::natural(*light), m, actions);
                        *changed |= ui
                            .checkbox(light, "Smaller model")
                            .on_hover_text(
                                "Takes 3.5 GB of graphics memory instead of 8, and translates a \
                                 little less naturally. For when a game runs on the same card.",
                            )
                            .changed();
                    });
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
                    ui.label(p.description);
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
                    .on_hover_text(if playing { "Stop" } else { "Play sample" })
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
fn engine_row(ui: &mut Ui, s: &mut Settings, v: tts::Engine, title: &str, about: &str) -> bool {
    let changed = ui.radio_value(&mut s.speak_engine, v, title).changed();
    indented(ui, |ui| {
        ui.weak(about);
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
    section(ui, "Read aloud");
    *changed |= ui
        .checkbox(&mut s.speak, "Read captions aloud")
        .on_hover_text("Speaks each finished caption, the translation when there is one.")
        .changed();
    if let Some(voice) = &m.reading {
        ui.weak(format!("Reading with {voice}"));
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
        section(ui, "Voice engine");
        *changed |= engine_row(
            ui,
            s,
            tts::Engine::System,
            "System voices",
            "The voices installed on this computer.",
        );
        *changed |= engine_row(
            ui,
            s,
            tts::Engine::Supertonic,
            "Supertonic 3",
            "Natural voices that run on this computer, for 22 languages. Others use system voices.",
        );
        if s.speak_engine == tts::Engine::Supertonic {
            indented(ui, |ui| model_status(ui, ModelId::Supertonic, m, actions));
            if !m.cached.contains(&ModelId::Supertonic) {
                indented(ui, |ui| {
                    ui.colored_label(WARN, "System voices read until it is downloaded.");
                });
            }
        }
        #[cfg(not(omnivoice))]
        {
            ui.add_enabled(false, egui::RadioButton::new(false, "Speaker's voice"))
                .on_disabled_hover_text("Comes with the Windows GPU build");
            indented(ui, |ui| {
                ui.weak("Reads in the voice of whoever is speaking. Comes with the Windows GPU build.");
            });
        }
        #[cfg(omnivoice)]
        {
            *changed |= engine_row(
                ui,
                s,
                tts::Engine::Speaker,
                "Speaker's voice",
                "Reads in the voice of whoever is speaking, learned on this computer from a few seconds of their speech. Needs a GPU with Vulkan.",
            );
            if s.speak_engine == tts::Engine::Speaker {
                speaker_voice(ui, s, m, changed, actions);
            }
        }

        let cloning = s.speak_engine == tts::Engine::Speaker;
        section(ui, if cloning { "Backup voice" } else { "Voice" });
        if cloning {
            ui.weak("Reads until the speaker's voice is learned, and if it cannot run.");
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
            let voice = match (reader.filter(|_| neural), style.resolve(&m.voices, &sample_lang)) {
                (Some((_, v, _)), _) => format!("Supertonic 3 {v}"),
                (_, Ok((Some(v), ..))) => format!("{} ({})", v.name, v.lang),
                (_, Ok((None, ..))) => "System voice".into(),
                (_, Err(_)) => format!("No {} voice", lang_name(&sample_lang)),
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

        section(ui, "Playback");
        grid(ui, "playback", |ui| {
            ui.label("Volume");
            *changed |= ui
                .add(
                    Slider::new(&mut s.speak_volume, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .changed();
            ui.end_row();
            ui.label("Read on");
            device_combo(
                ui,
                "speak_out",
                &m.devices.outputs,
                &mut s.speak_device,
                changed,
            );
            ui.end_row();
            if crate::audio::duck::SUPPORTED {
                ui.label("Other apps while reading");
                *changed |= ui
                    .add(
                        Slider::new(&mut s.speak_duck, 0.1..=1.0).custom_formatter(|v, _| {
                            if v >= 1.0 {
                                "Not turned down".into()
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
            let label = format!("Also read speech already in {}", lang_name(&s.target_language));
            *changed |= ui
                .checkbox(&mut s.speak_same_lang, label)
                .on_hover_text("Off: speech you can already understand is heard as it is, and not read over itself.")
                .changed();
        }
        ui.weak(LISTEN_NOTE);
        ui.weak("When captions pile up, they are read together and a little faster.");
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
                .checkbox(
                    &mut s.omnivoice_accepted,
                    "I will use it for personal, non-commercial use only",
                )
                .changed();
        });
    } else {
        indented(ui, |ui| model_status(ui, ModelId::OmniVoice, m, actions));
        if m.cached.contains(&ModelId::OmniVoice) {
            indented(ui, |ui| {
                ui.label(if m.voice_learned {
                    "Reading in the speaker's voice."
                } else {
                    "Listening for a few seconds of speech to learn the voice."
                });
                if m.voice_learned && ui.button("Learn again").clicked() {
                    actions.push(SettingsAction::RelearnVoice);
                }
            });
        }
    }
    indented(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.weak("OmniVoice by k2-fsa, CC-BY-NC-4.0: for personal, non-commercial use only (");
        ui.hyperlink_to("license", "https://huggingface.co/k2-fsa/OmniVoice#license");
        ui.weak("). Do not use it to pass yourself off as someone.");
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
        ui.label("Voice");
        let label = |v: &Voice| format!("{} ({})", v.name, v.lang);
        let current = match &s.speak_voice {
            None if neural => format!("Supertonic 3 {}", s.speak_neural_voice),
            None => "Automatic".to_string(),
            Some(id) => m
                .voices
                .iter()
                .find(|v| &v.id == id)
                .map_or_else(|| format!("{id} (missing)"), label),
        };
        ComboBox::from_id_salt("voice")
            .selected_text(current)
            .width(260.0)
            .show_ui(ui, |ui| {
                if neural {
                    for v in supertonic::STYLES {
                        let on = s.speak_voice.is_none() && s.speak_neural_voice == v;
                        if ui
                            .selectable_label(on, format!("Supertonic 3 {v}"))
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
                        .selectable_value(&mut s.speak_voice, None, "Automatic")
                        .on_hover_text("The system voice for each caption's language.")
                        .changed();
                }
                for v in voices {
                    *changed |= ui
                        .selectable_value(&mut s.speak_voice, Some(v.id.clone()), label(v))
                        .changed();
                }
            });
        ui.end_row();

        ui.label("Speed");
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
    section(ui, "Position");
    ui.weak(
        "Click and hold any caption to drag it. A sample is on screen while this page is open.",
    );
    if ui
        .add_enabled(s.overlay_pos.is_some(), egui::Button::new("Reset position"))
        .clicked()
    {
        actions.push(SettingsAction::ResetPosition);
    }
    *changed |= ui
        .checkbox(&mut s.overlay_locked, "Lock in place")
        .on_hover_text("Captions cannot be dragged, and a click on one goes to the app under it.")
        .changed();

    section(ui, "Streaming");
    *changed |= ui
        .checkbox(&mut s.browser_source, "Captions as a web page")
        .on_hover_text("For a stream: the caption window is not part of a captured game.")
        .changed();
    if s.browser_source {
        ui.horizontal(|ui| {
            ui.weak("In OBS, add a Browser source with this address:");
            let url = super::web::Web::url();
            if ui.link(&url).on_hover_text("Copy").clicked() {
                ui.ctx().copy_text(url);
            }
        });
    }

    section(ui, "Look");
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

        ui.label("Captions on screen");
        *changed |= ui.add(Slider::new(&mut c.cues, 1..=3)).changed();
        ui.end_row();

        ui.label("Keep on screen");
        *changed |= ui
            .add(
                Slider::new(&mut c.hold_secs, 1.0..=15.0)
                    .step_by(0.5)
                    .suffix(" s"),
            )
            .changed();
        ui.end_row();

        ui.label("Text edge");
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
    ui.add_space(4.0);
    if ui
        .add_enabled(
            *c != CaptionStyle::default(),
            egui::Button::new("Reset look"),
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
    if !(mods.ctrl || mods.alt || mods.mac_cmd) {
        return None;
    }
    let name = key.name();
    let code = match name.as_bytes() {
        [c] if c.is_ascii_uppercase() => format!("Key{name}"),
        [c] if c.is_ascii_digit() => format!("Digit{name}"),
        [b'F', rest @ ..] if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit) => {
            name.to_string()
        }
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
            autostart: false,
            voices: Vec::new(),
            running: false,
            loading: false,
            status: String::new(),
            alert: false,
            reading: None,
            previewing: None,
            voice_learned: false,
        }
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
        assert_eq!(text(alt_shift, Key::T).unwrap(), "alt+shift+KeyT");
        assert_eq!(text(Modifiers::CTRL, Key::Num1).unwrap(), "ctrl+Digit1");
        assert_eq!(text(Modifiers::ALT, Key::F5).unwrap(), "alt+F5");
        assert_eq!(text(Modifiers::CTRL, Key::Space).unwrap(), "ctrl+Space");
        assert_eq!(text(Modifiers::NONE, Key::T), None);
        assert_eq!(text(Modifiers::SHIFT, Key::T), None);
        assert_eq!(text(Modifiers::ALT, Key::Enter), None);
        for k in [Key::T, Key::Num1, Key::F5, Key::Space] {
            let s = text(alt_shift, k).unwrap();
            assert!(s.parse::<global_hotkey::hotkey::HotKey>().is_ok(), "{s}");
        }
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
            "Starting downloads the speech and translation models first, about 2.0 GB, once."
        );
        assert_eq!(
            pending_downloads(&s, &status(Vec::new(), "CUDA 0"))[0],
            ModelId::WhisperTurbo
        );
        let m = status(vec![ModelId::Madlad3bQ4k], "CPU");
        assert_eq!(
            download_note(&pending_downloads(&s, &m)),
            "Starting downloads the speech model first, about 290 MB, once."
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
