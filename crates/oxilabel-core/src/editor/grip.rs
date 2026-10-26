//! The handles of a selected shape. One list says which handles a shape has and where: the
//! session hit-tests it and the annotation layer draws it.

use crate::{
    annotation::Shape,
    geometry::{Handle, Point},
};

use super::{HANDLE_REACH, ROTATION_HANDLE_DISTANCE};

/// What dragging a handle does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GripKind {
    /// One of a rectangle's eight resize handles.
    Resize(Handle),
    /// Moves vertex `n` (see `Shape::vertices`): a polygon, polyline or points vertex, an
    /// oriented rectangle's corner, an ellipse's centre.
    Vertex(usize),
    /// The "+" in the middle of edge `n` (see `Shape::edges`): dragging it inserts a vertex.
    Insert(usize),
    /// An ellipse's radius handle: 0 for rx, 1 for ry.
    Radius(usize),
    /// Turns an oriented rectangle or an ellipse; its stem starts at `from` on the outline.
    Turn { from: Point },
}

impl GripKind {
    /// Where several handles are in reach, the lowest rank wins (then the nearest).
    fn rank(&self) -> u8 {
        match self {
            GripKind::Turn { .. } => 0,
            GripKind::Radius(_) => 1,
            GripKind::Resize(_) | GripKind::Vertex(_) => 2,
            GripKind::Insert(_) => 3,
        }
    }
}

/// A handle and where it is, in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grip {
    pub kind: GripKind,
    pub at: Point,
}

/// The handles of `shape` seen at `scale` (rotation handles keep their distance on screen), in
/// drawing order. A shape of `fixed` size moves and turns, but has no handles that resize it.
pub fn grips(shape: &Shape, fixed: bool, scale: f64) -> Vec<Grip> {
    let grip = |kind, at| Grip { kind, at };
    let vertices = || shape.vertices().into_iter().enumerate().map(|(n, at)| grip(GripKind::Vertex(n), at));
    let stem = ROTATION_HANDLE_DISTANCE / scale;
    match shape {
        Shape::Rectangle(_) if fixed => Vec::new(),
        Shape::Rectangle(rect) => Handle::ALL.iter().map(|&handle| grip(GripKind::Resize(handle), rect.handle_position(handle))).collect(),
        Shape::OrientedRectangle(rect) => {
            let turn = grip(GripKind::Turn { from: rect.rotation_handle(0.0) }, rect.rotation_handle(stem));
            let corners = if fixed { Vec::new() } else { vertices().collect() };
            corners.into_iter().chain([turn]).collect()
        }
        Shape::Ellipse(ellipse) => {
            let radii = ellipse.radius_handles().into_iter().enumerate().filter(|_| !fixed).map(|(axis, at)| grip(GripKind::Radius(axis), at));
            let turn = grip(GripKind::Turn { from: ellipse.rotation_handle(0.0) }, ellipse.rotation_handle(stem));
            vertices().chain(radii).chain([turn]).collect()
        }
        Shape::Polygon { .. } | Shape::Polyline { .. } => {
            let inserts = shape.edges().into_iter().enumerate().map(|(n, (a, b))| grip(GripKind::Insert(n), a.midpoint(b)));
            inserts.chain(vertices()).collect()
        }
        Shape::Points { .. } => vertices().collect(),
        Shape::Mask(_) | Shape::Tag => Vec::new(),
    }
}

/// The handle under `point` among `grips`, seen at `scale`: within `HANDLE_REACH` screen pixels,
/// the best ranked and then the nearest.
pub fn grip_at(grips: &[Grip], point: Point, scale: f64) -> Option<Grip> {
    let reach = HANDLE_REACH / scale;
    grips
        .iter()
        .map(|grip| (grip, grip.at.distance(point)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|(a, da), (b, db)| a.kind.rank().cmp(&b.kind.rank()).then(da.total_cmp(db)))
        .map(|(grip, _)| *grip)
}
