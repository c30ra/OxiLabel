//! The signed-in user's settings in the UI: loaded with the session, saved to the server when
//! changed, and read by every key handler (through the keymap), the pie (flick distance) and the
//! page (theme).

use leptos::{prelude::*, task::spawn_local};
use rtag_core::{
    keymap::{KeyPress, Keymap},
    settings::{Theme, UserSettings},
};
use web_sys::KeyboardEvent;

use crate::api;

#[derive(Clone, Copy)]
pub struct Prefs {
    pub settings: RwSignal<UserSettings>,
    pub keymap: Memo<Keymap>,
}

impl Prefs {
    /// Provides the settings to every page and applies the theme whenever it changes.
    pub fn provide(settings: UserSettings) -> Prefs {
        let settings = RwSignal::new(settings);
        let prefs = Prefs { settings, keymap: Memo::new(move |_| settings.with(UserSettings::keymap)) };
        Effect::new(move |_| {
            let theme = match settings.with(|s| s.theme) {
                Theme::Dark => "dark",
                Theme::Light => "light",
                Theme::System => "system",
            };
            if let Some(root) = document().document_element() {
                root.set_attribute("data-theme", theme).ok();
            }
        });
        provide_context(prefs);
        prefs
    }

    /// Changes the settings here at once and stores them on the server; a refusal is returned
    /// (the change stays shown, so the user can fix it).
    pub fn change(self, change: impl FnOnce(&mut UserSettings), on_error: impl Fn(String) + 'static) {
        self.settings.update(change);
        let settings = self.settings.get_untracked();
        spawn_local(async move {
            if let Err(message) = api::put::<_, UserSettings>("/api/me/settings", &settings).await {
                on_error(message);
            }
        });
    }
}

/// The signed-in user's settings; defaults outside a signed-in page. Call it while building a
/// component (key handlers capture the keymap then), not inside an event handler.
pub fn prefs() -> Prefs {
    use_context::<Prefs>().unwrap_or_else(|| Prefs::provide(UserSettings::default()))
}

pub fn press(ev: &KeyboardEvent) -> KeyPress {
    KeyPress { key: ev.key(), ctrl: ev.ctrl_key() || ev.meta_key(), alt: ev.alt_key(), shift: ev.shift_key() }
}
