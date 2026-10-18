//! Domain types shared by the server and the browser UI. Compiles to native and WASM.

pub mod annotation;
pub mod attribute;
pub mod constraint;
pub mod edges;
pub mod ellipse;
pub mod export;
pub mod fill;
pub mod filter;
pub mod geometry;
pub mod history;
pub mod import;
pub mod inference;
pub mod job;
pub mod keymap;
pub mod mask;
pub mod oriented;
pub mod pie;
pub mod pyramid;
pub mod settings;
pub mod user;
pub mod view;

use serde::{Deserialize, Serialize};

use crate::{attribute::AttributeDefinition, constraint::ShapeConstraints};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
    /// A project whose labels (with their ids, keys, shapes and attributes) the new one starts with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_labels_from: Option<i64>,
}

/// A project as the projects list shows it: how many images, how many annotated, and its labels'
/// colors in id order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub images: i64,
    pub annotated: i64,
    pub jobs: i64,
    pub label_colors: Vec<String>,
}

/// How many images a folder of the image root holds, subfolders included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCount {
    pub images: u64,
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
    Ellipse,
    Mask,
    Tag,
}

impl ShapeType {
    pub const ALL: [ShapeType; 8] = [
        ShapeType::Rectangle,
        ShapeType::OrientedRectangle,
        ShapeType::Polygon,
        ShapeType::Polyline,
        ShapeType::Points,
        ShapeType::Ellipse,
        ShapeType::Mask,
        ShapeType::Tag,
    ];

    /// The name people see, e.g. "Oriented rectangle".
    pub fn name(self) -> &'static str {
        match self {
            ShapeType::Rectangle => "Rectangle",
            ShapeType::OrientedRectangle => "Oriented rectangle",
            ShapeType::Polygon => "Polygon",
            ShapeType::Polyline => "Polyline",
            ShapeType::Points => "Points",
            ShapeType::Ellipse => "Ellipse",
            ShapeType::Mask => "Mask",
            ShapeType::Tag => "Tag",
        }
    }
}

/// A label as stored in a project. `id` is permanent within the project and never reused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub hotkey: Option<String>,
    pub shape_types: Vec<ShapeType>,
    pub ignore: bool,
    #[serde(default)]
    pub attributes: Vec<AttributeDefinition>,
    #[serde(flatten)]
    pub footprint: Footprint,
    /// Whether the editor shows the footprint of this label's polylines and points.
    #[serde(default = "default_true")]
    pub show_footprint: bool,
    #[serde(default)]
    pub constraints: ShapeConstraints,
}

/// The defaults of `cvat_to_masks.py`.
pub const DEFAULT_LINE_WIDTH: f64 = 7.0;
pub const DEFAULT_POINT_RADIUS: f64 = 8.0;

fn default_line_width() -> f64 {
    DEFAULT_LINE_WIDTH
}

fn default_point_radius() -> f64 {
    DEFAULT_POINT_RADIUS
}

pub(crate) fn default_true() -> bool {
    true
}

/// A label's export footprint: the area its polylines and points become in exports that need
/// areas or boxes. Polylines are stroked `line_width` wide; each point becomes a disc of
/// `point_radius`, or a square of twice that side. In original-image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    #[serde(default = "default_line_width")]
    pub line_width: f64,
    #[serde(default = "default_point_radius")]
    pub point_radius: f64,
    #[serde(default)]
    pub point_shape: PointShape,
}

impl Default for Footprint {
    fn default() -> Self {
        Footprint { line_width: DEFAULT_LINE_WIDTH, point_radius: DEFAULT_POINT_RADIUS, point_shape: PointShape::Disc }
    }
}

impl Footprint {
    /// Half the width of the band a polyline covers in exports, drawn like OpenCV draws thick
    /// lines: the width cut to whole pixels (at least 1), 2·⌈w/2⌉ + 1 pixels across.
    pub fn line_reach(&self) -> f64 {
        let thickness = self.line_width.trunc().max(1.0);
        if thickness == 1.0 { 0.5 } else { (thickness / 2.0).ceil() + 0.5 }
    }

    /// How far from a point's pixel centre the pixels of its area lie: the radius cut to whole
    /// pixels (0 still covers the one pixel).
    pub fn point_reach(&self) -> f64 {
        self.point_radius.trunc().max(0.0)
    }

    /// The footprint made sound: a line width and a point radius above zero.
    pub fn validated(self) -> Result<Self, String> {
        if !(self.line_width.is_finite() && self.line_width > 0.0 && self.point_radius.is_finite() && self.point_radius > 0.0) {
            return Err("line width and point radius must be above zero".into());
        }
        Ok(self)
    }
}

/// The area each point of a points shape becomes in exports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointShape {
    #[default]
    Disc,
    Square,
}

impl PointShape {
    /// The name it is stored and sent as.
    pub fn name(self) -> &'static str {
        match self {
            PointShape::Disc => "disc",
            PointShape::Square => "square",
        }
    }

    /// The point shape called `name`; anything else is a disc.
    pub fn named(name: &str) -> PointShape {
        if name == PointShape::Square.name() { PointShape::Square } else { PointShape::Disc }
    }
}

/// Everything about a label except its id: what clients send to create or edit one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelDefinition {
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default = "all_shape_types")]
    pub shape_types: Vec<ShapeType>,
    #[serde(default)]
    pub ignore: bool,
    #[serde(default)]
    pub attributes: Vec<AttributeDefinition>,
    #[serde(flatten)]
    pub footprint: Footprint,
    #[serde(default = "default_true")]
    pub show_footprint: bool,
    #[serde(default)]
    pub constraints: ShapeConstraints,
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
        self.attributes = attribute::validated(self.attributes)?;
        self.footprint = self.footprint.validated()?;
        self.constraints = self.constraints.validated()?;
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
        let LabelDefinition { name, color, hotkey, shape_types, ignore, attributes, footprint, show_footprint, constraints } = definition;
        Label { id, name, color, hotkey, shape_types, ignore, attributes, footprint, show_footprint, constraints }
    }

    /// Everything but the id.
    pub fn definition(&self) -> LabelDefinition {
        let Label { name, color, hotkey, shape_types, ignore, attributes, footprint, show_footprint, constraints, .. } = self.clone();
        LabelDefinition { name, color, hotkey, shape_types, ignore, attributes, footprint, show_footprint, constraints }
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
    /// Number of annotations on the image.
    pub annotations: u32,
    /// File size in bytes; unknown if the file was missing when it was recorded.
    pub size: Option<u64>,
    /// File modification time as `YYYY-MM-DD HH:MM`, in the server's local time.
    pub modified: Option<String>,
}

/// Which images of a project to list. Every set field must match; `None` means no restriction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageFilter {
    /// Regular expression matched case-insensitively anywhere in the image path.
    pub name: Option<String>,
    pub min_width: Option<u32>,
    pub max_width: Option<u32>,
    pub min_height: Option<u32>,
    pub max_height: Option<u32>,
    /// File size bounds in bytes, inclusive.
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    /// Modification date bounds as `YYYY-MM-DD`, inclusive.
    pub modified_from: Option<String>,
    pub modified_to: Option<String>,
    /// Only the images of this job.
    pub job: Option<i64>,
    /// Only images in no job.
    #[serde(default)]
    pub without_job: bool,
    /// Only images marked annotated (true) or not (false).
    pub annotated: Option<bool>,
}

impl ImageFilter {
    /// The set fields as URL query pairs.
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut pairs = Vec::new();
        let mut push = |key, value: Option<String>| {
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                pairs.push((key, value));
            }
        };
        push("name", self.name.clone());
        push("min_width", self.min_width.map(|v| v.to_string()));
        push("max_width", self.max_width.map(|v| v.to_string()));
        push("min_height", self.min_height.map(|v| v.to_string()));
        push("max_height", self.max_height.map(|v| v.to_string()));
        push("min_size", self.min_size.map(|v| v.to_string()));
        push("max_size", self.max_size.map(|v| v.to_string()));
        push("modified_from", self.modified_from.clone());
        push("modified_to", self.modified_to.clone());
        push("job", self.job.map(|v| v.to_string()));
        push("without_job", self.without_job.then(|| "true".to_owned()));
        push("annotated", self.annotated.map(|v| v.to_string()));
        pairs
    }

    pub fn is_empty(&self) -> bool {
        self.to_query().is_empty()
    }
}

/// An image's place in its project (1-based, by path) and the images around it, itself included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageNeighbours {
    pub position: i64,
    pub total: i64,
    pub images: Vec<Image>,
}

/// Sets whether an image counts as annotated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageStatus {
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

/// Images to take out of a project. The files on disk are not touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalRequest {
    pub ids: Vec<i64>,
}

/// How many images a removal took out; ids not in the project are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalReport {
    pub removed: u64,
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
