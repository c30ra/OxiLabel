//! Annotations: a label applied to a shape on an image, always in original-image coordinates.

use serde::{Deserialize, Serialize};

use crate::{
    ShapeType,
    geometry::{Point, Rect},
};

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

    /// Whether `point` is on the shape, allowing `margin` image pixels around it.
    pub fn contains(&self, point: Point, margin: f64) -> bool {
        match self {
            Shape::Rectangle(rect) => rect.contains(point, margin),
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

/// Index of the topmost (last drawn) annotation under `point`, if any.
pub fn topmost_at(annotations: &[Annotation], point: Point, margin: f64) -> Option<usize> {
    annotations.iter().rposition(|annotation| annotation.shape.contains(point, margin))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(label: i64, min: (f64, f64), max: (f64, f64)) -> Annotation {
        Annotation::manual(label, Shape::Rectangle(Rect::new(min.0, min.1, max.0, max.1)))
    }

    #[test]
    fn the_topmost_shape_under_the_point_wins() {
        let annotations = [boxed(1, (0.0, 0.0), (100.0, 100.0)), boxed(2, (40.0, 40.0), (60.0, 60.0))];
        assert_eq!(topmost_at(&annotations, Point::new(50.0, 50.0), 0.0), Some(1));
        assert_eq!(topmost_at(&annotations, Point::new(10.0, 10.0), 0.0), Some(0));
        assert_eq!(topmost_at(&annotations, Point::new(150.0, 10.0), 0.0), None);
        assert_eq!(topmost_at(&annotations, Point::new(102.0, 10.0), 3.0), Some(0));
    }

    #[test]
    fn rectangles_inside_the_image_with_an_area_are_valid() {
        assert_eq!(boxed(1, (0.0, 0.0), (400.0, 300.0)).shape.validate(400, 300), Ok(()));
        assert!(boxed(1, (0.0, 0.0), (400.5, 300.0)).shape.validate(400, 300).is_err());
        assert!(boxed(1, (5.0, 5.0), (5.0, 9.0)).shape.validate(400, 300).is_err());
        assert!(boxed(1, (f64::NAN, 5.0), (6.0, 9.0)).shape.validate(400, 300).is_err());
    }

    #[test]
    fn manual_annotations_serialize_without_source_or_confidence() {
        let json = serde_json::to_value(boxed(3, (1.0, 2.0), (3.0, 4.0))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"label": 3, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 2.0}, "max": {"x": 3.0, "y": 4.0}}})
        );
    }
}
