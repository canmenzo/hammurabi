use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::UiAction;
use crate::i18n::{T, language_name, t, tf};
use crate::settings::{Settings, TARGET_LANGS};
use crate::tts::{self, PERSONAS};
use crate::types::AudioSource;

const SOURCES: [(AudioSource, &str, T); 3] = [
    (AudioSource::System, "src:system", T::SystemAudio),
    (AudioSource::Microphone, "src:mic", T::Microphone),
    (AudioSource::Both, "src:both", T::Both),
];

pub struct Tray {
    icon: TrayIcon,
    menu: Menu,
    start: MenuItem,
    source_menu: Submenu,
    sources: Vec<CheckMenuItem>,
    translate: CheckMenuItem,
    target_menu: Submenu,
    targets: Vec<CheckMenuItem>,
    speak: CheckMenuItem,
    voice_menu: Submenu,
    personas: Vec<CheckMenuItem>,
    #[cfg(omnivoice)]
    speaker_voice: (CheckMenuItem, MenuItem),
    autostart: CheckMenuItem,
    settings: MenuItem,
    about: MenuItem,
    logs: MenuItem,
    quit: MenuItem,
    update: Option<(MenuItem, String)>,
    active: Option<bool>,
}

fn about_text() -> String {
    tf(T::AboutX, &[&env!("CARGO_PKG_VERSION")])
}

fn icon(active: bool) -> Option<Icon> {
    let (rgba, w, h) = super::icons::icon_rgba(active, 32);
    Icon::from_rgba(rgba, w, h).ok()
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let start = MenuItem::with_id("start", t(T::StartCaptions), true, None);
        let sources: Vec<_> = SOURCES
            .iter()
            .map(|(_, id, label)| CheckMenuItem::with_id(*id, t(*label), true, false, None))
            .collect();
        let translate = CheckMenuItem::with_id("translate", t(T::Translate), true, false, None);
        let targets: Vec<_> = TARGET_LANGS
            .iter()
            .map(|c| {
                CheckMenuItem::with_id(format!("lang:{c}"), language_name(c), true, false, None)
            })
            .collect();
        let speak = CheckMenuItem::with_id("speak", t(T::PageReadAloud), true, false, None);
        let personas: Vec<_> = PERSONAS
            .iter()
            .map(|p| CheckMenuItem::with_id(format!("voice:{}", p.id), p.name(), true, false, None))
            .collect();
        let autostart = CheckMenuItem::with_id("autostart", t(T::StartAtLogin), true, false, None);
        let settings = MenuItem::with_id("settings", t(T::SettingsMenu), true, None);
        let about = MenuItem::with_id("about", about_text(), true, None);
        let logs = MenuItem::with_id("logs", t(T::OpenLogs), true, None);
        let quit = MenuItem::with_id("quit", t(T::Quit), true, None);

        // Their texts are set by `sync`.
        let source_menu = Submenu::new("", true);
        for s in &sources {
            source_menu.append(s)?;
        }
        let target_menu = Submenu::new("", true);
        for item in &targets {
            target_menu.append(item)?;
        }
        let voice_menu = Submenu::new("", true);
        for p in &personas {
            voice_menu.append(p)?;
        }
        #[cfg(omnivoice)]
        let speaker_voice = (
            CheckMenuItem::with_id("speaker-voice", t(T::SpeakersVoice), true, false, None),
            MenuItem::with_id("relearn", t(T::LearnVoiceAgain), true, None),
        );
        #[cfg(omnivoice)]
        voice_menu.append_items(&[
            &PredefinedMenuItem::separator(),
            &speaker_voice.0,
            &speaker_voice.1,
        ])?;
        let menu = Menu::new();
        menu.append_items(&[
            &start,
            &source_menu,
            &translate,
            &target_menu,
            &speak,
            &voice_menu,
            &PredefinedMenuItem::separator(),
            &settings,
            &autostart,
            &PredefinedMenuItem::separator(),
            &about,
            &logs,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let mut builder = TrayIconBuilder::new()
            .with_id("hammurabi")
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("Hammurabi");
        if let Some(i) = icon(false) {
            builder = builder.with_icon(i);
        }
        // Left click opens Settings there; the menu stays on the right button.
        #[cfg(windows)]
        {
            builder = builder.with_menu_on_left_click(false);
        }
        Ok(Self {
            icon: builder.build()?,
            menu,
            start,
            source_menu,
            sources,
            translate,
            target_menu,
            targets,
            speak,
            voice_menu,
            personas,
            #[cfg(omnivoice)]
            speaker_voice,
            autostart,
            settings,
            about,
            logs,
            quit,
            update: None,
            active: Some(false),
        })
    }

    pub fn action(id: &MenuId) -> Option<UiAction> {
        let id = id.0.as_str();
        Some(match id {
            "start" => UiAction::ToggleRunning,
            "translate" => UiAction::ToggleTranslate,
            "speak" => UiAction::ToggleSpeak,
            #[cfg(omnivoice)]
            "speaker-voice" => UiAction::ToggleSpeakerVoice,
            #[cfg(omnivoice)]
            "relearn" => UiAction::RelearnVoice,
            "settings" => UiAction::OpenSettings,
            "autostart" => UiAction::ToggleAutostart,
            "update" => UiAction::OpenUpdate,
            "about" => UiAction::About,
            "logs" => UiAction::OpenLogs,
            "quit" => UiAction::Quit,
            _ => {
                if let Some((src, _, _)) = SOURCES.iter().find(|(_, sid, _)| *sid == id) {
                    UiAction::SetSource(*src)
                } else if let Some(p) = id.strip_prefix("voice:") {
                    UiAction::SetPersona(PERSONAS.iter().find(|x| x.id == p)?.id)
                } else {
                    let code = id.strip_prefix("lang:")?;
                    UiAction::SetTarget(TARGET_LANGS.iter().find(|&&c| c == code)?)
                }
            }
        })
    }

    /// Adds "Update available" above About, once.
    pub fn show_update(&mut self, version: &str) -> anyhow::Result<()> {
        if self.update.is_none() {
            let item =
                MenuItem::with_id("update", tf(T::UpdateAvailableX, &[&version]), true, None);
            // The menu ends with About, Open logs, separator, Quit.
            let about = self.menu.items().len() - 4;
            self.menu.insert(&item, about)?;
            self.update = Some((item, version.into()));
        }
        Ok(())
    }

    /// Muda toggles check items on click, so every check is rewritten from state. Every text is
    /// set again too, for a change of the app's language.
    pub fn sync(&mut self, s: &Settings, running: bool, autostart: bool, tooltip: &str) {
        self.start.set_text(t(if running {
            T::StopCaptions
        } else {
            T::StartCaptions
        }));
        for (item, (src, _, label)) in self.sources.iter().zip(SOURCES.iter()) {
            item.set_text(t(*label));
            item.set_checked(s.source == *src);
            if s.source == *src {
                self.source_menu.set_text(tf(T::SourceX, &[&t(*label)]));
            }
        }
        self.translate.set_text(t(T::Translate));
        self.translate.set_checked(s.translate);
        self.target_menu
            .set_text(tf(T::TranslateToX, &[&language_name(&s.target_language)]));
        self.target_menu.set_enabled(s.translate);
        for (item, code) in self.targets.iter().zip(TARGET_LANGS.iter()) {
            item.set_text(language_name(code));
            item.set_checked(s.target_language == *code);
        }
        self.speak.set_text(t(T::PageReadAloud));
        self.speak.set_checked(s.speak);
        let persona = tts::persona(&s.speak_persona);
        for (item, p) in self.personas.iter().zip(PERSONAS.iter()) {
            item.set_text(p.name());
            item.set_checked(persona.id == p.id);
        }
        self.voice_menu.set_text(tf(T::VoiceX, &[&persona.name()]));
        self.voice_menu.set_enabled(s.speak);
        #[cfg(omnivoice)]
        {
            let on = s.speak_engine == tts::Engine::Speaker;
            self.speaker_voice.0.set_text(t(T::SpeakersVoice));
            self.speaker_voice.0.set_checked(on);
            self.speaker_voice.1.set_text(t(T::LearnVoiceAgain));
            self.speaker_voice.1.set_enabled(on && s.speak);
        }
        self.autostart.set_text(t(T::StartAtLogin));
        self.autostart.set_checked(autostart);
        self.settings.set_text(t(T::SettingsMenu));
        self.about.set_text(about_text());
        self.logs.set_text(t(T::OpenLogs));
        self.quit.set_text(t(T::Quit));
        if let Some((item, version)) = &self.update {
            item.set_text(tf(T::UpdateAvailableX, &[version]));
        }
        let _ = self.icon.set_tooltip(Some(tooltip));
        if self.active != Some(running) {
            self.active = Some(running);
            let _ = self.icon.set_icon(icon(running));
        }
        // ksni serves a menu snapshot; re-set it so the D-Bus layout refreshes.
        #[cfg(target_os = "linux")]
        self.icon.set_menu(Some(Box::new(self.menu.clone())));
    }
}
