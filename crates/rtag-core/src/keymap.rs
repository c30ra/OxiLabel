//! Keyboard commands and their bindings: every action a key triggers, its default keys, and a
//! user's own keys on top. A binding is a key (as `KeyboardEvent.key` names it, letters in lower
//! case) with Ctrl (or ⌘), Alt and Shift. Shift only counts for letters and named keys: symbols
//! such as `+` need Shift on some keyboards and not on others.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Binding {
    pub key: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ctrl: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub alt: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shift: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A key as pressed: the event's key and modifiers (Ctrl or ⌘ count as Ctrl).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The key as bindings name it: letters in lower case, the space bar as "Space".
pub fn normal_key(key: &str) -> String {
    match key {
        " " => "Space".to_owned(),
        _ if key.chars().count() == 1 => key.to_lowercase(),
        _ => key.to_owned(),
    }
}

/// Whether Shift is part of what a key means (letters and named keys) rather than of how it is
/// typed (symbols).
fn shift_counts(key: &str) -> bool {
    key.chars().count() > 1 || key.chars().all(char::is_alphabetic)
}

impl Binding {
    pub fn key(key: &str) -> Self {
        Binding { key: normal_key(key), ..Binding::default() }
    }

    pub fn ctrl(key: &str) -> Self {
        Binding { ctrl: true, ..Binding::key(key) }
    }

    pub fn shift(key: &str) -> Self {
        Binding { shift: true, ..Binding::key(key) }
    }

    pub fn alt(key: &str) -> Self {
        Binding { alt: true, ..Binding::key(key) }
    }

    /// The binding a key press would make, e.g. when rebinding.
    pub fn of(press: &KeyPress) -> Self {
        let key = normal_key(&press.key);
        let shift = press.shift && shift_counts(&key);
        Binding { key, ctrl: press.ctrl, alt: press.alt, shift }
    }

    pub fn matches(&self, press: &KeyPress) -> bool {
        let key = normal_key(&press.key);
        key == self.key && press.ctrl == self.ctrl && press.alt == self.alt && (!shift_counts(&key) || press.shift == self.shift)
    }

    /// Whether the key alone (whatever the modifiers) is this binding's, for keys held down and
    /// released: a release often comes without the modifiers it was pressed with.
    pub fn same_key(&self, key: &str) -> bool {
        normal_key(key) == self.key
    }

    /// How people read it, e.g. "Ctrl+Shift+Z", "Q", "Space".
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(if self.key.chars().count() == 1 { self.key.to_uppercase() } else { self.key.clone() });
        parts.join("+")
    }
}

macro_rules! actions {
    ($($action:ident => $id:literal, $group:literal, $name:literal, [$($default:expr),*];)*) => {
        /// Everything a key does.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($action,)* }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$action,)*];

            /// The stable name settings store it under.
            pub fn id(self) -> &'static str {
                match self { $(Action::$action => $id,)* }
            }

            pub fn from_id(id: &str) -> Option<Action> {
                match id { $($id => Some(Action::$action),)* _ => None }
            }

            /// Where the settings screen lists it.
            pub fn group(self) -> &'static str {
                match self { $(Action::$action => $group,)* }
            }

            pub fn name(self) -> &'static str {
                match self { $(Action::$action => $name,)* }
            }

            pub fn defaults(self) -> Vec<Binding> {
                match self { $(Action::$action => vec![$($default),*],)* }
            }
        }
    };
}

actions! {
    Select => "select", "Tools", "Select", [Binding::key("v")];
    Rectangle => "rectangle", "Tools", "Rectangle", [Binding::key("r")];
    Polygon => "polygon", "Tools", "Polygon", [Binding::key("p")];
    Polyline => "polyline", "Tools", "Polyline", [Binding::key("l")];
    Points => "points", "Tools", "Points", [Binding::key(".")];
    Ellipse => "ellipse", "Tools", "Ellipse", [Binding::key("c")];
    Brush => "brush", "Tools", "Brush", [Binding::key("b")];
    Eraser => "eraser", "Tools", "Brush as eraser", [Binding::key("e")];
    FuzzyFill => "fuzzy_fill", "Tools", "Brush as fuzzy fill", [Binding::key("u")];
    Tag => "tag", "Tools", "Tag the image", [Binding::key("t")];
    Pie => "pie", "Pie menus", "Pie menu (hold)", [Binding::key("q")];
    LabelPie => "label_pie", "Pie menus", "Label pie (hold)", [Binding::shift("q")];
    Undo => "undo", "Editing", "Undo", [Binding::ctrl("z")];
    Redo => "redo", "Editing", "Redo", [Binding::ctrl("y"), Binding { shift: true, ..Binding::ctrl("z") }];
    Delete => "delete", "Editing", "Delete the selection", [Binding::key("Delete"), Binding::key("x")];
    SelectAll => "select_all", "Editing", "Select all", [Binding::key("a")];
    SelectNone => "select_none", "Editing", "Select nothing", [Binding::alt("a")];
    Cancel => "cancel", "Editing", "Cancel, step back", [Binding::key("Escape")];
    Finish => "finish", "Editing", "Finish the shape", [Binding::key("Enter")];
    RemoveLastPoint => "remove_last_point", "Editing", "Remove the last point drawn", [Binding::key("Backspace")];
    Smaller => "smaller", "Editing", "Smaller (brush, spacing, threshold)", [Binding::key("[")];
    Larger => "larger", "Editing", "Larger (brush, spacing, threshold)", [Binding::key("]")];
    ResizeBrush => "resize_brush", "Editing", "Resize the brush with the pointer", [Binding::key("f")];
    Pan => "pan", "View", "Pan (hold and drag)", [Binding::key("Space")];
    Fit => "fit", "View", "Fit the image", [Binding::key("Home")];
    ZoomIn => "zoom_in", "View", "Zoom in", [Binding::key("+"), Binding::key("=")];
    ZoomOut => "zoom_out", "View", "Zoom out", [Binding::key("-")];
    Original => "original", "View", "Show the original (hold)", [Binding::key("o")];
    NextImage => "next_image", "Images", "Next image", [Binding::key("j")];
    PreviousImage => "previous_image", "Images", "Previous image", [Binding::key("k")];
    FirstImage => "first_image", "Images", "First image (press twice)", [Binding::key("g")];
    LastImage => "last_image", "Images", "Last image", [Binding::shift("g")];
    MarkAnnotated => "mark_annotated", "Images", "Mark annotated and open the next", [Binding::ctrl("Enter")];
    NextPage => "next_page", "Images", "Next page of the grid", [Binding::key("PageDown")];
    PreviousPage => "previous_page", "Images", "Previous page of the grid", [Binding::key("PageUp")];
}

/// Every action's bindings: the defaults, with a user's own in place of an action's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Binding>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::with(&BTreeMap::new())
    }
}

impl Keymap {
    /// The defaults with `own` (by action id; unknown ids are ignored) in their place.
    pub fn with(own: &BTreeMap<String, Vec<Binding>>) -> Self {
        let bindings = Action::ALL
            .iter()
            .map(|&action| (action, own.get(action.id()).cloned().unwrap_or_else(|| action.defaults())))
            .collect();
        Keymap { bindings }
    }

    pub fn bindings(&self, action: Action) -> &[Binding] {
        self.bindings.get(&action).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn matches(&self, action: Action, press: &KeyPress) -> bool {
        self.bindings(action).iter().any(|binding| binding.matches(press))
    }

    /// Whether `key` (whatever the modifiers) is one of the action's keys; for releases.
    pub fn is_key(&self, action: Action, key: &str) -> bool {
        self.bindings(action).iter().any(|binding| binding.same_key(key))
    }

    /// The first binding's label, for hints such as "Brush (B)"; empty if unbound.
    pub fn label(&self, action: Action) -> String {
        self.bindings(action).first().map(Binding::label).unwrap_or_default()
    }

    /// The bindings two or more actions share, with those actions.
    pub fn conflicts(&self) -> BTreeMap<Binding, Vec<Action>> {
        let mut users: BTreeMap<Binding, Vec<Action>> = BTreeMap::new();
        for (action, bindings) in &self.bindings {
            for binding in bindings {
                users.entry(binding.clone()).or_default().push(*action);
            }
        }
        users.retain(|_, actions| actions.len() > 1);
        users
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str, ctrl: bool, alt: bool, shift: bool) -> KeyPress {
        KeyPress { key: key.into(), ctrl, alt, shift }
    }

    #[test]
    fn keys_match_with_their_modifiers() {
        let keys = Keymap::default();
        assert!(keys.matches(Action::Rectangle, &press("r", false, false, false)));
        assert!(!keys.matches(Action::Rectangle, &press("r", true, false, false)), "Ctrl+R is not R");
        // Shift on a letter is part of the binding: Shift+Q is the label pie, not the pie.
        assert!(keys.matches(Action::LabelPie, &press("Q", false, false, true)));
        assert!(!keys.matches(Action::Pie, &press("Q", false, false, true)));
        assert!(keys.matches(Action::Redo, &press("Z", true, false, true)));
        assert!(keys.matches(Action::Undo, &press("z", true, false, false)));
        // Symbols match however they were typed.
        assert!(keys.matches(Action::ZoomIn, &press("+", false, false, true)));
        assert!(keys.matches(Action::Pan, &press(" ", false, false, false)));
        assert!(keys.is_key(Action::Pie, "Q"));
    }

    #[test]
    fn the_defaults_do_not_conflict() {
        assert_eq!(Keymap::default().conflicts(), BTreeMap::new());
        for &action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn own_bindings_replace_the_defaults_and_conflicts_are_found() {
        let own = BTreeMap::from([
            ("rectangle".to_owned(), vec![Binding::key("w")]),
            ("polygon".to_owned(), vec![Binding::key("w")]),
            ("no_such_action".to_owned(), vec![Binding::key("z")]),
        ]);
        let keys = Keymap::with(&own);
        assert!(keys.matches(Action::Rectangle, &press("w", false, false, false)));
        assert!(!keys.matches(Action::Rectangle, &press("r", false, false, false)));
        assert_eq!(keys.conflicts(), BTreeMap::from([(Binding::key("w"), vec![Action::Rectangle, Action::Polygon])]));
        assert_eq!(keys.label(Action::Polyline), "L");
    }

    #[test]
    fn a_press_makes_a_binding_people_can_read() {
        let binding = Binding::of(&press("Z", true, false, true));
        assert_eq!((binding.label(), binding), ("Ctrl+Shift+Z".to_owned(), Binding { shift: true, ..Binding::ctrl("z") }));
        assert_eq!(Binding::of(&press("+", false, false, true)), Binding::key("+"), "Shift is not part of a symbol");
        assert_eq!(Binding::of(&press(" ", false, false, false)).label(), "Space");
        assert_eq!(Binding::key("PageDown").label(), "PageDown");
    }
}
