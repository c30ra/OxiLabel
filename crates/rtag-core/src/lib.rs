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

/// An image in a project. `path` is relative to the server's image root, with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub id: i64,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub annotated: bool,
}

/// One page of a project's images, ordered by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePage {
    pub total: i64,
    pub images: Vec<Image>,
}

/// What to import: folders (imported recursively) and single images, relative to the image root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub paths: Vec<String>,
}

/// The folders and images directly inside one folder of the image root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseListing {
    /// The listed folder relative to the image root; empty for the root itself.
    pub path: String,
    pub entries: Vec<BrowseEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    /// Whether the image is already in the project being imported into; only set for images
    /// when browsing on behalf of a project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Folder,
    Image,
}

/// What an import did: new images added, known ones (same path or same content) skipped,
/// files that are not images counted, and images that could not be read listed with a reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
    pub unsupported: usize,
    pub failed: Vec<ImportFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFailure {
    pub path: String,
    pub reason: String,
}
