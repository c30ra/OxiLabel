//! Domain types shared by the server and the browser UI. Compiles to native and WASM.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
}

/// The kinds of annotation a label can be used with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    Rectangle,
    OrientedRectangle,
    Polygon,
    Polyline,
    Points,
    Mask,
    Tag,
}

impl ShapeType {
    pub const ALL: [ShapeType; 7] = [
        ShapeType::Rectangle,
        ShapeType::OrientedRectangle,
        ShapeType::Polygon,
        ShapeType::Polyline,
        ShapeType::Points,
        ShapeType::Mask,
        ShapeType::Tag,
    ];
}

/// A label as stored in a project. `id` is permanent within the project and never reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub hotkey: Option<String>,
    pub shape_types: Vec<ShapeType>,
    pub ignore: bool,
}

/// Everything about a label except its id: what clients send to create or edit one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelDefinition {
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default = "all_shape_types")]
    pub shape_types: Vec<ShapeType>,
    #[serde(default)]
    pub ignore: bool,
}

impl LabelDefinition {
    /// Checks the definition against the project's `other` labels (excluding the one being
    /// edited) and returns it normalized: trimmed name, empty hotkey as none, no repeated shapes.
    pub fn validated(mut self, others: &[Label]) -> Result<Self, String> {
        self.name = self.name.trim().to_owned();
        self.hotkey = self.hotkey.map(|key| key.trim().to_owned()).filter(|key| !key.is_empty());
        let mut shape_types = Vec::with_capacity(self.shape_types.len());
        for shape in self.shape_types {
            if !shape_types.contains(&shape) {
                shape_types.push(shape);
            }
        }
        self.shape_types = shape_types;

        if self.name.is_empty() {
            return Err("label name must not be blank".into());
        }
        if others.iter().any(|other| other.name == self.name) {
            return Err(format!("a label named \"{}\" already exists", self.name));
        }
        if !is_hex_color(&self.color) {
            return Err("color must look like #rrggbb".into());
        }
        if let Some(key) = &self.hotkey {
            if key.chars().count() != 1 {
                return Err("hotkey must be a single key".into());
            }
            if let Some(other) = others.iter().find(|other| other.hotkey.as_ref() == Some(key)) {
                return Err(format!("hotkey \"{key}\" is already used by \"{}\"", other.name));
            }
        }
        if self.shape_types.is_empty() {
            return Err("a label must allow at least one shape type".into());
        }
        Ok(self)
    }
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn all_shape_types() -> Vec<ShapeType> {
    ShapeType::ALL.to_vec()
}

impl Label {
    pub fn new(id: i64, definition: LabelDefinition) -> Self {
        let LabelDefinition { name, color, hotkey, shape_types, ignore } = definition;
        Label { id, name, color, hotkey, shape_types, ignore }
    }
}
