//! The Select tool: a click picks the topmost shape (Shift toggles it, Ctrl removes it), a drag
//! on empty space draws a selection box (Shift adds, Ctrl removes), a drag on a shape moves the
//! selection, and the handles of a single selected shape edit it: rectangle handles resize,
//! vertices drag, the "+" on an edge inserts a vertex, Ctrl+click on a vertex removes it, and the
//! round handles turn oriented rectangles and ellipses and change an ellipse's radii.

use std::{borrow::Cow, collections::BTreeSet};

use crate::{
    annotation::{Annotation, Shape, overlapping, topmost_at},
    ellipse::Ellipse,
    geometry::{Point, Rect},
};

use super::{
    BoxMode, DRAG_THRESHOLD, Gesture, MIN_SHAPE_SIZE, PICK_MARGIN, Pointer, Session,
    grip::{Grip, GripKind, grip_at, grips},
    snap_turn,
};

impl Session {
    /// A press of the main button with the Select tool.
    pub fn press(&mut self, pointer: Pointer) {
        let Pointer { at: point, scale, modifiers } = pointer;
        let (extend, subtract) = (modifiers.shift, modifiers.ctrl);
        if !extend && let Some((index, grip)) = self.grip_under(point, scale) {
            self.grab(index, grip, point, subtract);
            return;
        }
        match topmost_at(&self.annotations, point, PICK_MARGIN / scale, self.threshold) {
            // Shift+click toggles a shape, Ctrl+click removes it from the selection.
            Some(index) if extend => self.toggle_selected(index),
            Some(index) if subtract => {
                self.selection.remove(&index);
            }
            // Dragging a selected shape moves the whole selection.
            Some(index) => {
                if !self.selection.contains(&index) {
                    self.selection = BTreeSet::from([index]);
                }
                self.gesture = Gesture::Moving { grab: point, dx: 0.0, dy: 0.0 };
            }
            // On empty space a drag draws a selection box.
            None => {
                let mode = if extend {
                    BoxMode::Extend
                } else if subtract {
                    BoxMode::Subtract
                } else {
                    self.selection.clear();
                    BoxMode::Replace
                };
                self.gesture = Gesture::Selecting { start: point, current: point, mode };
            }
        }
    }

    /// Starts dragging `grip` of annotation `index`; with Ctrl, a vertex is removed instead.
    fn grab(&mut self, index: usize, grip: Grip, point: Point, subtract: bool) {
        let Some(shape) = self.annotations.get(index).map(|a| a.shape.clone()) else { return };
        self.gesture = match (grip.kind, shape) {
            // Ctrl+click on a vertex removes it, if the shape keeps enough.
            (GripKind::Vertex(vertex), shape) if subtract => {
                match shape.without_vertex(vertex) {
                    Some(smaller) => self.reshape(index, smaller),
                    None => self.notice = Some("That shape cannot lose another point.".into()),
                }
                return;
            }
            _ if subtract => return,
            (GripKind::Resize(handle), Shape::Rectangle(rect)) => Gesture::Resizing { index, handle, before: rect, current: rect },
            (GripKind::Vertex(vertex), shape) => Gesture::Vertex { index, vertex, current: shape },
            // Dragging an edge's "+" inserts a vertex there and drags it.
            (GripKind::Insert(edge), shape) => match shape.with_inserted_vertex(edge + 1, point) {
                Some(grown) => Gesture::Vertex { index, vertex: edge + 1, current: grown },
                None => return,
            },
            (GripKind::Turn { .. }, Shape::OrientedRectangle(rect)) => Gesture::Rotating { index, before: rect, current: rect },
            (GripKind::Turn { .. }, Shape::Ellipse(ellipse)) => Gesture::Turning { index, before: ellipse, current: ellipse },
            (GripKind::Radius(axis), Shape::Ellipse(ellipse)) => Gesture::Radius { index, axis, current: ellipse },
            _ => return,
        };
    }

    /// The pointer moved (with or without a button down).
    pub fn move_to(&mut self, pointer: Pointer) {
        let Pointer { at: point, modifiers, .. } = pointer;
        let (width, height) = (f64::from(self.image.0), f64::from(self.image.1));
        let inside = point.clamped(width, height);
        let snap = modifiers.shift != self.options.snap;
        self.gesture = match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Idle => Gesture::Idle,
            Gesture::Selecting { start, mode, .. } => Gesture::Selecting { start, current: point, mode },
            Gesture::Moving { grab, dx, dy } => match self.movable_bounds() {
                Some(bounds) => {
                    let moved = bounds.moved_within(point.x - grab.x, point.y - grab.y, width, height);
                    Gesture::Moving { grab, dx: moved.min.x - bounds.min.x, dy: moved.min.y - bounds.min.y }
                }
                None => Gesture::Moving { grab, dx, dy },
            },
            Gesture::Resizing { index, handle, before, .. } => Gesture::Resizing { index, handle, before, current: before.resized(handle, inside) },
            Gesture::Vertex { index, vertex, current } => Gesture::Vertex { index, vertex, current: current.with_vertex(vertex, inside) },
            Gesture::Rotating { index, before, .. } => {
                let angle = snap_turn(before.angle_towards(point), snap);
                Gesture::Rotating { index, before, current: before.rotated_to(angle) }
            }
            Gesture::Turning { index, before, current } => {
                let turned = before.rotated_to(snap_turn(before.angle_towards(point), snap));
                Gesture::Turning { index, before, current: if self.fits(turned) { turned } else { current } }
            }
            // The radius follows the handle, up to the largest that keeps the ellipse in the image.
            Gesture::Radius { index, axis, current } => Gesture::Radius { index, axis, current: self.largest_fitting(current, current.with_radius(axis, point)) },
        };
    }

    /// The main button was released: the gesture's result is applied.
    pub fn release(&mut self, pointer: Pointer) {
        let scale = pointer.scale;
        match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Idle => {}
            Gesture::Selecting { start, current, mode } => {
                let region = Rect::from_corners(start, current);
                // A click without a drag only cleared the selection (or did nothing with Shift/Ctrl).
                if region.width() * scale < DRAG_THRESHOLD && region.height() * scale < DRAG_THRESHOLD {
                    return;
                }
                let hits = overlapping(&self.annotations, region, self.threshold);
                match mode {
                    BoxMode::Replace | BoxMode::Extend => self.selection.extend(hits),
                    BoxMode::Subtract => hits.iter().for_each(|hit| {
                        self.selection.remove(hit);
                    }),
                }
            }
            Gesture::Moving { dx, dy, .. } => {
                if dx == 0.0 && dy == 0.0 {
                    return;
                }
                if self.selection.iter().any(|&i| self.annotations.get(i).is_some_and(|a| a.locked)) {
                    self.notice = Some("Locked shapes stay where they are: unlock them in the objects list first.".into());
                }
                self.change_selected(|annotation| {
                    if annotation.locked {
                        return annotation.clone();
                    }
                    Annotation { shape: annotation.shape.translated(dx, dy), ..annotation.clone() }
                });
            }
            Gesture::Resizing { index, before, current, .. } => {
                let big_enough = current.width() * scale >= MIN_SHAPE_SIZE && current.height() * scale >= MIN_SHAPE_SIZE;
                if current != before && big_enough {
                    self.reshape(index, Shape::Rectangle(current));
                }
            }
            Gesture::Vertex { index, current, .. } => self.reshape_valid(index, current),
            Gesture::Rotating { index, current, .. } => self.reshape_valid(index, Shape::OrientedRectangle(current)),
            Gesture::Radius { index, current, .. } => {
                if current.rx.min(current.ry) * scale >= MIN_SHAPE_SIZE / 2.0 {
                    self.reshape(index, Shape::Ellipse(current));
                }
            }
            Gesture::Turning { index, current, .. } => self.reshape(index, Shape::Ellipse(current)),
        }
    }

    /// Reshapes annotation `index` if `shape` is valid in the image, or says why not.
    fn reshape_valid(&mut self, index: usize, shape: Shape) {
        match shape.validate(self.image.0, self.image.1) {
            Ok(()) => self.reshape(index, shape),
            Err(reason) => self.notice = Some(format!("Not changed: {reason}.")),
        }
    }

    fn fits(&self, ellipse: Ellipse) -> bool {
        Shape::Ellipse(ellipse).validate(self.image.0, self.image.1).is_ok()
    }

    /// `wanted` if it fits in the image, else the ellipse with radii as far from `from`'s towards
    /// `wanted`'s as fits (`from` itself if none does).
    fn largest_fitting(&self, from: Ellipse, wanted: Ellipse) -> Ellipse {
        if self.fits(wanted) {
            return wanted;
        }
        let (mut low, mut high, mut best) = (0.0, 1.0, from);
        for _ in 0..24 {
            let middle = (low + high) / 2.0;
            let towards = |from: f64, to: f64| from + (to - from) * middle;
            let between = Ellipse { rx: towards(from.rx, wanted.rx), ry: towards(from.ry, wanted.ry), ..from };
            if self.fits(between) {
                (low, best) = (middle, between);
            } else {
                high = middle;
            }
        }
        best
    }

    /// The box around every selected shape that moves (shown, not locked, not a tag); moving
    /// keeps it inside the image.
    fn movable_bounds(&self) -> Option<Rect> {
        self.selection
            .iter()
            .filter_map(|&index| self.annotations.get(index))
            .filter(|a| !a.shape.is_tag() && !a.locked && a.shown(self.threshold))
            .map(|a| a.shape.bounds())
            .reduce(|a, b| a.union(&b))
    }

    /// The single selected annotation, if it can be edited by its handles: shown and not locked.
    fn editable(&self) -> Option<(usize, &Annotation)> {
        let index = self.single_selection()?;
        let annotation = self.annotations.get(index).filter(|a| !a.locked && a.shown(self.threshold))?;
        Some((index, annotation))
    }

    /// The handles of the single selected shape, as the gesture in progress shows it, seen at
    /// `scale`; none when it is locked or not shown. Hit-testing uses the same list.
    pub fn grips(&self, scale: f64) -> Vec<Grip> {
        let Some((index, annotation)) = self.editable() else { return Vec::new() };
        let fixed = self.labels.iter().find(|l| l.id == annotation.label).is_some_and(|l| l.constraints.fixes(annotation.shape.shape_type()));
        grips(&self.shown_shape(index, annotation), fixed, scale)
    }

    /// The handle of the single selected shape under `point`, seen at `scale`.
    pub fn grip_at(&self, point: Point, scale: f64) -> Option<Grip> {
        grip_at(&self.grips(scale), point, scale)
    }

    fn grip_under(&self, point: Point, scale: f64) -> Option<(usize, Grip)> {
        Some((self.single_selection()?, self.grip_at(point, scale)?))
    }

    /// Annotation `index`'s shape as the gesture in progress shows it: being reshaped or moved.
    pub fn shown_shape<'a>(&self, index: usize, annotation: &'a Annotation) -> Cow<'a, Shape> {
        match &self.gesture {
            Gesture::Resizing { index: i, current, .. } if *i == index => Cow::Owned(Shape::Rectangle(*current)),
            Gesture::Vertex { index: i, current, .. } if *i == index => Cow::Owned(current.clone()),
            Gesture::Rotating { index: i, current, .. } if *i == index => Cow::Owned(Shape::OrientedRectangle(*current)),
            Gesture::Radius { index: i, current, .. } | Gesture::Turning { index: i, current, .. } if *i == index => Cow::Owned(Shape::Ellipse(*current)),
            Gesture::Moving { dx, dy, .. } if self.selection.contains(&index) && !annotation.locked => Cow::Owned(annotation.shape.translated(*dx, *dy)),
            _ => Cow::Borrowed(&annotation.shape),
        }
    }

    /// The selection box being dragged, if any.
    pub fn selection_box(&self) -> Option<Rect> {
        match self.gesture {
            Gesture::Selecting { start, current, .. } => Some(Rect::from_corners(start, current)),
            _ => None,
        }
    }
}
