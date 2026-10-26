//! The session driven through its events, as the annotation layer drives it. Helpers shared by
//! the tests of each tool.

use serde_json::json;

use super::{Modifiers, Pointer, Session};
use crate::{
    Label,
    annotation::{Annotation, Shape},
    geometry::{Point, Rect},
};

mod select;

/// The image every test edits.
const IMAGE: (u32, u32) = (1000, 800);

/// A label allowing every shape, with `constraints` (a `ShapeConstraints` as JSON).
fn label_with(id: i64, constraints: serde_json::Value) -> Label {
    serde_json::from_value(json!({
        "id": id,
        "name": format!("label {id}"),
        "color": "#ff0000",
        "hotkey": null,
        "shape_types": ["rectangle", "oriented_rectangle", "polygon", "polyline", "points", "ellipse", "mask", "tag"],
        "ignore": false,
        "constraints": constraints,
    }))
    .expect("a valid label")
}

/// A session on `annotations` whose label 1 is free and label 2 fixes rectangles, oriented
/// rectangles and ellipses to 100 × 50.
fn session(annotations: Vec<Annotation>) -> Session {
    let fixed = json!({
        "rectangle": { "width": 100.0, "height": 50.0 },
        "oriented_rectangle": { "width": 100.0, "height": 50.0 },
        "ellipse": { "rx": 50.0, "ry": 25.0 },
    });
    Session::new(annotations, vec![label_with(1, json!({})), label_with(2, fixed)], IMAGE)
}

fn boxed(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Annotation {
    Annotation::manual(1, Shape::Rectangle(Rect::new(min_x, min_y, max_x, max_y)))
}

fn polygon(points: &[(f64, f64)]) -> Annotation {
    Annotation::manual(1, Shape::Polygon { points: points.iter().map(|&(x, y)| Point::new(x, y)).collect() })
}

/// The pointer at (`x`, `y`) in image pixels, at 100% and without modifiers.
fn at(x: f64, y: f64) -> Pointer {
    Pointer { at: Point::new(x, y), scale: 1.0, modifiers: Modifiers::default() }
}

fn shift(pointer: Pointer) -> Pointer {
    Pointer { modifiers: Modifiers { shift: true, ..pointer.modifiers }, ..pointer }
}

fn ctrl(pointer: Pointer) -> Pointer {
    Pointer { modifiers: Modifiers { ctrl: true, ..pointer.modifiers }, ..pointer }
}

fn click(session: &mut Session, pointer: Pointer) {
    session.press(pointer);
    session.release(pointer);
}

fn drag(session: &mut Session, from: Pointer, to: Pointer) {
    session.press(from);
    session.move_to(to);
    session.release(to);
}

fn selected(session: &Session) -> Vec<usize> {
    session.selection().iter().copied().collect()
}

fn shape(session: &Session, index: usize) -> Shape {
    session.annotations()[index].shape.clone()
}
