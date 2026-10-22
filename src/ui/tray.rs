use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::UiAction;
use crate::settings::{Settings, TARGET_LANGS, lang_name};
use crate::types::AudioSource;

const SOURCES: [(AudioSource, &str, &str); 3] = [
    (AudioSource::System, "src:system", "System audio"),
    (AudioSource::Microphone, "src:mic", "Microphone"),
    (AudioSource::Both, "src:both", "Both"),
];

pub struct Tray {
    icon: TrayIcon,
    menu: Menu,
    start: MenuItem,
    sources: Vec<CheckMenuItem>,
    translate: CheckMenuItem,
    targets: Vec<CheckMenuItem>,
    move_item: CheckMenuItem,
    autostart: CheckMenuItem,
    update: Option<MenuItem>,
    active: Option<bool>,
}

fn icon(active: bool) -> Option<Icon> {
    let (rgba, w, h) = super::icons::icon_rgba(active, 32);
    Icon::from_rgba(rgba, w, h).ok()
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let start = MenuItem::with_id("start", "Start captions", true, None);
        let sources: Vec<_> = SOURCES
            .iter()
            .map(|(_, id, label)| CheckMenuItem::with_id(*id, *label, true, false, None))
            .collect();
        let translate = CheckMenuItem::with_id("translate", "Translate", true, false, None);
        let targets: Vec<_> = TARGET_LANGS
            .iter()
            .map(|c| CheckMenuItem::with_id(format!("lang:{c}"), lang_name(c), true, false, None))
            .collect();
        let move_item = CheckMenuItem::with_id("move", "Move captions", true, false, None);
        let autostart = CheckMenuItem::with_id("autostart", "Start at login", true, false, None);

        let source_menu = Submenu::new("Source", true);
        for s in &sources {
            source_menu.append(s)?;
        }
        let target_menu = Submenu::new("Target language", true);
        for t in &targets {
            target_menu.append(t)?;
        }
        let menu = Menu::new();
        menu.append_items(&[
            &start,
            &source_menu,
            &translate,
            &target_menu,
            &move_item,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("settings", "Settings…", true, None),
            &autostart,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "about",
                format!("About Hammurabi v{}", env!("CARGO_PKG_VERSION")),
                true,
                None,
            ),
            &MenuItem::with_id("logs", "Open logs", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit", true, None),
        ])?;

        let mut builder = TrayIconBuilder::new()
            .with_id("hammurabi")
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("Hammurabi");
        if let Some(i) = icon(false) {
            builder = builder.with_icon(i);
        }
        Ok(Self {
            icon: builder.build()?,
            menu,
            start,
            sources,
            translate,
            targets,
            move_item,
            autostart,
            update: None,
            active: Some(false),
        })
    }

    pub fn action(id: &MenuId) -> Option<UiAction> {
        let id = id.0.as_str();
        Some(match id {
            "start" => UiAction::ToggleRunning,
            "translate" => UiAction::ToggleTranslate,
            "move" => UiAction::ToggleMove,
            "settings" => UiAction::OpenSettings,
            "autostart" => UiAction::ToggleAutostart,
            "update" => UiAction::OpenUpdate,
            "about" => UiAction::About,
            "logs" => UiAction::OpenLogs,
            "quit" => UiAction::Quit,
            _ => {
                if let Some((src, _, _)) = SOURCES.iter().find(|(_, sid, _)| *sid == id) {
                    UiAction::SetSource(*src)
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
            let item = MenuItem::with_id(
                "update",
                format!("Update available: v{version}"),
                true,
                None,
            );
            // The menu ends with About, Open logs, separator, Quit.
            let about = self.menu.items().len() - 4;
            self.menu.insert(&item, about)?;
            self.update = Some(item);
        }
        Ok(())
    }

    /// Muda toggles check items on click, so every check is rewritten from state.
    pub fn sync(
        &mut self,
        s: &Settings,
        running: bool,
        move_mode: bool,
        autostart: bool,
        tooltip: &str,
    ) {
        self.start.set_text(if running {
            "Stop captions"
        } else {
            "Start captions"
        });
        for (item, (src, _, _)) in self.sources.iter().zip(SOURCES.iter()) {
            item.set_checked(s.source == *src);
        }
        self.translate.set_checked(s.translate);
        for (item, code) in self.targets.iter().zip(TARGET_LANGS.iter()) {
            item.set_checked(s.target_language == *code);
        }
        self.move_item.set_checked(move_mode);
        self.autostart.set_checked(autostart);
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
