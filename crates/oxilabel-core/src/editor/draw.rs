//! The drawing tools: a rectangle (two clicks or a drag, square, from the centre, three points
//! for an oriented one), a polygon, polyline or points shape placed vertex by vertex, a freehand
//! polyline (vertices every `spacing` image pixels, optionally following the image's edges), an
//! ellipse (from its centre or across its box) and a constrained shape placed from a label's
//! constraint. Press, move and release keep the gesture and its preview; finishing adds the
//! annotation with its label's default attribute values. Shapes too small to be meant are not
//! created.

use crate::{
    Label, ShapeType,
    annotation::{Annotation, Shape},
    constraint::ShapeConstraints,
    ellipse::Ellipse,
    geometry::{Point, Rect, snap_angle, steps_along, steps_towards},
    history::Edit,
    oriented::OrientedRect,
};

use super::{
    DRAG_THRESHOLD, EdgePath, Gesture, HANDLE_REACH, MIN_SHAPE_SIZE, Pointer, Preview, SNAP_DEGREES,
    Session, Tool, snap_turn,
};

impl Session {
    /// The width and height of the image, in image pixels.
    pub(super) fn image_size(&self) -> (f64, f64) {
        (f64::from(self.image.0), f64::from(self.image.1))
    }

    /// `point` kept inside the image.
    pub(super) fn inside(&self, point: Point) -> Point {
        let (width, height) = self.image_size();
        point.clamped(width, height)
    }

    /// The label new shapes get, if the project still has it.
    pub(super) fn label_definition(&self) -> Option<&Label> {
        self.label.and_then(|label| self.label_by_id(label))
    }

    /// The shape type the current tool draws, when a label's constraint can fix its size: the tool
    /// then places shapes instead of dragging them out.
    pub fn constrained_shape(&self) -> Option<(ShapeType, ShapeConstraints)> {
        let shape = match self.tool {
            Tool::Rectangle if self.options.oriented => ShapeType::OrientedRectangle,
            Tool::Rectangle => ShapeType::Rectangle,
            Tool::Ellipse => ShapeType::Ellipse,
            _ => return None,
        };
        let constraints = self.label_definition()?.constraints;
        constraints.fixes(shape).then_some((shape, constraints))
    }

    /// The constrained shape a click at `point` would place, and its size, for the layer's preview
    /// while the pointer moves before the click.
    pub fn constrained_preview(&self, point: Point) -> Option<(Shape, Option<(f64, f64)>)> {
        let (shape_type, constraints) = self.constrained_shape()?;
        let shape = constraints.place(shape_type, point, 0.0, self.image.0, self.image.1)?;
        Some((shape, constraints.size(shape_type)))
    }

    /// `to`, snapped to `SNAP_DEGREES` steps from `from` when Shift (or the snap option, which
    /// Shift inverts) is on, and kept inside the image.
    fn snapped(&self, from: Option<Point>, to: Point, shift: bool) -> Point {
        match from {
            Some(from) if shift != self.options.snap => self.inside(snap_angle(from, to, SNAP_DEGREES)),
            _ => to,
        }
    }

    /// Whether two points are within `HANDLE_REACH` screen pixels of each other.
    fn within_reach(&self, a: Point, b: Point, scale: f64) -> bool {
        a.distance(b) * scale <= HANDLE_REACH
    }

    /// A press of the main button with a drawing tool.
    pub(super) fn draw_press(&mut self, pointer: Pointer, edges: Option<EdgePath<'_>>, tool: Tool) {
        if self.label.is_none() {
            let shape = tool.name().to_lowercase();
            self.notice = Some(format!("Add a label that allows {shape} shapes on the Labels tab first."));
            return;
        }
        let point = self.inside(pointer.at);
        let shift = pointer.modifiers.shift;
        let alt = pointer.modifiers.alt;
        // A label's shape constraint places its shape: one click, and one more for its angle.
        if let Some((shape_type, constraints)) = self.constrained_shape() {
            let (centre, angle) = match self.gesture {
                Gesture::Placing { centre, angle } => (centre, angle),
                _ if constraints.needs_angle(shape_type) => {
                    self.gesture = Gesture::Placing { centre: point, angle: 0.0 };
                    return;
                }
                _ => (point, 0.0),
            };
            match constraints.place(shape_type, centre, angle, self.image.0, self.image.1) {
                Some(shape) => self.add_shape(shape),
                None => {
                    self.gesture = Gesture::Idle;
                    let name = self.label_definition().map(|label| label.name.clone()).unwrap_or_default();
                    self.notice = Some(format!("“{name}” has a shape constraint that does not fit this image."));
                }
            }
            return;
        }
        match (tool, self.gesture.clone()) {
            (Tool::Rectangle, Gesture::Drawing { start, square, from_centre, .. }) => {
                self.finish_rectangle(start, point, square, from_centre, pointer.scale);
            }
            (Tool::Rectangle, Gesture::Oriented { mut points, .. }) => {
                let point = self.snapped(points.last().copied(), point, shift);
                points.push(point);
                if let [a, b, c] = points[..] {
                    self.add_shape(Shape::OrientedRectangle(OrientedRect::from_three_points(a, b, c)));
                } else {
                    self.gesture = Gesture::Oriented { points, current: point };
                }
            }
            (Tool::Rectangle, _) if self.options.oriented => {
                self.gesture = Gesture::Oriented { points: vec![point], current: point };
            }
            (Tool::Rectangle, _) => {
                self.gesture = Gesture::Drawing {
                    start: point,
                    current: point,
                    dragged: false,
                    square: shift != self.options.square,
                    from_centre: alt != self.options.from_centre,
                };
            }
            (Tool::Ellipse, Gesture::Ellipse { start, circle, from_centre, .. }) => {
                self.finish_ellipse(start, point, circle, from_centre, pointer.scale);
            }
            (Tool::Ellipse, _) => {
                self.gesture = Gesture::Ellipse {
                    start: point,
                    current: point,
                    dragged: false,
                    circle: shift,
                    from_centre: alt != self.options.ellipse_from_centre,
                };
            }
            (Tool::Polyline, _) if self.options.freehand => {
                let start = if self.options.edge_snap != shift {
                    edges.and_then(|edges| edges(None, point)).and_then(|path| path.first().copied()).unwrap_or(point)
                } else {
                    point
                };
                self.gesture = Gesture::Stroke { points: vec![start], current: start };
            }
            (Tool::Points, _) if self.options.single_point => self.add_shape(Shape::Points { points: vec![point] }),
            (_, Gesture::Path { mut points, .. }) => {
                // Clicking a polygon's first vertex closes it.
                let closes =
                    tool == Tool::Polygon && points.len() >= 3 && self.within_reach(points[0], point, pointer.scale);
                if closes {
                    self.finish_path();
                } else {
                    let next = if tool == Tool::Points { point } else { self.snapped(points.last().copied(), point, shift) };
                    points.push(next);
                    self.gesture = Gesture::Path { points, current: next };
                }
            }
            _ => self.gesture = Gesture::Path { points: vec![point], current: point },
        }
    }

    /// The pointer moved with a drawing tool.
    pub(super) fn draw_move_to(&mut self, pointer: Pointer, edges: Option<EdgePath<'_>>) {
        let point = pointer.at;
        let shift = pointer.modifiers.shift;
        let inside = self.inside(point);
        let snap = shift != self.options.snap;
        self.gesture = match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Idle => Gesture::Idle,
            // Turning a constrained shape: its width side points at the pointer.
            Gesture::Placing { centre, .. } => {
                let angle = snap_turn((point.y - centre.y).atan2(point.x - centre.x), snap);
                Gesture::Placing { centre, angle }
            }
            // Only a move with the primary button held is a drag; a hover leaves the shape waiting
            // for its second click.
            Gesture::Drawing { start, dragged, .. } => {
                let moved = pointer.primary && start.distance(point) * pointer.scale > DRAG_THRESHOLD;
                Gesture::Drawing {
                    start,
                    current: inside,
                    dragged: dragged || moved,
                    square: shift != self.options.square,
                    from_centre: pointer.modifiers.alt != self.options.from_centre,
                }
            }
            Gesture::Ellipse { start, dragged, .. } => {
                let moved = pointer.primary && start.distance(point) * pointer.scale > DRAG_THRESHOLD;
                Gesture::Ellipse {
                    start,
                    current: inside,
                    dragged: dragged || moved,
                    circle: shift,
                    from_centre: pointer.modifiers.alt != self.options.ellipse_from_centre,
                }
            }
            Gesture::Stroke { mut points, .. } => {
                let to = inside;
                let last = *points.last().expect("a stroke starts with a point");
                let spacing = self.options.spacing;
                // Snapping follows the edge path from the last vertex; until its pixels are
                // loaded the line follows the pointer.
                let snapping = self.options.edge_snap != shift;
                let current = match snapping.then(|| edges.and_then(|edges| edges(Some(last), to))).flatten() {
                    Some(path) => {
                        points.extend(steps_along(&path, spacing));
                        *path.last().expect("a path has its ends")
                    }
                    None => {
                        points.extend(steps_towards(last, to, spacing));
                        to
                    }
                };
                Gesture::Stroke { points, current }
            }
            Gesture::Oriented { points, .. } => {
                let current = self.snapped(points.last().copied(), inside, shift && points.len() == 1);
                Gesture::Oriented { points, current }
            }
            Gesture::Path { points, .. } => {
                let current = if self.tool == Tool::Points {
                    inside
                } else {
                    self.snapped(points.last().copied(), inside, shift)
                };
                Gesture::Path { points, current }
            }
            // The Select tool's gestures never meet a drawing tool (changing tools cancels them).
            other => other,
        };
    }

    /// The main button was released with a drawing tool.
    pub(super) fn draw_release(&mut self, pointer: Pointer) {
        match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            // A click without dragging leaves the rectangle or ellipse open for a second click;
            // only releasing the button that made the drag finishes it.
            Gesture::Drawing { start, current, dragged: true, square, from_centre } if pointer.primary => {
                self.finish_rectangle(start, current, square, from_centre, pointer.scale);
            }
            Gesture::Ellipse { start, current, dragged: true, circle, from_centre } if pointer.primary => {
                self.finish_ellipse(start, current, circle, from_centre, pointer.scale);
            }
            // Releasing ends a freehand stroke; a stroke too short for two vertices keeps its end.
            Gesture::Stroke { mut points, current } => {
                if points.len() < 2 && points[0].distance(current) * pointer.scale >= MIN_SHAPE_SIZE {
                    points.push(current);
                }
                if points.len() < 2 {
                    self.gesture = Gesture::Idle;
                } else {
                    self.complete_path(points);
                }
            }
            other => self.gesture = other,
        }
    }

    /// Finishes the polygon, polyline or points being placed; whether there was one to finish.
    pub(super) fn finish_path(&mut self) -> bool {
        let Gesture::Path { points, .. } = self.gesture.clone() else { return false };
        self.complete_path(points);
        true
    }

    /// Drops the last vertex of the shape being placed, or the shape when only one is left.
    pub(super) fn remove_last_point(&mut self) -> bool {
        let Gesture::Path { mut points, current } = self.gesture.clone() else { return false };
        if points.len() > 1 {
            points.pop();
            self.gesture = Gesture::Path { points, current };
        } else {
            self.gesture = Gesture::Idle;
        }
        true
    }

    /// Adds the path `points` as the current tool's shape; a double-click places its point twice.
    fn complete_path(&mut self, mut points: Vec<Point>) {
        points.dedup_by(|a, b| a.distance(*b) < 0.5);
        let shape = match self.tool {
            Tool::Polygon => Shape::Polygon { points },
            Tool::Polyline => Shape::Polyline { points },
            Tool::Points => Shape::Points { points },
            _ => {
                self.gesture = Gesture::Idle;
                return;
            }
        };
        self.add_shape(shape);
    }

    /// The rectangle a drag spans, if it is large enough on screen to be intended.
    fn finish_rectangle(&mut self, start: Point, end: Point, square: bool, from_centre: bool, scale: f64) {
        let (width, height) = self.image_size();
        let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
        if rect.width() * scale < MIN_SHAPE_SIZE || rect.height() * scale < MIN_SHAPE_SIZE {
            self.gesture = Gesture::Idle;
            return;
        }
        self.add_shape(Shape::Rectangle(rect));
    }

    /// The ellipse filling the box a drag spans (as for rectangles), shrunk around its centre to
    /// stay inside the image.
    fn ellipse_from(&self, start: Point, end: Point, circle: bool, from_centre: bool) -> Ellipse {
        let rect = Rect::from_drag(start, end, circle, from_centre);
        let ellipse = Ellipse::in_box(rect.min, rect.max);
        // Exactly round: the box's halves can differ in the last bit.
        let ellipse = if circle { Ellipse { ry: ellipse.rx, ..ellipse } } else { ellipse };
        ellipse.fitted(self.image.0, self.image.1)
    }

    /// The ellipse a drag spans, if its smaller radius is large enough on screen to be intended.
    fn finish_ellipse(&mut self, start: Point, end: Point, circle: bool, from_centre: bool, scale: f64) {
        let ellipse = self.ellipse_from(start, end, circle, from_centre);
        if ellipse.rx.min(ellipse.ry) * scale >= MIN_SHAPE_SIZE / 2.0 {
            self.add_shape(Shape::Ellipse(ellipse));
        } else {
            self.gesture = Gesture::Idle;
        }
    }

    /// Adds `shape` with the current label, if the label allows it and it is valid in the image;
    /// new annotations get their label's default attribute values.
    fn add_shape(&mut self, shape: Shape) {
        self.gesture = Gesture::Idle;
        let Some(label) = self.label else { return };
        let Some(definition) = self.label_definition() else {
            self.notice = Some("“” cannot be used for this shape.".into());
            return;
        };
        if !definition.shape_types.contains(&shape.shape_type()) {
            self.notice = Some(format!("“{}” cannot be used for this shape.", definition.name));
            return;
        }
        if let Err(reason) = shape.validate(self.image.0, self.image.1) {
            self.notice = Some(format!("Not added: {reason}."));
            return;
        }
        let attributes = self.default_attributes(label);
        let annotation = Annotation { attributes, ..Annotation::manual(label, shape) };
        self.apply(Edit::Add { index: self.annotations.len(), annotation });
    }

    /// What the drawing gesture in progress shows, if any.
    pub fn preview(&self) -> Option<Preview> {
        let (width, height) = self.image_size();
        match &self.gesture {
            Gesture::Drawing { start, current, square, from_centre, .. } => {
                Some(Preview::Rectangle(Rect::from_drag(*start, *current, *square, *from_centre).clamped(width, height)))
            }
            Gesture::Oriented { points, current } => {
                let shape = match points[..] {
                    [a, b] => Some(OrientedRect::from_three_points(a, b, *current)),
                    _ => None,
                };
                Some(Preview::Oriented { points: points.clone(), current: *current, shape })
            }
            Gesture::Path { points, current } => {
                Some(Preview::Path { points: points.clone(), current: *current, shape: self.tool.shape_type()? })
            }
            Gesture::Stroke { points, current } => Some(Preview::Freehand { points: points.clone(), current: *current }),
            Gesture::Ellipse { start, current, circle, from_centre, .. } => {
                Some(Preview::Ellipse(self.ellipse_from(*start, *current, *circle, *from_centre)))
            }
            Gesture::Placing { centre, angle } => {
                let (shape_type, constraints) = self.constrained_shape()?;
                let shape = constraints.place(shape_type, *centre, *angle, self.image.0, self.image.1)?;
                Some(Preview::Placing { shape, angle: *angle })
            }
            _ => None,
        }
    }
}