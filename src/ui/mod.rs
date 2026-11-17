pub mod autostart;
pub mod cursor;
pub mod hotkey;
pub mod icons;
pub mod instance;
pub mod overlay;
pub mod platform;
pub mod settings_window;
pub mod tray;
pub mod update;
pub mod web;

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
use settings_window::{ModelsStatus, Page, SettingsAction, SettingsWindow};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// An engine that dies again within this long is left stopped instead of restarted.
const ENGINE_RESTART_GAP: Duration = Duration::from_secs(30);
const WEBSITE: &str = "https://hammurabi.menzo.dev";
const SETTINGS_ID: &str = "hammurabi-settings";
/// The sample caption is sent this often to the captions page, for a page that connects later.
const WEB_SAMPLE_EVERY: Duration = Duration::from_secs(1);
/// Start without opening a window: what the login entry passes.
pub const BACKGROUND_ARG: &str = "--background";

/// Sent from tray/menu/hotkey handlers (any thread) to the UI loop.
pub enum AppEvent {
    Menu(MenuId),
    HotKey(u32),
    Update(update::Status),
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
    web: Option<web::Web>,
    settings_win: SettingsWindow,
    models: ModelsStatus,
    app_rx: Receiver<AppEvent>,
    running: bool,
    state: EngineState,
    engine_restarted: Option<Instant>,
    /// The window is being dragged by a caption.
    dragging: bool,
    pointer: cursor::Watch,
    settings_open: bool,
    settings_shown: bool,
    shown: bool,
    first_frame: bool,
    root_ready: bool,
    passthrough: bool,
    layout: Option<(egui::Pos2, egui::Vec2)>,
    status: String,
    /// When `status` ends; None keeps it.
    status_until: Option<Instant>,
    alert: bool,
    /// When the sample caption was last sent to the captions page.
    web_sample: Option<Instant>,
    dirty_since: Option<Instant>,
    tray_dirty: bool,
    next_update_check: Option<Instant>,
    /// When the downloaded update is installed, a moment after saying so.
    install_at: Option<Instant>,
    /// "Check for updates" was clicked, which installs even with automatic updates off.
    update_asked: bool,
    app_tx: Sender<AppEvent>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        let ctx = cc.egui_ctx.clone();
        settings_window::apply_theme(&ctx);
        let first_run = crate::paths::config_file().is_ok_and(|p| !p.exists());
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
            web: None,
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
                asr_ms: None,
                tr_ms: None,
                autostart: autostart::is_enabled(),
                voices: Vec::new(),
                running: false,
                loading: false,
                status: String::new(),
                alert: false,
                reading: None,
                previewing: None,
                voice_learned: false,
                update: update::Status::Idle,
            },
            app_rx,
            running: false,
            state: EngineState::Idle,
            engine_restarted: None,
            dragging: false,
            pointer: cursor::Watch::spawn(ctx.clone()),
            settings_open: false,
            settings_shown: false,
            // eframe shows the root after its first painted frame regardless of `with_visible(false)`,
            // so the first logic pass hides it again.
            shown: true,
            first_frame: true,
            root_ready: false,
            passthrough: true,
            layout: None,
            status: String::new(),
            status_until: None,
            alert: false,
            web_sample: None,
            dirty_since: None,
            tray_dirty: true,
            next_update_check: None,
            install_at: None,
            update_asked: false,
            app_tx,
        };
        if let Some(e) = hk_err.or_else(|| app.hotkeys.apply(&app.settings)) {
            app.warn(e);
        }
        app.sync_web();
        // Entries written before --background existed would otherwise open Settings at every login.
        if app.models.autostart
            && let Err(e) = autostart::set(true)
        {
            log::warn!("start at login: {e:#}");
        }
        let has_arg = |name: &str| std::env::args().any(|a| a == name);
        let start = has_arg("--start");
        if start {
            app.handle(UiAction::ToggleRunning, &ctx);
        }
        if first_run {
            app.settings_win.welcome = true;
            if let Err(e) = config::save(&app.settings) {
                log::error!("config save failed: {e:#}");
            }
        }
        if first_run || !(start || has_arg(BACKGROUND_ARG)) {
            app.handle(UiAction::OpenSettings, &ctx);
        }
        let version = env!("CARGO_PKG_VERSION");
        if app.settings.last_version.as_deref() != Some(version) {
            if app.settings.last_version.is_some() {
                app.info(format!("Hammurabi is updated to v{version}."));
            }
            app.settings.last_version = Some(version.into());
            if let Err(e) = config::save(&app.settings) {
                log::error!("config save failed: {e:#}");
            }
        }
        log::info!("hammurabi ready (tray: {})", app.tray.is_some());
        app
    }

    fn engine(&self) -> &EngineHandle {
        self.engine.as_ref().expect("engine lives until quit")
    }

    fn warn(&mut self, text: String) {
        log::warn!("{text}");
        self.set_status(Status {
            level: StatusLevel::Warning,
            text,
            ttl: Some(Duration::from_secs(6)),
        });
    }

    /// Shows a status in Settings, the tray tooltip and the overlay, until its ttl passes.
    fn set_status(&mut self, s: Status) {
        let now = Instant::now();
        self.status = s.text.clone();
        self.status_until = s.ttl.map(|t| now + t);
        self.alert = !matches!(s.level, StatusLevel::Info);
        self.overlay.set_status(s, now);
        self.tray_dirty = true;
    }

    /// Starts the captions page once it is wanted, and tells it how captions look now.
    fn sync_web(&mut self) {
        if self.settings.browser_source && self.web.is_none() {
            match web::Web::spawn() {
                Ok(w) => self.web = Some(w),
                Err(e) => return self.warn(format!("The captions page could not start: {e}")),
            }
        }
        if let Some(w) = &self.web {
            w.style(&self.settings);
            if !self.settings.browser_source {
                w.caption(&CaptionUpdate::Clear, "");
            }
        }
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
                self.alert = false;
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
            UiAction::RelearnVoice => {
                self.models.voice_learned = false;
                self.engine().send(EngineCommand::RelearnVoice);
            }
            UiAction::OpenSettings => {
                if !self.settings_open {
                    self.settings_open = true;
                    platform::activate_app();
                    self.refresh_models();
                    self.models.devices = audio::list_devices();
                    self.models.autostart = autostart::is_enabled();
                    self.models.voices = tts::voices();
                } else {
                    let id = ViewportId::from_hash_of(SETTINGS_ID);
                    ctx.send_viewport_cmd_to(id, ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
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
                if let update::Status::Available(r) = &self.models.update {
                    let _ = open::that_detached(&r.url);
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

    /// The engine thread ended without being asked to, so a panic took it down. Everything sent
    /// to it would go nowhere, and nothing would say so.
    fn engine_died(&mut self, ctx: &Context) {
        log::error!("the engine stopped unexpectedly");
        let c = ctx.clone();
        self.engine = Some(Engine::spawn(Arc::new(move || c.request_repaint())));
        self.models.reading = None;
        self.models.previewing = None;
        self.models.downloading.clear();
        let now = Instant::now();
        let again = self
            .engine_restarted
            .is_some_and(|t| now.duration_since(t) < ENGINE_RESTART_GAP);
        self.engine_restarted = Some(now);
        if !self.running {
            return;
        }
        if again {
            self.running = false;
            self.state = EngineState::Idle;
            self.warn("Captions stopped after an internal error. See the log for details.".into());
        } else {
            self.engine()
                .send(EngineCommand::Start(self.settings.clone()));
            self.warn("Captions hit an internal error and restarted.".into());
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
                // Follows the engine: a Stop then Start before the Stop's Idle came is on again
                // with the Start's Loading.
                self.running = matches!(s, EngineState::Loading | EngineState::Running);
                if !self.running {
                    self.models.asr_ms = None;
                    self.models.tr_ms = None;
                }
                if s == EngineState::Running {
                    self.status.clear();
                    self.alert = false;
                }
                self.tray_dirty = true;
            }
            EngineEvent::Status(s) => {
                log::info!("status: {}", s.text);
                // Failed downloads send no final progress; drop the bars (running ones come back).
                if matches!(s.level, StatusLevel::Error | StatusLevel::Warning) {
                    self.models.downloading.clear();
                }
                self.set_status(s);
            }
            EngineEvent::Progress(p) => {
                self.models.downloading.retain(|d| d.0 != p.model);
                let text = match p.phase {
                    ProgressPhase::Downloading if p.total.is_none_or(|t| p.done < t) => {
                        self.models.downloading.push((p.model, p.done, p.total));
                        let mut parts: Vec<String> = self
                            .models
                            .downloading
                            .iter()
                            .map(|&(m, done, total)| {
                                let pct = total
                                    .map(|t| format!(" {}%", done * 100 / t.max(1)))
                                    .unwrap_or_default();
                                format!("{}{pct}", purpose(m))
                            })
                            .collect();
                        // Each progress moves its model to the end; sorted, they keep their places.
                        parts.sort();
                        Some(format!("Downloading: {}", parts.join(", ")))
                    }
                    ProgressPhase::Downloading => {
                        self.refresh_models();
                        None
                    }
                    ProgressPhase::Loading => {
                        self.refresh_models();
                        Some(format!("Loading the {}…", purpose(p.model)))
                    }
                };
                if let Some(text) = text {
                    self.set_status(Status {
                        level: StatusLevel::Info,
                        text,
                        ttl: Some(Duration::from_secs(5)),
                    });
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
                if self.settings.browser_source
                    && let Some(w) = &self.web
                {
                    // A caption takes the sample's place. The sample is the page's live line,
                    // which a final caption leaves as it is, so it is cleared first.
                    if self.web_sample.take().is_some() && !matches!(c, CaptionUpdate::Live { .. })
                    {
                        let empty = CaptionUpdate::Live {
                            original: String::new(),
                            translated: None,
                            lang: None,
                        };
                        w.caption(&empty, "");
                    }
                    w.caption(&c, &self.settings.target_language);
                }
                self.overlay.apply(c, &self.settings, now)
            }
            EngineEvent::Speech(a) => match a {
                tts::Activity::Reading(voice) => self.models.reading = Some(voice),
                tts::Activity::Idle => {
                    self.models.reading = None;
                    self.models.previewing = None;
                }
                tts::Activity::VoiceLearned => self.models.voice_learned = true,
                tts::Activity::VoiceForgotten => self.models.voice_learned = false,
            },
            // A run that ends after Stop must not show a time while captions are off.
            EngineEvent::Stats { asr_ms, device } => {
                self.models.compute = device;
                if self.running {
                    self.models.asr_ms = Some(smoothed(self.models.asr_ms, asr_ms));
                }
            }
            EngineEvent::Translated { ms } if self.running => {
                self.models.tr_ms = Some(smoothed(self.models.tr_ms, ms));
            }
            EngineEvent::Translated { .. } => {}
        }
    }

    fn on_update(&mut self, mut status: update::Status) {
        if let update::Status::Ready(r, _) = &status
            && self.settings.update_tried.as_ref() == Some(&r.version)
        {
            log::warn!(
                "the install of v{} did not take, offering it instead",
                r.version
            );
            status = update::Status::Available(r.clone());
        }
        if let update::Status::Available(release) = &status {
            log::info!("update available: v{} {}", release.version, release.url);
            if let Some(t) = &mut self.tray
                && let Err(e) = t.show_update(&release.version)
            {
                log::warn!("tray update item failed: {e:#}");
            }
            if self.settings.update_notified.as_ref() != Some(&release.version) {
                self.info(format!(
                    "Hammurabi v{} is available. Open the tray menu to get it.",
                    release.version
                ));
                self.settings.update_notified = Some(release.version.clone());
                if let Err(e) = config::save(&self.settings) {
                    log::error!("config save failed: {e:#}");
                }
            }
            self.tray_dirty = true;
        }
        self.models.update = status;
    }

    fn info(&mut self, text: String) {
        self.set_status(Status {
            level: StatusLevel::Info,
            text,
            ttl: Some(Duration::from_secs(10)),
        });
    }

    fn check_updates(&mut self, ctx: &Context) {
        if matches!(
            self.models.update,
            update::Status::Checking | update::Status::Downloading(..)
        ) {
            return;
        }
        self.next_update_check = Some(Instant::now() + update::INTERVAL);
        self.models.update = update::Status::Checking;
        let (tx, c) = (self.app_tx.clone(), ctx.clone());
        update::spawn(move |s| {
            let _ = tx.send(AppEvent::Update(s));
            c.request_repaint();
        });
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
    instance::listen(move |signal| {
        let id = match signal {
            instance::Signal::Show => "settings",
            instance::Signal::Quit => "quit",
        };
        let _ = t.send(AppEvent::Menu(MenuId::new(id)));
        c.request_repaint();
    });
    let (c, t) = (ctx.clone(), tx.clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = t.send(AppEvent::Menu(e.id));
        c.request_repaint();
    }));
    // Unhandled tray clicks would pile up in tray-icon's channel.
    #[cfg(not(windows))]
    tray_icon::TrayIconEvent::set_event_handler(Some(|_| {}));
    #[cfg(windows)]
    {
        use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
        let (c, t) = (ctx.clone(), tx.clone());
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = t.send(AppEvent::Menu(MenuId::new("settings")));
                c.request_repaint();
            }
        }));
    }
    let c = ctx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.state() == HotKeyState::Pressed {
            let _ = tx.send(AppEvent::HotKey(e.id()));
            c.request_repaint();
        }
    }));
}

/// What a model is for, as status lines name it.
fn purpose(id: ModelId) -> &'static str {
    match id {
        ModelId::Madlad3bQ4k | ModelId::Gemma12b | ModelId::GemmaE4b => "translation model",
        ModelId::WhisperTiny
        | ModelId::WhisperBase
        | ModelId::WhisperSmall
        | ModelId::WhisperTurbo
        | ModelId::WhisperLarge => "speech model",
        _ => "voices",
    }
}

/// A time in milliseconds joined to those before it, so that one slow run does not show as slowness.
fn smoothed(was: Option<f32>, ms: u32) -> f32 {
    was.map_or(ms as f32, |was| 0.8 * was + 0.2 * ms as f32)
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.app_rx.try_recv() {
            let action = match ev {
                AppEvent::Menu(id) => tray::Tray::action(&id),
                AppEvent::HotKey(id) => self.hotkeys.action(id),
                AppEvent::Update(s) => {
                    self.on_update(s);
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
        if self.engine().finished() {
            self.engine_died(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }

        let now = Instant::now();
        if let Some(t) = self.status_until {
            if now >= t {
                self.status_until = None;
                self.status.clear();
                self.alert = false;
                self.tray_dirty = true;
            } else {
                ctx.request_repaint_after(t - now);
            }
        }
        if self.tray_dirty {
            self.tray_dirty = false;
            let tip = self.tooltip();
            if let Some(t) = &mut self.tray {
                t.sync(&self.settings, self.running, self.models.autostart, &tip);
            }
        }

        let visible = self.running || self.settings_open || self.overlay.has_status();
        if visible != self.shown {
            log::debug!("root visible = {visible}");
            self.shown = visible;
            if !visible {
                self.pointer.set_rects(Vec::new());
            }
            ctx.send_viewport_cmd(ViewportCommand::Visible(visible));
            self.root_ready = false;
            if visible {
                ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint();
        }

        // A downloaded update is installed while nothing is in use: captions off, no model
        // downloading. The setup program shows its progress and starts the new version.
        let idle = !self.running && self.models.downloading.is_empty();
        if let update::Status::Ready(release, path) = self.models.update.clone()
            && idle
            && (self.settings.check_updates || self.update_asked)
        {
            let version = &release.version;
            match self.install_at {
                None => {
                    let text =
                        format!("Updating Hammurabi to v{version}. It restarts in a moment.");
                    self.info(text);
                    self.install_at = Some(now + Duration::from_secs(3));
                    ctx.request_repaint_after(Duration::from_secs(3));
                }
                Some(t) if now >= t => {
                    self.settings.update_tried = Some(version.clone());
                    match update::install(&path, self.settings_open) {
                        Ok(()) => {
                            log::info!("installing the update with {}", path.display());
                            self.quit();
                        }
                        Err(e) => {
                            log::warn!("could not start the update: {e:#}");
                            self.on_update(update::Status::Available(release));
                        }
                    }
                }
                Some(t) => ctx.request_repaint_after(t - now),
            }
        } else {
            self.install_at = None;
        }
        if self.settings.check_updates && !platform::packaged() {
            let due = *self.next_update_check.get_or_insert(now);
            if now >= due {
                self.check_updates(ctx);
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
                self.sync_web();
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

        // Crossing monitors mid-drag changes the size to lay out for, but not the saved position.
        if !self.dragging
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
        let passthrough = self.settings.overlay_locked || !(self.dragging || self.pointer.over());
        if self.passthrough != passthrough {
            self.passthrough = passthrough;
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(passthrough));
        }

        // Styling captions shows them where they will appear.
        let sample = (self.settings_open && self.settings_win.page == Page::Captions)
            .then(|| tts::sample(&self.settings.target_language, "Hammurabi"));
        let pressed = self
            .overlay
            .paint(ui, &self.settings.caption, sample.as_deref());
        // The captions page shows it too, in the live line as the overlay does, so that its size
        // and place can be set in the streaming app. Leaving the page clears the live line only
        // while the sample is in it: a caption that came since took its place (`on_engine`).
        if let Some(w) = &self.web {
            match sample.as_deref().filter(|_| self.settings.browser_source) {
                Some(text) => {
                    if self.overlay.quiet()
                        && self
                            .web_sample
                            .is_none_or(|t| t.elapsed() >= WEB_SAMPLE_EVERY)
                    {
                        let c = CaptionUpdate::Live {
                            original: overlay::SAMPLE_ORIGINAL.into(),
                            translated: Some(text.into()),
                            lang: None,
                        };
                        w.caption(&c, &self.settings.target_language);
                        self.web_sample = Some(Instant::now());
                    }
                    ctx.request_repaint_after(WEB_SAMPLE_EVERY);
                }
                None if self.web_sample.take().is_some() => {
                    let c = CaptionUpdate::Live {
                        original: String::new(),
                        translated: None,
                        lang: None,
                    };
                    w.caption(&c, "");
                }
                None => {}
            }
        }
        if pressed {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            self.dragging = true;
        }
        // Windows runs a drag in its own loop, so no release event reaches this window.
        if self.dragging {
            if !pressed && cursor::read().is_none_or(|p| !p.down) {
                self.dragging = false;
                if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
                    self.settings.overlay_pos = Some([rect.min.x, rect.min.y]);
                    self.layout = None;
                    self.dirty_since = Some(Instant::now());
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        if let Some(inner) = ctx.input(|i| i.viewport().inner_rect) {
            let scale = if cfg!(target_os = "macos") {
                1.0
            } else {
                ctx.pixels_per_point()
            };
            let rects = self.overlay.bubbles().iter();
            self.pointer.set_rects(
                rects
                    .map(|r| r.translate(inner.min.to_vec2()) * scale)
                    .collect(),
            );
        }

        // On Windows, eframe paints a hidden root, and the frame right after showing it, without the
        // event loop it needs to create a window, and egui panics if asked to open one then. Frames
        // after those come from a window event, which has the event loop.
        let root_ready = self.root_ready;
        self.root_ready = ctx.input(|i| i.viewport().visible()).unwrap_or(true);
        if self.settings_open && !root_ready {
            ctx.request_repaint();
        }
        if self.settings_open && root_ready {
            let mut close = false;
            self.models.running = self.running;
            self.models.loading = self.state == EngineState::Loading;
            self.models.status.clone_from(&self.status);
            self.models.alert = self.alert;
            let page = self.settings_win.page;
            let actions = ctx.show_viewport_immediate(
                ViewportId::from_hash_of(SETTINGS_ID),
                // winit shows a window before sizing it, and it is blank white until painted.
                ViewportBuilder::default()
                    .with_visible(false)
                    .with_title("Hammurabi")
                    .with_icon(icons::window_icon())
                    .with_inner_size([740.0, 600.0])
                    .with_min_inner_size([640.0, 420.0]),
                |ui, _| {
                    close = ui.ctx().input(|i| i.viewport().close_requested());
                    self.settings_win
                        .show(ui.ctx(), &mut self.settings, &self.models)
                },
            );
            if !std::mem::replace(&mut self.settings_shown, true) {
                let id = ViewportId::from_hash_of(SETTINGS_ID);
                ctx.send_viewport_cmd_to(id, ViewportCommand::Visible(true));
                ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
            }
            // The overlay was painted before this window, so it shows any change next frame.
            if page != self.settings_win.page || !actions.is_empty() {
                ctx.request_repaint();
            }
            for a in actions {
                match a {
                    SettingsAction::Changed => {
                        self.layout = None;
                        if let Some(e) = self.hotkeys.apply(&self.settings) {
                            self.warn(e);
                        }
                        self.changed();
                    }
                    SettingsAction::Download(id) => {
                        // A bar at once: the first bytes can take seconds. A failed download's
                        // error status clears it.
                        if !self.models.downloading.iter().any(|d| d.0 == id) {
                            self.models.downloading.push((id, 0, None));
                        }
                        self.engine().send(EngineCommand::Prefetch(id));
                    }
                    SettingsAction::DeleteModel(id) => {
                        // After Stop the translator's server can still hold the file open.
                        if let Err(e) = models::remove(id) {
                            log::warn!("could not delete {id:?}: {e:#}");
                            self.warn(
                                "Hammurabi still has that model open. Quit it from the tray icon, \
                                 open it again and delete the model before starting captions."
                                    .into(),
                            );
                        }
                        self.refresh_models();
                    }
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
                        let text = tts::sample(&lang, tts::persona(id).name());
                        self.models.previewing = Some(id);
                        self.engine()
                            .send(EngineCommand::Preview { text, lang, style });
                    }
                    SettingsAction::StopPreview => self.engine().send(EngineCommand::StopSpeech),
                    SettingsAction::ToggleRunning => self.handle(UiAction::ToggleRunning, &ctx),
                    SettingsAction::ResetPosition => {
                        self.settings.overlay_pos = None;
                        self.layout = None;
                        self.changed();
                    }
                    #[cfg(omnivoice)]
                    SettingsAction::RelearnVoice => self.handle(UiAction::RelearnVoice, &ctx),
                    SettingsAction::CheckUpdates => {
                        self.update_asked = true;
                        self.check_updates(&ctx);
                    }
                    SettingsAction::OpenUpdate => self.handle(UiAction::OpenUpdate, &ctx),
                }
            }
            if close {
                self.settings_open = false;
                self.settings_shown = false;
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}
