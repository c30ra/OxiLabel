//! A user's own settings, stored on the server so they follow the user: shortcuts, how far a
//! pie flick reaches, how labels are switched while drawing, and the theme.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    keymap::{Binding, Keymap},
    pie::FLICK_DISTANCE,
};

/// How far a flick may be set, in screen pixels.
pub const FLICK_RANGE: (f64, f64) = (40.0, 400.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStyle {
    /// Number keys, and the label pie key (Shift+Q).
    #[default]
    Hotkeys,
    /// Number keys, and a Label slice in the tool's options pie.
    OptionsRing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    /// Whatever the system prefers.
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    /// The user's own bindings, by action id; actions not listed keep their defaults.
    pub shortcuts: BTreeMap<String, Vec<Binding>>,
    /// How far (screen pixels) the pointer goes from a pie's centre to pick by flicking.
    pub flick_distance: f64,
    pub label_style: LabelStyle,
    pub theme: Theme,
}

impl Default for UserSettings {
    fn default() -> Self {
        UserSettings { shortcuts: BTreeMap::new(), flick_distance: FLICK_DISTANCE, label_style: LabelStyle::default(), theme: Theme::default() }
    }
}

impl UserSettings {
    pub fn keymap(&self) -> Keymap {
        Keymap::with(&self.shortcuts)
    }

    /// The settings made sound: the flick distance within its range, no empty keys.
    pub fn validated(mut self) -> Result<Self, String> {
        if !self.flick_distance.is_finite() {
            return Err("the flick distance must be a number".into());
        }
        self.flick_distance = self.flick_distance.clamp(FLICK_RANGE.0, FLICK_RANGE.1);
        if self.shortcuts.values().flatten().any(|binding| binding.key.trim().is_empty()) {
            return Err("a shortcut needs a key".into());
        }
        Ok(self)
    }
}
