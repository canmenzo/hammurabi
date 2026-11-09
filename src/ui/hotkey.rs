use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;

use super::UiAction;
use crate::settings::Settings;

/// Global hotkeys; must be created on the main (event loop) thread.
pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    toggle: Option<HotKey>,
    applied: Option<String>,
}

impl Hotkeys {
    pub fn new() -> (Self, Option<String>) {
        let (manager, err) = match GlobalHotKeyManager::new() {
            Ok(m) => (Some(m), None),
            Err(e) => (None, Some(format!("Global hotkeys unavailable: {e}"))),
        };
        let hk = Self {
            manager,
            toggle: None,
            applied: None,
        };
        (hk, err)
    }

    /// (Re-)registers when the configured strings changed. Returns a status message on failure.
    pub fn apply(&mut self, s: &Settings) -> Option<String> {
        if self.applied.as_ref() == Some(&s.hotkey_toggle) {
            return None;
        }
        self.applied = Some(s.hotkey_toggle.clone());
        let manager = self.manager.as_ref()?;
        if let Some(hk) = self.toggle.take() {
            let _ = manager.unregister(hk);
        }
        let text = &s.hotkey_toggle;
        let registered = text
            .parse::<HotKey>()
            .map_err(|e| e.to_string())
            .and_then(|hk| {
                manager.register(hk).map_err(|e| e.to_string())?;
                log::debug!("hotkey {text} registered, id {}", hk.id());
                Ok(hk)
            });
        match registered {
            Ok(hk) => {
                self.toggle = Some(hk);
                None
            }
            Err(e) => Some(format!("Hotkey not registered ({text}: {e})")),
        }
    }

    pub fn action(&self, id: u32) -> Option<UiAction> {
        self.toggle
            .is_some_and(|h| h.id() == id)
            .then_some(UiAction::ToggleRunning)
    }
}
