//! The editor session: one image's annotations, their undo history, the selection and the gesture
//! in progress, driven by input events (press, move, release, key actions, undo, redo, a merged
//! pre-annotation run) that carry the modifiers held and the view scale. The annotation layer in
//! the browser is its adapter: it turns browser events into these, and draws what the session
//! shows.
//!
//! One rule keeps gestures safe: any change to the annotation list (an edit, undo, redo, a merged
//! run, a reload) cancels a gesture that holds annotation indices, so it can never change the
//! wrong annotation. A gesture's own result is applied after it has ended.
//!
//! The Select tool's gestures are in `select`; which handles a shape has is in `grip`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    Label, ShapeType,
    annotation::{Annotation, Shape},
    geometry::{Handle, Point, Rect},
    history::{Edit, History},
    keymap::Action,
    ellipse::Ellipse,
    oriented::OrientedRect,
};

mod draw;
pub mod grip;
mod select;
#[cfg(test)]
mod tests;

/// How far from a handle (in screen pixels) a press still grabs it; a polygon also closes on its
/// first vertex within this distance.
pub const HANDLE_REACH: f64 = 8.0;
/// How far from a shape (in screen pixels) a click still picks it.
pub const PICK_MARGIN: f64 = 4.0;
/// A press that moves less than this (in screen pixels) is a click, not a drag.
pub const DRAG_THRESHOLD: f64 = 4.0;
/// Shapes smaller than this on screen are treated as accidental and not created or kept.
pub const MIN_SHAPE_SIZE: f64 = 3.0;
/// Shift (or the snap option) turns lines and rotations in steps of this many degrees.
pub const SNAP_DEGREES: f64 = 15.0;
/// Screen distance from a shape's outline to its rotation handle.
pub const ROTATION_HANDLE_DISTANCE: f64 = 24.0;
/// Distance between freehand polyline vertices, in image pixels, and its limits.
pub const DEFAULT_SPACING: f64 = 20.0;
pub const MIN_SPACING: f64 = 1.0;
pub const MAX_SPACING: f64 = 500.0;

/// The drawing tools, and the brush, tag and Select. `Select` handles shapes; `Brush` paints masks
/// and `Tag` is never active (picking it toggles a label as an image tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Select,
    Rectangle,
    Polygon,
    Polyline,
    Points,
    Ellipse,
    Brush,
    Tag,
}

impl Tool {
    /// The shape this tool draws; `None` for Select. (Oriented rectangles come from Rectangle.)
    pub fn shape_type(self) -> Option<ShapeType> {
        match self {
            Tool::Select => None,
            Tool::Rectangle => Some(ShapeType::Rectangle),
            Tool::Polygon => Some(ShapeType::Polygon),
            Tool::Polyline => Some(ShapeType::Polyline),
            Tool::Points => Some(ShapeType::Points),
            Tool::Ellipse => Some(ShapeType::Ellipse),
            Tool::Brush => Some(ShapeType::Mask),
            Tool::Tag => Some(ShapeType::Tag),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Rectangle => "Rectangle",
            Tool::Polygon => "Polygon",
            Tool::Polyline => "Polyline",
            Tool::Points => "Points",
            Tool::Ellipse => "Ellipse",
            Tool::Brush => "Brush",
            Tool::Tag => "Tag",
        }
    }

    /// The action that switches to the tool.
    pub fn action(self) -> Action {
        match self {
            Tool::Select => Action::Select,
            Tool::Rectangle => Action::Rectangle,
            Tool::Polygon => Action::Polygon,
            Tool::Polyline => Action::Polyline,
            Tool::Points => Action::Points,
            Tool::Ellipse => Action::Ellipse,
            Tool::Brush => Action::Brush,
            Tool::Tag => Action::Tag,
        }
    }
}

/// The modifier keys held during a pointer event. `ctrl` is Ctrl, or Cmd on a Mac.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// A pointer event: where it is in image coordinates, the view's scale (screen pixels per image
/// pixel, for reach and size limits measured on screen) and the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    pub at: Point,
    pub scale: f64,
    pub modifiers: Modifiers,
}

/// The tool options gestures follow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Always snap turns and lines to `SNAP_DEGREES` steps (Shift inverts it).
    pub snap: bool,
    /// Rectangles: always square, always from the centre (Shift and Alt invert them).
    pub square: bool,
    pub from_centre: bool,
    /// Rectangles: three points for an oriented rectangle.
    pub oriented: bool,
    /// Polylines: drag to draw, placing a vertex every `spacing` image pixels.
    pub freehand: bool,
    pub spacing: f64,
    /// Points: every click is a finished one-point annotation.
    pub single_point: bool,
    /// Ellipses: drawn from their centre out (true) or across their bounding box (false); Alt
    /// inverts it.
    pub ellipse_from_centre: bool,
    /// Freehand polylines follow the image's edges (Shift inverts it).
    pub edge_snap: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            snap: false,
            square: false,
            from_centre: false,
            oriented: false,
            freehand: false,
            spacing: DEFAULT_SPACING,
            single_point: false,
            ellipse_from_centre: true,
            edge_snap: false,
        }
    }
}

/// The edge path a freehand polyline follows: from `from` (or just the strongest edge near `to`
/// when `from` is none) to `to`, in image coordinates. `None` while the pixels it needs are still
/// loading; the line then follows the pointer.
pub type EdgePath<'a> = &'a dyn Fn(Option<Point>, Point) -> Option<Vec<Point>>;

/// How a box selection combines with what is already selected (Blender: drag, Shift+drag,
/// Ctrl+drag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxMode {
    Replace,
    Extend,
    Subtract,
}

/// What the pointer is doing right now.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    Idle,
    /// Dragging a selection box over empty space.
    Selecting { start: Point, current: Point, mode: BoxMode },
    /// The selection moved together by (`dx`, `dy`), limited so every shape stays in the image.
    Moving { grab: Point, dx: f64, dy: f64 },
    /// Dragging a resize handle of rectangle `index`.
    Resizing { index: usize, handle: Handle, before: Rect, current: Rect },
    /// Dragging one vertex of annotation `index`; `current` is the shape with it moved.
    Vertex { index: usize, vertex: usize, current: Shape },
    /// Turning oriented rectangle `index` by its rotation handle.
    Rotating { index: usize, before: OrientedRect, current: OrientedRect },
    /// Dragging the handle of ellipse `index`'s radius `axis` (0 for rx, 1 for ry).
    Radius { index: usize, axis: usize, current: Ellipse },
    /// Turning ellipse `index` by its rotation handle.
    Turning { index: usize, before: Ellipse, current: Ellipse },
    /// A rectangle from `start`; `dragged` is false while waiting for the second click.
    Drawing { start: Point, current: Point, dragged: bool, square: bool, from_centre: bool },
    /// An oriented rectangle: one or two points placed, the next follows the pointer.
    Oriented { points: Vec<Point>, current: Point },
    /// A polygon, polyline or points shape being placed vertex by vertex.
    Path { points: Vec<Point>, current: Point },
    /// A freehand polyline: vertices are added every `spacing` while the button is down.
    Stroke { points: Vec<Point>, current: Point },
    /// An ellipse from `start` (its centre, or a corner of its box); like a rectangle, it is
    /// finished by releasing a drag or by a second click.
    Ellipse { start: Point, current: Point, dragged: bool, circle: bool, from_centre: bool },
    /// A fixed-size shape centred on `centre`, its angle following the pointer until a second
    /// click sets it.
    Placing { centre: Point, angle: f64 },
}

impl Gesture {
    /// Whether the gesture refers to annotations by index (the selection's, or its own).
    fn holds_indices(&self) -> bool {
        matches!(
            self,
            Gesture::Moving { .. }
                | Gesture::Resizing { .. }
                | Gesture::Vertex { .. }
                | Gesture::Rotating { .. }
                | Gesture::Radius { .. }
                | Gesture::Turning { .. }
        )
    }
}

/// What a drawing gesture shows before it is finished: the preview the annotation layer draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    /// The rectangle as it would be made now, inside the image.
    Rectangle(Rect),
    /// An oriented rectangle: one or two points placed, the next following the pointer.
    Oriented { points: Vec<Point>, current: Point },
    /// The vertices placed so far and the one following the pointer; `shape` is what the tool
    /// draws (which closes and which joins the dots).
    Path { points: Vec<Point>, current: Point, shape: ShapeType },
    /// A freehand polyline's vertices, and the point following the pointer.
    Freehand { points: Vec<Point>, current: Point },
    /// The ellipse as it would be made now, inside the image.
    Ellipse(Ellipse),
    /// A fixed-size shape being placed; `turning` once a click has fixed its centre.
    Placing { shape: Shape, turning: bool, angle: f64 },
}

/// One image's annotations being edited. Every change to them goes through the undo history.
#[derive(Debug, Clone)]
pub struct Session {
    annotations: Vec<Annotation>,
    history: History,
    /// Indices of the selected annotations.
    selection: BTreeSet<usize>,
    gesture: Gesture,
    /// The tool the pointer acts with.
    tool: Tool,
    /// The label new shapes get.
    label: Option<i64>,
    /// The project's labels: their shape constraints decide which shapes have a fixed size, and
    /// their attributes give new annotations their values.
    labels: Vec<Label>,
    /// The image's width and height in pixels.
    image: (u32, u32),
    /// Pending pre-annotations less confident than this are neither drawn nor picked.
    threshold: f64,
    options: Options,
    /// Increases with every change to the annotations.
    revision: u64,
    /// A short hint for the user from the last event, e.g. why a change was refused.
    notice: Option<String>,
}

impl Session {
    pub fn new(annotations: Vec<Annotation>, labels: Vec<Label>, image: (u32, u32)) -> Self {
        Session {
            annotations,
            history: History::default(),
            selection: BTreeSet::new(),
            gesture: Gesture::Idle,
            tool: Tool::Select,
            label: None,
            labels,
            image,
            threshold: 0.0,
            options: Options::default(),
            revision: 0,
            notice: None,
        }
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// Indices of the selected annotations.
    pub fn selection(&self) -> &BTreeSet<usize> {
        &self.selection
    }

    /// The only selected annotation, if exactly one is selected.
    pub fn single_selection(&self) -> Option<usize> {
        if self.selection.len() == 1 { self.selection.first().copied() } else { None }
    }

    /// Increases with every change to the annotations; autosave compares it with the last saved.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a gesture is in progress.
    pub fn is_busy(&self) -> bool {
        self.gesture != Gesture::Idle
    }

    /// The hint left by the last events, if any, which it clears.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    /// The tool the pointer acts with.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Switches tools; a gesture begun with the old tool no longer applies.
    pub fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.cancel();
            self.tool = tool;
        }
    }

    /// The label new shapes get.
    pub fn label(&self) -> Option<i64> {
        self.label
    }

    /// Sets the label new shapes get; a fixed-size shape being placed no longer matches its size.
    pub fn set_label(&mut self, label: Option<i64>) {
        if self.label != label {
            if matches!(self.gesture, Gesture::Placing { .. }) {
                self.cancel();
            }
            self.label = label;
        }
    }

    /// A press of the main button.
    pub fn press(&mut self, pointer: Pointer) {
        self.press_with_edges(pointer, None);
    }

    /// A press where `edges` supplies the path a freehand polyline follows along the image's
    /// edges; without it the line follows the pointer.
    pub fn press_with_edges(&mut self, pointer: Pointer, edges: Option<EdgePath<'_>>) {
        match self.tool {
            Tool::Select => self.select_press(pointer),
            // The brush, eraser and fuzzy fill are the annotation layer's until ticket 13.
            Tool::Brush | Tool::Tag => {}
            tool => self.draw_press(pointer, edges, tool),
        }
    }

    /// The pointer moved (with or without a button down).
    pub fn move_to(&mut self, pointer: Pointer) {
        self.move_to_with_edges(pointer, None);
    }

    /// A move where `edges` supplies the path a freehand polyline follows along the image's edges.
    pub fn move_to_with_edges(&mut self, pointer: Pointer, edges: Option<EdgePath<'_>>) {
        match self.tool {
            Tool::Select => self.select_move_to(pointer),
            Tool::Brush | Tool::Tag => {}
            _ => self.draw_move_to(pointer, edges),
        }
    }

    /// The main button was released: the gesture's result is applied.
    pub fn release(&mut self, pointer: Pointer) {
        match self.tool {
            Tool::Select => self.select_release(pointer),
            Tool::Brush | Tool::Tag => {}
            _ => self.draw_release(pointer),
        }
    }

    /// The confidence below which pending pre-annotations are neither drawn nor picked.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// Sets the confidence threshold; what it hides cannot stay selected.
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
        let annotations = &self.annotations;
        self.selection.retain(|&index| annotations.get(index).is_some_and(|a| a.shown(threshold)));
    }

    /// Ends the gesture in progress without changing anything.
    pub fn cancel(&mut self) {
        self.gesture = Gesture::Idle;
    }

    /// Handles a key action; returns whether it did anything. Cancel (Esc) first ends the
    /// gesture, then clears the selection, and otherwise is left to the caller.
    pub fn key(&mut self, action: Action) -> bool {
        match action {
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::SelectNone => self.selection.clear(),
            // Everything shown: hidden shapes are not picked.
            Action::SelectAll => {
                let threshold = self.threshold;
                self.selection = self.annotations.iter().enumerate().filter(|(_, a)| a.shown(threshold)).map(|(i, _)| i).collect();
            }
            Action::Delete => self.delete_selected(),
            Action::Finish => return self.finish_path(),
            Action::RemoveLastPoint => return self.remove_last_point(),
            Action::Cancel if self.is_busy() => self.cancel(),
            Action::Cancel if !self.selection.is_empty() => self.selection.clear(),
            _ => return false,
        }
        true
    }

    // ---- The annotation list. Every change goes through `apply`, `undo`, `redo` or `reset`,
    // which cancel a gesture holding indices into it.

    /// The list changes: a gesture holding indices into it can no longer apply.
    fn list_changed(&mut self) {
        if self.gesture.holds_indices() {
            self.gesture = Gesture::Idle;
        }
        self.revision += 1;
    }

    /// Replaces everything with `annotations` as loaded from the server; nothing to undo.
    pub fn reset(&mut self, annotations: Vec<Annotation>) {
        self.history = History::default();
        self.selection.clear();
        self.annotations = annotations;
        self.list_changed();
    }

    /// Performs `edit` as one undo step.
    pub fn apply(&mut self, edit: Edit) {
        self.history.apply(&mut self.annotations, edit);
        self.list_changed();
    }

    /// Reverses the last edit and clears the selection, whose indices may no longer fit.
    pub fn undo(&mut self) {
        if self.history.undo(&mut self.annotations) {
            self.selection.clear();
            self.list_changed();
        }
    }

    /// Performs the last undone edit again and clears the selection.
    pub fn redo(&mut self) {
        if self.history.redo(&mut self.annotations) {
            self.selection.clear();
            self.list_changed();
        }
    }

    /// Replaces the selection.
    pub fn set_selection(&mut self, selection: BTreeSet<usize>) {
        self.selection = selection;
    }

    /// Adds annotation `index` to the selection, or removes it if it is selected.
    pub fn toggle_selected(&mut self, index: usize) {
        if !self.selection.remove(&index) {
            self.selection.insert(index);
        }
    }

    /// Replaces annotation `index`'s shape (one undo step) unless it is unchanged.
    fn reshape(&mut self, index: usize, shape: Shape) {
        let Some(before) = self.annotations.get(index).cloned() else { return };
        if before.shape != shape {
            // Editing a pre-annotation reviews it.
            let after = Annotation { shape, pending: false, ..before.clone() };
            self.apply(Edit::Replace { index, before, after });
        }
    }

    /// Removes annotation `index` (one undo step) and clears the selection, whose indices would
    /// no longer fit.
    pub fn remove(&mut self, index: usize) {
        let Some(annotation) = self.annotations.get(index).cloned() else { return };
        self.selection.clear();
        self.apply(Edit::Remove { index, annotation });
    }

    /// Removes every selected annotation that is shown and not locked, in one undo step.
    pub fn delete_selected(&mut self) {
        let locked = self.selection.iter().filter(|&&index| self.annotations.get(index).is_some_and(|a| a.locked)).count();
        if locked > 0 {
            let (shapes, verb) = if locked == 1 { ("shape", "stays") } else { ("shapes", "stay") };
            self.notice = Some(format!("{locked} locked {shapes} {verb}: unlock them in the objects list first."));
        }
        // Highest index first, so the others keep their positions while removing.
        let removals: Vec<Edit> = self
            .selection
            .iter()
            .rev()
            .filter_map(|&index| {
                self.annotations
                    .get(index)
                    .filter(|a| !a.locked && a.shown(self.threshold))
                    .map(|annotation| Edit::Remove { index, annotation: annotation.clone() })
            })
            .collect();
        if removals.is_empty() {
            return;
        }
        self.selection.clear();
        self.apply(Edit::Batch(removals));
    }

    /// Sets annotation `index`'s hidden or locked flag (one undo step); hiding unselects it.
    pub fn set_flag(&mut self, index: usize, hidden: Option<bool>, locked: Option<bool>) {
        let Some(before) = self.annotations.get(index).cloned() else { return };
        let after = Annotation { hidden: hidden.unwrap_or(before.hidden), locked: locked.unwrap_or(before.locked), ..before.clone() };
        if after.hidden {
            self.selection.remove(&index);
        }
        if after != before {
            self.apply(Edit::Replace { index, before, after });
        }
    }

    /// Replaces the selected annotations with `change` applied to each, in one undo step; those
    /// it leaves unchanged are skipped.
    pub fn change_selected(&mut self, change: impl Fn(&Annotation) -> Annotation) {
        let edits: Vec<Edit> = self
            .selection
            .iter()
            .filter_map(|&index| {
                // Shapes not shown are left alone (they cannot be selected on purpose);
                // changing a pre-annotation reviews it.
                let before = self.annotations.get(index).filter(|a| a.shown(self.threshold))?;
                let after = Annotation { pending: false, ..change(before) };
                (after != *before).then(|| Edit::Replace { index, before: before.clone(), after })
            })
            .collect();
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits));
        }
    }

    /// Replaces `source`'s pending pre-annotations with those it made in `latest` (the image as the
    /// server now has it), in one undo step, keeping every other annotation as it is here.
    pub fn merge_pre_annotations(&mut self, source: &str, latest: &[Annotation]) {
        let ours = |a: &Annotation| a.pending && a.source.as_deref() == Some(source);
        // Highest index first, so the others keep their positions while removing.
        let mut edits: Vec<Edit> =
            self.annotations.iter().enumerate().rev().filter(|(_, a)| ours(a)).map(|(index, a)| Edit::Remove { index, annotation: a.clone() }).collect();
        let kept = self.annotations.len() - edits.len();
        edits.extend(latest.iter().filter(|a| ours(a)).enumerate().map(|(offset, a)| Edit::Add { index: kept + offset, annotation: a.clone() }));
        if !edits.is_empty() {
            self.selection.clear();
            self.apply(Edit::Batch(edits));
        }
    }

    /// Marks the pending pre-annotations at `indices` reviewed, in one undo step.
    pub fn accept(&mut self, indices: &[usize]) {
        let edits: Vec<Edit> = indices
            .iter()
            .filter_map(|&index| {
                let before = self.annotations.get(index).filter(|a| a.pending)?.clone();
                Some(Edit::Replace { index, after: Annotation { pending: false, ..before.clone() }, before })
            })
            .collect();
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits));
        }
    }

    /// The pending pre-annotations the user has not hidden: those shown (at or above the
    /// threshold), and those below it.
    pub fn pending(&self) -> (Vec<usize>, Vec<usize>) {
        let list = &self.annotations;
        list.iter()
            .enumerate()
            .filter(|(_, a)| a.pending && !a.hidden)
            .map(|(index, _)| index)
            .partition(|&index| list[index].confidence.unwrap_or(1.0) >= self.threshold)
    }

    /// Removes the annotations at `indices` that are not locked, in one undo step, and clears the
    /// selection.
    pub fn remove_all(&mut self, indices: &[usize]) {
        // Highest index first, so the others keep their positions while removing.
        let removals: Vec<Edit> = indices
            .iter()
            .rev()
            .filter_map(|&index| self.annotations.get(index).filter(|a| !a.locked).map(|annotation| Edit::Remove { index, annotation: annotation.clone() }))
            .collect();
        if !removals.is_empty() {
            self.selection.clear();
            self.apply(Edit::Batch(removals));
        }
    }
}

/// `angle` rounded to whole steps of `SNAP_DEGREES` when `snap` is on.
pub fn snap_turn(angle: f64, snap: bool) -> f64 {
    let step = SNAP_DEGREES.to_radians();
    if snap { (angle / step).round() * step } else { angle }
}
