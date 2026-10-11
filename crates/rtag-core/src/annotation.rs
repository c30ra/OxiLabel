//! Annotations: a label applied to a shape on an image, always in original-image coordinates.

use serde::{Deserialize, Serialize};

use crate::{ShapeType, geometry::Rect};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    /// An axis-aligned box from `min` to `max`.
    Rectangle(Rect),
}

impl Shape {
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Shape::Rectangle(_) => ShapeType::Rectangle,
        }
    }

    /// Checks that the shape is well formed and lies inside a `width` × `height` image.
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        match self {
            Shape::Rectangle(rect) => {
                let corners = [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
                if corners.iter().any(|value| !value.is_finite()) {
                    return Err("coordinates must be numbers".into());
                }
                if rect.min.x < 0.0 || rect.min.y < 0.0 || rect.max.x > f64::from(width) || rect.max.y > f64::from(height) {
                    return Err(format!("the shape must lie inside the {width} × {height} image"));
                }
                if rect.width() <= 0.0 || rect.height() <= 0.0 {
                    return Err("a rectangle needs a width and a height".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// The label's permanent id within the project.
    pub label: i64,
    pub shape: Shape,
    /// The model that produced it; `None` for manual work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl Annotation {
    pub fn manual(label: i64, shape: Shape) -> Self {
        Annotation { label, shape, source: None, confidence: None }
    }
}

/// All annotations of one image, in drawing order (the last one is on top), and the version they
/// belong to. Every successful save increases the version by one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageAnnotations {
    pub version: i64,
    pub annotations: Vec<Annotation>,
}

/// Replaces an image's annotations. Rejected if the image is no longer at `base_version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveAnnotations {
    pub base_version: i64,
    pub annotations: Vec<Annotation>,
}

/// How many annotations use a label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelUsage {
    pub annotations: i64,
}
