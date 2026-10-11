//! Annotations: a label applied to a shape on an image, always in original-image coordinates.

use serde::{Deserialize, Serialize};

use crate::{
    ShapeType,
    geometry::{Point, Rect, bounds_of, distance_to_segment, inside_polygon, polygon_area},
    oriented::OrientedRect,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    /// An axis-aligned box from `min` to `max`.
    Rectangle(Rect),
    /// A rotated box (drawn with three points).
    OrientedRectangle(OrientedRect),
    /// A closed outline; the last vertex connects back to the first.
    Polygon { points: Vec<Point> },
    /// An open line through the points in order.
    Polyline { points: Vec<Point> },
    /// Separate points, e.g. keypoints or small defects.
    Points { points: Vec<Point> },
}

/// Coordinates may stick out of the image by this much (float rounding) and still count as inside.
const EDGE_TOLERANCE: f64 = 1e-6;

impl Shape {
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Shape::Rectangle(_) => ShapeType::Rectangle,
            Shape::OrientedRectangle(_) => ShapeType::OrientedRectangle,
            Shape::Polygon { .. } => ShapeType::Polygon,
            Shape::Polyline { .. } => ShapeType::Polyline,
            Shape::Points { .. } => ShapeType::Points,
        }
    }

    /// The editable vertices: polygon, polyline and point positions, or rectangle corners.
    pub fn vertices(&self) -> Vec<Point> {
        match self {
            Shape::Rectangle(rect) => {
                vec![rect.min, Point::new(rect.max.x, rect.min.y), rect.max, Point::new(rect.min.x, rect.max.y)]
            }
            Shape::OrientedRectangle(rect) => rect.corners().to_vec(),
            Shape::Polygon { points } | Shape::Polyline { points } | Shape::Points { points } => points.clone(),
        }
    }

    /// The fewest vertices the shape can have.
    pub fn min_vertices(&self) -> usize {
        match self {
            Shape::Rectangle(_) | Shape::OrientedRectangle(_) => 4,
            Shape::Polygon { .. } => 3,
            Shape::Polyline { .. } => 2,
            Shape::Points { .. } => 1,
        }
    }

    /// The segments between vertices: closed for polygons, open for polylines, none otherwise.
    pub fn edges(&self) -> Vec<(Point, Point)> {
        match self {
            Shape::Polygon { points } => {
                (0..points.len()).map(|i| (points[i], points[(i + 1) % points.len()])).collect()
            }
            Shape::Polyline { points } => points.windows(2).map(|pair| (pair[0], pair[1])).collect(),
            Shape::Rectangle(_) | Shape::OrientedRectangle(_) => {
                let corners = self.vertices();
                (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).collect()
            }
            Shape::Points { .. } => Vec::new(),
        }
    }

    /// The smallest axis-aligned rectangle containing the shape.
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Rectangle(rect) => *rect,
            Shape::OrientedRectangle(rect) => rect.bounds(),
            Shape::Polygon { points } | Shape::Polyline { points } | Shape::Points { points } => bounds_of(points),
        }
    }

    /// The shape moved by (`dx`, `dy`) image pixels.
    pub fn translated(&self, dx: f64, dy: f64) -> Shape {
        let shift = |points: &Vec<Point>| points.iter().map(|p| p.offset(dx, dy)).collect();
        match self {
            Shape::Rectangle(rect) => {
                Shape::Rectangle(Rect::new(rect.min.x + dx, rect.min.y + dy, rect.max.x + dx, rect.max.y + dy))
            }
            Shape::OrientedRectangle(rect) => Shape::OrientedRectangle(rect.translated(dx, dy)),
            Shape::Polygon { points } => Shape::Polygon { points: shift(points) },
            Shape::Polyline { points } => Shape::Polyline { points: shift(points) },
            Shape::Points { points } => Shape::Points { points: shift(points) },
        }
    }

    /// Whether `point` is on the shape, allowing `margin` image pixels around it: inside areas,
    /// near lines and near points.
    pub fn contains(&self, point: Point, margin: f64) -> bool {
        let near_edge = || self.edges().iter().any(|(a, b)| distance_to_segment(point, *a, *b) <= margin);
        match self {
            Shape::Rectangle(rect) => rect.contains(point, margin),
            Shape::OrientedRectangle(rect) => rect.contains(point, margin),
            Shape::Polygon { points } => inside_polygon(point, points) || near_edge(),
            Shape::Polyline { .. } => near_edge(),
            Shape::Points { points } => points.iter().any(|p| p.distance(point) <= margin),
        }
    }

    /// The shape with vertex `index` (see `vertices`) moved to `to`. Rectangles keep the opposite
    /// corner; oriented rectangles also keep their angle.
    pub fn with_vertex(&self, index: usize, to: Point) -> Shape {
        let mut moved = self.clone();
        match &mut moved {
            Shape::Rectangle(rect) => *rect = Rect::from_corners(self.vertices()[(index + 2) % 4], to),
            Shape::OrientedRectangle(rect) => *rect = rect.with_corner(index, to),
            Shape::Polygon { points } | Shape::Polyline { points } | Shape::Points { points } => {
                if let Some(point) = points.get_mut(index) {
                    *point = to;
                }
            }
        }
        moved
    }

    /// The polygon or polyline with `point` inserted before vertex `index`; other shapes have no
    /// edges to split and return `None`.
    pub fn with_inserted_vertex(&self, index: usize, point: Point) -> Option<Shape> {
        let mut grown = self.clone();
        match &mut grown {
            Shape::Polygon { points } | Shape::Polyline { points } if index <= points.len() => points.insert(index, point),
            _ => return None,
        }
        Some(grown)
    }

    /// The shape without vertex `index`, or `None` if that would leave too few (or it is a
    /// rectangle).
    pub fn without_vertex(&self, index: usize) -> Option<Shape> {
        let mut shrunk = self.clone();
        match &mut shrunk {
            Shape::Polygon { points } | Shape::Polyline { points } | Shape::Points { points }
                if index < points.len() && points.len() > self.min_vertices() =>
            {
                points.remove(index);
            }
            _ => return None,
        }
        Some(shrunk)
    }

    /// Checks that the shape is well formed and lies inside a `width` × `height` image.
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        let vertices = self.vertices();
        if vertices.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err("coordinates must be numbers".into());
        }
        let (w, h) = (f64::from(width) + EDGE_TOLERANCE, f64::from(height) + EDGE_TOLERANCE);
        if vertices.iter().any(|p| p.x < -EDGE_TOLERANCE || p.y < -EDGE_TOLERANCE || p.x > w || p.y > h) {
            return Err(format!("the shape must lie inside the {width} × {height} image"));
        }
        match self {
            Shape::Rectangle(rect) if rect.width() <= 0.0 || rect.height() <= 0.0 => {
                Err("a rectangle needs a width and a height".into())
            }
            Shape::OrientedRectangle(rect) if !(rect.width > 0.0 && rect.height > 0.0 && rect.angle.is_finite()) => {
                Err("an oriented rectangle needs a width, a height and an angle".into())
            }
            Shape::Polygon { points } if points.len() < 3 || polygon_area(points) <= 0.0 => {
                Err("a polygon needs at least three points enclosing an area".into())
            }
            Shape::Polyline { points } if points.len() < 2 || points.windows(2).all(|pair| pair[0] == pair[1]) => {
                Err("a polyline needs at least two different points".into())
            }
            Shape::Points { points } if points.is_empty() => Err("a points shape needs at least one point".into()),
            _ => Ok(()),
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

/// Indices of the annotations whose bounds overlap `region` (box selection).
pub fn overlapping(annotations: &[Annotation], region: Rect) -> Vec<usize> {
    annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| annotation.shape.bounds().intersects(&region))
        .map(|(index, _)| index)
        .collect()
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
    fn box_selection_finds_overlapping_shapes() {
        let annotations = [boxed(1, (0.0, 0.0), (10.0, 10.0)), boxed(2, (20.0, 20.0), (30.0, 30.0)), boxed(3, (50.0, 0.0), (60.0, 10.0))];
        assert_eq!(overlapping(&annotations, Rect::new(5.0, 5.0, 25.0, 25.0)), vec![0, 1]);
        assert_eq!(overlapping(&annotations, Rect::new(35.0, 35.0, 45.0, 45.0)), Vec::<usize>::new());
    }

    #[test]
    fn shapes_translate() {
        let moved = boxed(1, (1.0, 2.0), (3.0, 4.0)).shape.translated(10.0, -1.0);
        assert_eq!(moved.bounds(), Rect::new(11.0, 1.0, 13.0, 3.0));
    }

    #[test]
    fn rectangles_inside_the_image_with_an_area_are_valid() {
        assert_eq!(boxed(1, (0.0, 0.0), (400.0, 300.0)).shape.validate(400, 300), Ok(()));
        assert!(boxed(1, (0.0, 0.0), (400.5, 300.0)).shape.validate(400, 300).is_err());
        assert!(boxed(1, (5.0, 5.0), (5.0, 9.0)).shape.validate(400, 300).is_err());
        assert!(boxed(1, (f64::NAN, 5.0), (6.0, 9.0)).shape.validate(400, 300).is_err());
    }

    fn points(list: &[(f64, f64)]) -> Vec<Point> {
        list.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn every_shape_type_serializes_with_its_type_tag() {
        let shapes = [
            Shape::Polygon { points: points(&[(0.0, 0.0), (4.0, 0.0), (0.0, 3.0)]) },
            Shape::Polyline { points: points(&[(0.0, 0.0), (4.0, 0.0)]) },
            Shape::Points { points: points(&[(1.0, 1.0)]) },
            Shape::OrientedRectangle(OrientedRect { centre: Point::new(5.0, 5.0), width: 4.0, height: 2.0, angle: 0.5 }),
        ];
        let tags: Vec<String> = shapes.iter().map(|s| serde_json::to_value(s).unwrap()["type"].as_str().unwrap().to_owned()).collect();
        assert_eq!(tags, ["polygon", "polyline", "points", "oriented_rectangle"]);
        for shape in shapes {
            let back: Shape = serde_json::from_value(serde_json::to_value(&shape).unwrap()).unwrap();
            assert_eq!(back, shape);
        }
    }

    #[test]
    fn hit_testing_by_shape_type() {
        let polygon = Shape::Polygon { points: points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]) };
        assert!(polygon.contains(Point::new(5.0, 5.0), 0.0));
        assert!(polygon.contains(Point::new(11.0, 5.0), 1.5));
        let line = Shape::Polyline { points: points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]) };
        assert!(line.contains(Point::new(5.0, 1.0), 1.5));
        assert!(!line.contains(Point::new(5.0, 5.0), 1.5), "a polyline has no inside");
        let dots = Shape::Points { points: points(&[(3.0, 3.0), (8.0, 8.0)]) };
        assert!(dots.contains(Point::new(8.5, 8.0), 1.0));
        assert!(!dots.contains(Point::new(5.0, 5.0), 1.0));
    }

    #[test]
    fn vertices_can_be_moved_inserted_and_removed_down_to_the_minimum() {
        let triangle = Shape::Polygon { points: points(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]) };
        let moved = triangle.with_vertex(1, Point::new(12.0, 1.0));
        assert_eq!(moved.vertices()[1], Point::new(12.0, 1.0));
        let square = triangle.with_inserted_vertex(2, Point::new(10.0, 10.0)).unwrap();
        assert_eq!(square.vertices(), points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]));
        assert_eq!(square.without_vertex(2).unwrap(), triangle);
        assert_eq!(triangle.without_vertex(0), None, "a polygon keeps three vertices");
        assert_eq!(Shape::Points { points: points(&[(1.0, 1.0)]) }.without_vertex(0), None);
        let rect = Shape::Rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(rect.with_vertex(2, Point::new(20.0, 5.0)), Shape::Rectangle(Rect::new(0.0, 0.0, 20.0, 5.0)));
        assert_eq!(rect.with_inserted_vertex(1, Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn edges_are_closed_for_polygons_and_open_for_polylines() {
        let pts = points(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        assert_eq!(Shape::Polygon { points: pts.clone() }.edges().len(), 3);
        assert_eq!(Shape::Polyline { points: pts.clone() }.edges().len(), 2);
        assert!(Shape::Points { points: pts }.edges().is_empty());
    }

    #[test]
    fn new_shapes_are_validated() {
        let ok = |shape: Shape| shape.validate(100, 50);
        assert_eq!(ok(Shape::Polygon { points: points(&[(0.0, 0.0), (100.0, 0.0), (0.0, 50.0)]) }), Ok(()));
        assert!(ok(Shape::Polygon { points: points(&[(0.0, 0.0), (5.0, 5.0), (10.0, 10.0)]) }).is_err(), "no area");
        assert!(ok(Shape::Polygon { points: points(&[(0.0, 0.0), (5.0, 0.0)]) }).is_err());
        assert!(ok(Shape::Polyline { points: points(&[(1.0, 1.0), (1.0, 1.0)]) }).is_err());
        assert!(ok(Shape::Points { points: vec![] }).is_err());
        assert!(ok(Shape::Points { points: points(&[(101.0, 1.0)]) }).is_err(), "outside");
        let tilted = OrientedRect { centre: Point::new(50.0, 25.0), width: 20.0, height: 10.0, angle: 0.3 };
        assert_eq!(ok(Shape::OrientedRectangle(tilted)), Ok(()));
        assert!(ok(Shape::OrientedRectangle(OrientedRect { centre: Point::new(2.0, 2.0), ..tilted })).is_err(), "corners outside");
        assert!(ok(Shape::OrientedRectangle(OrientedRect { height: 0.0, ..tilted })).is_err());
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
