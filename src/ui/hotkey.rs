use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;

use super::UiAction;
use crate::settings::Settings;

/// Global hotkeys; must be created on the main (event loop) thread.
pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    toggle: Option<HotKey>,
    move_: Option<HotKey>,
    applied: Option<(String, String)>,
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
            move_: None,
            applied: None,
        };
        (hk, err)
    }

    /// (Re-)registers when the configured strings changed. Returns a status message on failure.
    pub fn apply(&mut self, s: &Settings) -> Option<String> {
        let wanted = (s.hotkey_toggle.clone(), s.hotkey_move.clone());
        if self.applied.as_ref() == Some(&wanted) {
            return None;
        }
        self.applied = Some(wanted);
        let manager = self.manager.as_ref()?;
        for hk in [self.toggle.take(), self.move_.take()]
            .into_iter()
            .flatten()
        {
            let _ = manager.unregister(hk);
        }
        let mut errors = Vec::new();
        let mut register = |text: &str| match text.parse::<HotKey>() {
            Ok(hk) => match manager.register(hk) {
                Ok(()) => {
                    log::debug!("hotkey {text} registered, id {}", hk.id());
                    Some(hk)
                }
                Err(e) => {
                    errors.push(format!("{text}: {e}"));
                    None
                }
            },
            Err(e) => {
                errors.push(format!("{text}: {e}"));
                None
            }
        };
        self.toggle = register(&s.hotkey_toggle);
        self.move_ = register(&s.hotkey_move);
        (!errors.is_empty()).then(|| format!("Hotkey not registered ({})", errors.join("; ")))
    }

    pub fn action(&self, id: u32) -> Option<UiAction> {
        if self.toggle.is_some_and(|h| h.id() == id) {
            Some(UiAction::ToggleRunning)
        } else if self.move_.is_some_and(|h| h.id() == id) {
            Some(UiAction::ToggleMove)
        } else {
            None
        }
    }
}
