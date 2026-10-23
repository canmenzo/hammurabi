pub mod autostart;
pub mod hotkey;
pub mod icons;
pub mod overlay;
pub mod platform;
pub mod settings_window;
pub mod tray;
pub mod update;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Context, ViewportBuilder, ViewportCommand, ViewportId};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use tray_icon::menu::{MenuEvent, MenuId};

use crate::audio::{self, DeviceList};
use crate::config;
use crate::models::{self, ModelId};
use crate::pipeline::{Engine, EngineHandle};
use crate::settings::{Settings, lang_name};
use crate::tts;
use crate::types::{
    AudioSource, CaptionUpdate, EngineCommand, EngineEvent, EngineState, ProgressPhase, Status,
    StatusLevel,
};
use overlay::OverlayState;
use settings_window::{ModelsStatus, SettingsAction, SettingsWindow};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
const WEBSITE: &str = "https://hammurabi.menzo.dev";

/// Sent from tray/menu/hotkey handlers (any thread) to the UI loop.
pub enum AppEvent {
    Menu(MenuId),
    HotKey(u32),
    Update(update::Release),
}

#[derive(Clone, Copy, Debug)]
pub enum UiAction {
    ToggleRunning,
    SetSource(AudioSource),
    ToggleTranslate,
    SetTarget(&'static str),
    ToggleSpeak,
    SetPersona(&'static str),
    #[cfg(omnivoice)]
    ToggleSpeakerVoice,
    #[cfg(omnivoice)]
    RelearnVoice,
    ToggleMove,
    OpenSettings,
    ToggleAutostart,
    OpenUpdate,
    About,
    OpenLogs,
    Quit,
}

pub struct App {
    settings: Settings,
    engine: Option<EngineHandle>,
    tray: Option<tray::Tray>,
    hotkeys: hotkey::Hotkeys,
    overlay: OverlayState,
    settings_win: SettingsWindow,
    models: ModelsStatus,
    app_rx: Receiver<AppEvent>,
    running: bool,
    state: EngineState,
    move_mode: bool,
    settings_open: bool,
    shown: bool,
    first_frame: bool,
    passthrough: bool,
    layout: Option<(egui::Pos2, egui::Vec2)>,
    status: String,
    dirty_since: Option<Instant>,
    tray_dirty: bool,
    update: Option<update::Release>,
    next_update_check: Option<Instant>,
    app_tx: Sender<AppEvent>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (app_tx, app_rx) = crossbeam_channel::unbounded();
        install_handlers(&ctx, app_tx.clone());

        let tray = tray::Tray::new()
            .inspect_err(|e| log::error!("tray icon failed: {e:#}"))
            .ok();
        let (hotkeys, hk_err) = hotkey::Hotkeys::new();
        let notifier_ctx = ctx.clone();
        let engine = Engine::spawn(Arc::new(move || notifier_ctx.request_repaint()));

        let mut app = Self {
            settings,
            engine: Some(engine),
            tray,
            hotkeys,
            overlay: OverlayState::default(),
            settings_win: SettingsWindow::default(),
            models: ModelsStatus {
                cached: Vec::new(),
                downloading: Vec::new(),
                devices: DeviceList {
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    system_audio: Ok(()),
                },
                compute: crate::device::label(),
                autostart: autostart::is_enabled(),
                voices: Vec::new(),
            },
            app_rx,
            running: false,
            state: EngineState::Idle,
            move_mode: false,
            settings_open: false,
            // eframe shows the root after its first painted frame regardless of `with_visible(false)`,
            // so the first logic pass hides it again.
            shown: true,
            first_frame: true,
            passthrough: true,
            layout: None,
            status: String::new(),
            dirty_since: None,
            tray_dirty: true,
            update: None,
            next_update_check: None,
            app_tx,
        };
        if let Some(e) = hk_err.or_else(|| app.hotkeys.apply(&app.settings)) {
            app.warn(e);
        }
        if std::env::args().any(|a| a == "--start") {
            app.handle(UiAction::ToggleRunning, &ctx);
        }
        log::info!("hammurabi ready (tray: {})", app.tray.is_some());
        app
    }

    fn engine(&self) -> &EngineHandle {
        self.engine.as_ref().expect("engine lives until quit")
    }

    fn warn(&mut self, text: String) {
        log::warn!("{text}");
        self.status = text.clone();
        self.overlay.set_status(
            Status {
                level: StatusLevel::Warning,
                text,
                ttl: Some(Duration::from_secs(6)),
            },
            Instant::now(),
        );
        self.tray_dirty = true;
    }

    /// Saving and reconfiguring the engine wait for `SAVE_DEBOUNCE` without further changes,
    /// so typing a URL does not restart the translator on every key.
    fn changed(&mut self) {
        self.dirty_since = Some(Instant::now());
        self.tray_dirty = true;
    }

    fn handle(&mut self, action: UiAction, ctx: &Context) {
        log::info!("action {action:?}");
        match action {
            UiAction::ToggleRunning => {
                self.running = !self.running;
                self.overlay.clear_status();
                self.status.clear();
                let cmd = if self.running {
                    EngineCommand::Start(self.settings.clone())
                } else {
                    EngineCommand::Stop
                };
                log::debug!("engine <- {}", if self.running { "Start" } else { "Stop" });
                self.engine().send(cmd);
                self.tray_dirty = true;
            }
            UiAction::SetSource(s) => {
                self.settings.source = s;
                self.changed();
            }
            UiAction::ToggleTranslate => {
                self.settings.translate = !self.settings.translate;
                self.changed();
            }
            UiAction::SetTarget(code) => {
                self.settings.target_language = code.into();
                self.changed();
            }
            UiAction::ToggleSpeak => {
                self.settings.speak = !self.settings.speak;
                self.changed();
            }
            UiAction::SetPersona(id) => {
                self.settings.speak_persona = id.into();
                self.changed();
            }
            #[cfg(omnivoice)]
            UiAction::ToggleSpeakerVoice => {
                let s = &mut self.settings;
                s.speak_engine = if s.speak_engine == tts::Engine::Speaker {
                    if models::is_cached(ModelId::Supertonic) {
                        tts::Engine::Supertonic
                    } else {
                        tts::Engine::System
                    }
                } else {
                    tts::Engine::Speaker
                };
                let download = s.speak_engine == tts::Engine::Speaker
                    && !models::is_cached(ModelId::OmniVoice);
                self.changed();
                // The licence note and the download are in Settings > Read aloud.
                if download {
                    self.handle(UiAction::OpenSettings, ctx);
                }
            }
            #[cfg(omnivoice)]
            UiAction::RelearnVoice => self.engine().send(EngineCommand::RelearnVoice),
            UiAction::ToggleMove => self.set_move(!self.move_mode, ctx),
            UiAction::OpenSettings => {
                if !self.settings_open {
                    self.settings_open = true;
                    platform::activate_app();
                    self.refresh_models();
                    self.models.devices = audio::list_devices();
                    self.models.autostart = autostart::is_enabled();
                    self.models.voices = tts::voices();
                }
            }
            UiAction::ToggleAutostart => {
                if let Err(e) = autostart::set(!self.models.autostart) {
                    self.warn(format!("Could not change start at login: {e:#}"));
                }
                self.models.autostart = autostart::is_enabled();
                log::info!("start at login: {}", self.models.autostart);
                self.tray_dirty = true;
            }
            UiAction::OpenUpdate => {
                if let Some(u) = &self.update {
                    let _ = open::that_detached(&u.url);
                }
            }
            UiAction::About => {
                let _ = open::that_detached(WEBSITE);
            }
            UiAction::OpenLogs => {
                if let Ok(dir) = crate::paths::log_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = open::that_detached(dir);
                }
            }
            UiAction::Quit => self.quit(),
        }
    }

    fn set_move(&mut self, on: bool, ctx: &Context) {
        if self.move_mode == on {
            return;
        }
        self.move_mode = on;
        self.tray_dirty = true;
        if !on && let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            self.settings.overlay_pos = Some([rect.min.x, rect.min.y]);
            self.layout = None;
            self.dirty_since = Some(Instant::now());
        }
    }

    fn refresh_models(&mut self) {
        self.models.cached = ModelId::ALL
            .iter()
            .copied()
            .filter(|&m| models::is_cached(m))
            .collect();
    }

    fn quit(&mut self) {
        log::info!("quit");
        if let Err(e) = config::save(&self.settings) {
            log::error!("config save failed: {e:#}");
        }
        if let Some(engine) = self.engine.take() {
            engine.shutdown(Duration::from_secs(1));
        }
        std::process::exit(0);
    }

    fn on_engine(&mut self, event: EngineEvent) {
        let now = Instant::now();
        match event {
            EngineEvent::State(s) => {
                self.state = s;
                if matches!(s, EngineState::Idle | EngineState::Error) {
                    self.running = false;
                }
                if s == EngineState::Running {
                    self.status.clear();
                }
                self.tray_dirty = true;
            }
            EngineEvent::Status(s) => {
                log::info!("status: {}", s.text);
                // Failed downloads send no final progress; drop the bars (running ones come back).
                if matches!(s.level, StatusLevel::Error | StatusLevel::Warning) {
                    self.models.downloading.clear();
                }
                self.status = s.text.clone();
                self.overlay.set_status(s, now);
                self.tray_dirty = true;
            }
            EngineEvent::Progress(p) => {
                self.models.downloading.retain(|d| d.0 != p.model);
                let text = match p.phase {
                    ProgressPhase::Downloading if p.total.is_none_or(|t| p.done < t) => {
                        self.models.downloading.push((p.model, p.done, p.total));
                        let parts: Vec<String> = self
                            .models
                            .downloading
                            .iter()
                            .map(|&(m, done, total)| {
                                let pct = total
                                    .map(|t| format!(" {}%", done * 100 / t.max(1)))
                                    .unwrap_or_default();
                                format!("{}{pct}", models::spec(m).label)
                            })
                            .collect();
                        Some(format!("Downloading {}", parts.join(", ")))
                    }
                    ProgressPhase::Downloading => {
                        self.refresh_models();
                        None
                    }
                    ProgressPhase::Loading => {
                        self.refresh_models();
                        Some(format!("Loading {}…", models::spec(p.model).label))
                    }
                };
                if let Some(text) = text {
                    self.status = text.clone();
                    let status = Status {
                        level: StatusLevel::Info,
                        text,
                        ttl: Some(Duration::from_secs(5)),
                    };
                    self.overlay.set_status(status, now);
                }
                self.tray_dirty = true;
            }
            EngineEvent::Caption(c) => {
                if let CaptionUpdate::Final {
                    id,
                    original,
                    translated,
                    lang,
                } = &c
                {
                    log::debug!("final #{id} ({lang}) {original:?} => {translated:?}");
                }
                self.overlay.apply(c, &self.settings, now)
            }
            EngineEvent::Stats { device, .. } => self.models.compute = device,
        }
    }

    fn on_update(&mut self, release: update::Release) {
        if !self.settings.check_updates {
            return;
        }
        log::info!("update available: v{} {}", release.version, release.url);
        if let Some(t) = &mut self.tray
            && let Err(e) = t.show_update(&release.version)
        {
            log::warn!("tray update item failed: {e:#}");
        }
        if self.settings.update_notified.as_ref() != Some(&release.version) {
            self.overlay.set_status(
                Status {
                    level: StatusLevel::Info,
                    text: format!(
                        "Hammurabi v{} is available. Open the tray menu to get it.",
                        release.version
                    ),
                    ttl: Some(Duration::from_secs(10)),
                },
                Instant::now(),
            );
            self.settings.update_notified = Some(release.version.clone());
            if let Err(e) = config::save(&self.settings) {
                log::error!("config save failed: {e:#}");
            }
        }
        self.update = Some(release);
        self.tray_dirty = true;
    }

    fn tooltip(&self) -> String {
        let state = match (self.running, self.state) {
            (false, _) => "Idle",
            (true, EngineState::Loading) => "Loading…",
            (true, _) => "Captions on",
        };
        let mut t = format!("Hammurabi: {state}");
        if self.settings.translate {
            t += &format!(" → {}", lang_name(&self.settings.target_language));
        }
        if !self.status.is_empty() {
            t += &format!("\n{}", self.status);
        }
        t
    }
}

fn install_handlers(ctx: &Context, tx: Sender<AppEvent>) {
    let (c, t) = (ctx.clone(), tx.clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = t.send(AppEvent::Menu(e.id));
        c.request_repaint();
    }));
    // Unhandled tray clicks would pile up in tray-icon's channel.
    tray_icon::TrayIconEvent::set_event_handler(Some(|_| {}));
    let c = ctx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.state() == HotKeyState::Pressed {
            let _ = tx.send(AppEvent::HotKey(e.id()));
            c.request_repaint();
        }
    }));
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.app_rx.try_recv() {
            let action = match ev {
                AppEvent::Menu(id) => tray::Tray::action(&id),
                AppEvent::HotKey(id) => self.hotkeys.action(id),
                AppEvent::Update(r) => {
                    self.on_update(r);
                    None
                }
            };
            if let Some(a) = action {
                self.handle(a, ctx);
            }
        }
        while let Some(ev) = self.engine().try_recv() {
            self.on_engine(ev);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }

        if self.tray_dirty {
            self.tray_dirty = false;
            let tip = self.tooltip();
            if let Some(t) = &mut self.tray {
                t.sync(
                    &self.settings,
                    self.running,
                    self.move_mode,
                    self.models.autostart,
                    &tip,
                );
            }
        }

        let visible =
            self.running || self.settings_open || self.move_mode || self.overlay.has_status();
        if visible != self.shown {
            log::debug!("root visible = {visible}");
            self.shown = visible;
            ctx.send_viewport_cmd(ViewportCommand::Visible(visible));
            if visible {
                ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint();
        }

        let now = Instant::now();
        if self.settings.check_updates {
            let due = *self.next_update_check.get_or_insert(now);
            if now >= due {
                self.next_update_check = Some(now + update::INTERVAL);
                let (tx, c) = (self.app_tx.clone(), ctx.clone());
                update::spawn(move |r| {
                    let _ = tx.send(AppEvent::Update(r));
                    c.request_repaint();
                });
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
        if let Some(t) = self.dirty_since {
            if now >= t + SAVE_DEBOUNCE {
                self.dirty_since = None;
                if let Err(e) = config::save(&self.settings) {
                    log::error!("config save failed: {e:#}");
                }
                if self.running {
                    self.engine()
                        .send(EngineCommand::Reconfigure(self.settings.clone()));
                }
            } else {
                ctx.request_repaint_after(t + SAVE_DEBOUNCE - now);
            }
        }
        if let Some(d) = self.overlay.next_deadline() {
            ctx.request_repaint_after(d.saturating_duration_since(now));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.first_frame) {
            platform::configure_overlay_window(frame);
        }

        if !self.move_mode
            && let Some(monitor) = ctx.input(|i| i.viewport().monitor_size)
        {
            let layout = overlay::layout_for_monitor(
                monitor,
                &self.settings.caption,
                self.settings.overlay_pos,
            );
            if self.layout != Some(layout) {
                self.layout = Some(layout);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(layout.0));
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(layout.1));
            }
        }
        if self.passthrough != !self.move_mode {
            self.passthrough = !self.move_mode;
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(self.passthrough));
        }

        let resp = self
            .overlay
            .paint(ui, &self.settings.caption, self.move_mode);
        if resp.start_drag {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if resp.finish_move {
            self.set_move(false, &ctx);
        }

        if self.settings_open {
            let mut close = false;
            let actions = ctx.show_viewport_immediate(
                ViewportId::from_hash_of("hammurabi-settings"),
                ViewportBuilder::default()
                    .with_title("Hammurabi Settings")
                    .with_icon(icons::window_icon())
                    .with_inner_size([520.0, 680.0]),
                |ui, _| {
                    close = ui.ctx().input(|i| i.viewport().close_requested());
                    self.settings_win
                        .show(ui.ctx(), &mut self.settings, &self.models)
                },
            );
            for a in actions {
                match a {
                    SettingsAction::Changed => {
                        self.layout = None;
                        if let Some(e) = self.hotkeys.apply(&self.settings) {
                            self.warn(e);
                        }
                        self.changed();
                    }
                    SettingsAction::Download(id) => self.engine().send(EngineCommand::Prefetch(id)),
                    SettingsAction::OpenModelsFolder => {
                        if let Ok(dir) = crate::paths::models_dir() {
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = open::that_detached(dir);
                        }
                    }
                    SettingsAction::RefreshDevices => {
                        self.models.devices = audio::list_devices();
                        self.models.voices = tts::voices();
                    }
                    SettingsAction::ToggleAutostart => self.handle(UiAction::ToggleAutostart, &ctx),
                    SettingsAction::OpenLogs => self.handle(UiAction::OpenLogs, &ctx),
                    SettingsAction::Preview(id) => {
                        let lang = self.settings.speak_lang().unwrap_or("en").to_string();
                        let style = tts::Style {
                            persona: id.into(),
                            ..self.settings.speak_style()
                        };
                        let text = tts::sample(&lang, tts::persona(id).name);
                        self.engine()
                            .send(EngineCommand::Preview { text, lang, style });
                    }
                }
            }
            if close {
                self.settings_open = false;
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}
