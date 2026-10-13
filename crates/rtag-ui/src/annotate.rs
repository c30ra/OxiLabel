//! Drawing and editing annotations on top of the tile viewer.
//!
//! Select (V) is the default tool: click picks the topmost shape, drag moves the selection, the
//! handles of a single selected shape edit it (rectangle handles resize; vertices drag; the round
//! "+" on an edge inserts a vertex; Ctrl+click removes one; an oriented rectangle's round handle
//! turns it). Delete or X removes the selection. Drawing tools: Rectangle (R; two clicks or a
//! drag, Shift square, Alt from the centre, or three points when "3-point oriented" is on),
//! Polygon (P), Polyline (L) and Points (.), which take a click per vertex and finish with Enter
//! or a double-click (a polygon also by clicking its first vertex); Backspace drops the last
//! vertex and Shift snaps lines to 15°. Freehand polylines are dragged, with a vertex every
//! "spacing" pixels (`[` and `]` change it); single-point mode makes each click one point.
//! Circle (C) draws centre and radius (or a diameter) by a drag or two clicks. Brush (B) paints
//! masks and its eraser mode (E) erases them (see `masks`); Ctrl inverts a stroke, and `[` and
//! `]` or F (then move the pointer right or left and click; Esc cancels) change the size. Its fuzzy fill mode
//! (U) fills the similar region around a click (`fill`), a drag changing the threshold. Freehand
//! polylines can snap to edges (live-wire, `edges`) and the brush can stop at edges (painting
//! only the similar region around the stroke's start); Shift inverts either while drawing. Esc
//! first cancels the shape being drawn, then clears the selection, then returns to Select. Ctrl+Z
//! and Ctrl+Y undo and redo.
//!
//! Holding Q or the right mouse button opens a pie at the cursor: with Select active the tool pie
//! (then the label pie), with a drawing tool its options pie, with a shape selected the label pie
//! to relabel it. Release or flick picks; a quick tap repeats the last tool and label. Shift+Q
//! opens only the label pie. Tag (T, or the tool pie) opens the label pie and toggles that label
//! as a tag of the whole image; tags show as chips over the image.

use std::collections::{BTreeSet, HashMap};

use leptos::{ev, html, prelude::*};
use rtag_core::{
    Label, ShapeType,
    annotation::{Annotation, Shape, overlapping, topmost_at},
    attribute::Attributes,
    edges::EdgeMap,
    fill::{FillSettings, grow},
    geometry::{Handle, Point, Rect, snap_angle, steps_along, steps_towards},
    mask::{Bitmap, PixelRect},
    pyramid::Pyramid,
    history::{Edit, History},
    oriented::OrientedRect,
    pie::{Compass, FLICK_DISTANCE, is_tap},
    view::View,
};

use crate::{
    draw::{self, Look, ROTATION_HANDLE_DISTANCE},
    analysis::Analysis,
    masks::{FillPreview, SpriteCache, Stroke},
    pie::{LabelSearch, Pie, PieAction, PieItem, PieMenu, SearchEntry, now},
    viewer::{context_2d, typing_in_a_field},
};

/// How far from a handle or a shape (in screen pixels) a click still hits it.
const HANDLE_REACH: f64 = 8.0;
const PICK_MARGIN: f64 = 4.0;
/// A press that moves less than this (in screen pixels) is a click, not a drag.
const DRAG_THRESHOLD: f64 = 4.0;
/// Shapes smaller than this on screen are treated as accidental and not created.
const MIN_SHAPE_SIZE: f64 = 3.0;
/// Shift (or the snap option) turns lines and rotations in steps of this many degrees.
const SNAP_DEGREES: f64 = 15.0;
/// Distance between freehand polyline vertices, in image pixels, and its limits; `[` and `]`
/// change it by this factor.
pub const DEFAULT_SPACING: f64 = 20.0;
pub const MIN_SPACING: f64 = 1.0;
pub const MAX_SPACING: f64 = 500.0;
/// Brush diameter in image pixels, and its limits.
pub const DEFAULT_BRUSH_SIZE: f64 = 20.0;
pub const MIN_BRUSH_SIZE: f64 = 1.0;
pub const MAX_BRUSH_SIZE: f64 = 1000.0;
/// Fuzzy fill defaults: the largest channel difference from the clicked pixel (GIMP's default),
/// how much a drag (in screen pixels, right and down) raises it, and the largest region as a
/// share of the image.
pub const DEFAULT_THRESHOLD: f64 = 15.0;
const THRESHOLD_PER_PIXEL: f64 = 0.5;
/// `[` and `]` change the threshold by this much.
const THRESHOLD_STEP: f64 = 5.0;
pub const MAX_THRESHOLD: f64 = 255.0;
pub const DEFAULT_MAX_FILL_PERCENT: f64 = 25.0;
/// Edge snapping looks this far around the pointer, in screen pixels, by default.
pub const DEFAULT_SNAP_RADIUS: f64 = 12.0;
/// The largest "Stop at edges" offset either way, in image pixels.
pub const MAX_EDGE_OFFSET: f64 = 200.0;
/// `[` and `]` change the spacing and the brush size by this factor.
const SIZE_STEP: f64 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
    Polygon,
    Polyline,
    Points,
    Circle,
    /// Paints masks; its eraser mode erases them.
    Brush,
    /// Never active: picking it, then a label, toggles that label as an image tag.
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
            Tool::Circle => Some(ShapeType::Circle),
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
            Tool::Circle => "Circle",
            Tool::Brush => "Brush",
            Tool::Tag => "Tag",
        }
    }

    /// The tool a key switches to.
    fn for_key(key: &str) -> Option<Tool> {
        match key {
            "v" | "V" => Some(Tool::Select),
            "r" | "R" => Some(Tool::Rectangle),
            "p" | "P" => Some(Tool::Polygon),
            "l" | "L" => Some(Tool::Polyline),
            "." => Some(Tool::Points),
            "c" | "C" => Some(Tool::Circle),
            "b" | "B" => Some(Tool::Brush),
            _ => None,
        }
    }
}

/// The annotations of the open image and everything that changes them. All changes go through
/// the undo history.
#[derive(Clone, Copy)]
pub struct Editor {
    pub annotations: RwSignal<Vec<Annotation>>,
    history: StoredValue<History>,
    /// Indices of the selected annotations.
    pub selection: RwSignal<BTreeSet<usize>>,
    pub tool: RwSignal<Tool>,
    /// The label new shapes get.
    pub label: RwSignal<Option<i64>>,
    /// Increases with every edit, undo and redo; autosave compares it with the last saved one.
    pub revision: RwSignal<u64>,
    /// A short hint for the user, e.g. why a shape could not be drawn.
    pub notice: RwSignal<Option<String>>,
    /// Rectangle options from the options pie: always square, always from the centre (Shift and
    /// Alt invert them while drawing), and three points for an oriented rectangle.
    pub square: RwSignal<bool>,
    pub from_centre: RwSignal<bool>,
    pub oriented: RwSignal<bool>,
    /// Line options: always snap to 15° (Shift inverts).
    pub snap: RwSignal<bool>,
    /// Polyline: draw by dragging, placing a vertex every `spacing` image pixels.
    pub freehand: RwSignal<bool>,
    pub spacing: RwSignal<f64>,
    /// Points: every click is a finished one-point annotation.
    pub single_point: RwSignal<bool>,
    /// Circle: the two points are the ends of a diameter instead of centre and radius.
    pub diameter: RwSignal<bool>,
    /// Brush: diameter in image pixels, and whether it erases.
    pub brush_size: RwSignal<f64>,
    pub eraser: RwSignal<bool>,
    /// Brush: fuzzy fill instead of painting, with its threshold (0–255), contiguous or global
    /// reach, clean-up size (patches and holes smaller than this many image pixels go) and
    /// largest region (percent of the image).
    pub fill: RwSignal<bool>,
    pub threshold: RwSignal<f64>,
    pub contiguous: RwSignal<bool>,
    pub clean_up: RwSignal<f64>,
    pub max_fill: RwSignal<f64>,
    /// Freehand polylines follow edges within `snap_radius` screen pixels of the pointer.
    pub edge_snap: RwSignal<bool>,
    pub snap_radius: RwSignal<f64>,
    /// The brush paints only the region similar to where the stroke starts (the fill's
    /// threshold), grown or shrunk by `edge_offset` image pixels.
    pub stop_at_edges: RwSignal<bool>,
    pub edge_offset: RwSignal<f64>,
    /// The last drawing tool, label and modes, repeated by a tap of the pie key.
    pub last: RwSignal<Option<(Tool, i64, Modes)>>,
    /// The shape types some label of the project allows; tools for others are disabled.
    pub usable_shapes: RwSignal<Vec<ShapeType>>,
    /// Whether a shape, stroke or other gesture is in progress (moving to another image waits).
    pub busy: RwSignal<bool>,
}

/// The tools' drawing modes, remembered with the last tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modes {
    oriented: bool,
    freehand: bool,
    single_point: bool,
    diameter: bool,
    fill: bool,
    eraser: bool,
}

impl Editor {
    /// Whether `tool` can be used: Select always, a drawing tool when some label allows its shape.
    pub fn can_use(&self, tool: Tool) -> bool {
        tool.shape_type().is_none_or(|shape| self.usable_shapes.with(|shapes| shapes.contains(&shape)))
    }

    /// Why a tool cannot be used, for a notice.
    pub fn unusable_notice(tool: Tool) -> String {
        format!("No label allows {} shapes: create or edit labels on the Labels tab.", tool.name().to_lowercase())
    }

    fn mode_signals(&self) -> [RwSignal<bool>; 6] {
        [self.oriented, self.freehand, self.single_point, self.diameter, self.fill, self.eraser]
    }

    /// The current modes (tracked).
    pub fn modes(&self) -> Modes {
        let [oriented, freehand, single_point, diameter, fill, eraser] = self.mode_signals().map(|mode| mode.get());
        Modes { oriented, freehand, single_point, diameter, fill, eraser }
    }

    /// Switches to `tool`, `label` and `modes`, touching only what differs, so a shape being
    /// drawn with them is kept.
    fn restore(&self, tool: Tool, label: i64, modes: Modes) {
        let Modes { oriented, freehand, single_point, diameter, fill, eraser } = modes;
        for (signal, value) in self.mode_signals().into_iter().zip([oriented, freehand, single_point, diameter, fill, eraser]) {
            if signal.get_untracked() != value {
                signal.set(value);
            }
        }
        if self.tool.get_untracked() != tool {
            self.tool.set(tool);
        }
        if self.label.get_untracked() != Some(label) {
            self.label.set(Some(label));
        }
    }

    pub fn new(annotations: Vec<Annotation>, label: Option<i64>) -> Self {
        Editor {
            annotations: RwSignal::new(annotations),
            history: StoredValue::new(History::default()),
            selection: RwSignal::new(BTreeSet::new()),
            tool: RwSignal::new(Tool::Select),
            label: RwSignal::new(label),
            revision: RwSignal::new(0),
            notice: RwSignal::new(None),
            square: RwSignal::new(false),
            from_centre: RwSignal::new(false),
            oriented: RwSignal::new(false),
            snap: RwSignal::new(false),
            freehand: RwSignal::new(false),
            spacing: RwSignal::new(DEFAULT_SPACING),
            single_point: RwSignal::new(false),
            diameter: RwSignal::new(false),
            brush_size: RwSignal::new(DEFAULT_BRUSH_SIZE),
            eraser: RwSignal::new(false),
            fill: RwSignal::new(false),
            threshold: RwSignal::new(DEFAULT_THRESHOLD),
            contiguous: RwSignal::new(true),
            clean_up: RwSignal::new(0.0),
            max_fill: RwSignal::new(DEFAULT_MAX_FILL_PERCENT),
            edge_snap: RwSignal::new(false),
            snap_radius: RwSignal::new(DEFAULT_SNAP_RADIUS),
            stop_at_edges: RwSignal::new(false),
            edge_offset: RwSignal::new(0.0),
            last: RwSignal::new(None),
            usable_shapes: RwSignal::new(ShapeType::ALL.to_vec()),
            busy: RwSignal::new(false),
        }
    }

    /// Replaces everything with `annotations` as loaded from the server; nothing to undo.
    pub fn reset(&self, annotations: Vec<Annotation>) {
        self.history.set_value(History::default());
        self.selection.set(BTreeSet::new());
        self.annotations.set(annotations);
    }

    pub fn apply(&self, edit: Edit) {
        self.annotations.update(|list| self.history.update_value(|history| history.apply(list, edit)));
        self.revision.update(|r| *r += 1);
    }

    pub fn undo(&self) {
        let mut undone = false;
        self.annotations.update(|list| self.history.update_value(|history| undone = history.undo(list)));
        if undone {
            self.selection.set(BTreeSet::new());
            self.revision.update(|r| *r += 1);
        }
    }

    pub fn redo(&self) {
        let mut redone = false;
        self.annotations.update(|list| self.history.update_value(|history| redone = history.redo(list)));
        if redone {
            self.selection.set(BTreeSet::new());
            self.revision.update(|r| *r += 1);
        }
    }

    /// The only selected annotation, if exactly one is selected.
    pub fn single_selection(&self) -> Option<usize> {
        self.selection.with_untracked(|set| if set.len() == 1 { set.first().copied() } else { None })
    }

    fn annotation(&self, index: usize) -> Option<Annotation> {
        self.annotations.with_untracked(|list| list.get(index).cloned())
    }

    /// Replaces annotation `index`'s shape (one undo step) unless it is unchanged.
    fn reshape(&self, index: usize, shape: Shape) {
        let Some(before) = self.annotation(index) else { return };
        if before.shape != shape {
            let after = Annotation { shape, ..before.clone() };
            self.apply(Edit::Replace { index, before, after });
        }
    }

    /// Removes annotation `index` (one undo step) and clears the selection, whose indices would
    /// no longer fit.
    pub fn remove(&self, index: usize) {
        let Some(annotation) = self.annotation(index) else { return };
        self.selection.set(BTreeSet::new());
        self.apply(Edit::Remove { index, annotation });
    }

    /// Removes every selected annotation that is not locked, in one undo step.
    pub fn delete_selected(&self) {
        let selection = self.selection.get_untracked();
        let locked = selection.iter().filter(|&&index| self.annotation(index).is_some_and(|a| a.locked)).count();
        if locked > 0 {
            let (shapes, verb) = if locked == 1 { ("shape", "stays") } else { ("shapes", "stay") };
            self.notice.set(Some(format!("{locked} locked {shapes} {verb}: unlock them in the objects list first.")));
        }
        // Highest index first, so the others keep their positions while removing.
        let removals: Vec<Edit> = self.annotations.with_untracked(|list| {
            selection
                .iter()
                .rev()
                .filter_map(|&index| list.get(index).filter(|a| !a.locked && !a.hidden).map(|annotation| Edit::Remove { index, annotation: annotation.clone() }))
                .collect()
        });
        if removals.is_empty() {
            return;
        }
        self.selection.set(BTreeSet::new());
        self.apply(Edit::Batch(removals));
    }

    /// Sets annotation `index`'s hidden or locked flag (one undo step); hiding unselects it.
    pub fn set_flag(&self, index: usize, hidden: Option<bool>, locked: Option<bool>) {
        let Some(before) = self.annotation(index) else { return };
        let after = Annotation { hidden: hidden.unwrap_or(before.hidden), locked: locked.unwrap_or(before.locked), ..before.clone() };
        if after.hidden {
            self.selection.update(|set| {
                set.remove(&index);
            });
        }
        if after != before {
            self.apply(Edit::Replace { index, before, after });
        }
    }

    /// Replaces the selected annotations with `change` applied to each, in one undo step; those
    /// it leaves unchanged are skipped.
    pub fn change_selected(&self, change: impl Fn(&Annotation) -> Annotation) {
        let selection = self.selection.get_untracked();
        let edits: Vec<Edit> = self.annotations.with_untracked(|list| {
            selection
                .iter()
                .filter_map(|&index| {
                    // Hidden shapes are left alone (they cannot be selected on purpose).
                    let before = list.get(index).filter(|a| !a.hidden)?.clone();
                    let after = change(&before);
                    (after != before).then_some(Edit::Replace { index, before, after })
                })
                .collect()
        });
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits));
        }
    }
}

/// How a box selection combines with what is already selected (Blender: drag, Shift+drag, Ctrl+drag).
#[derive(Debug, Clone, Copy, PartialEq)]
enum BoxMode {
    Replace,
    Extend,
    Subtract,
}

/// What the pointer is doing right now.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    Idle,
    /// A rectangle from `start`; `dragged` is false while waiting for the second click.
    Drawing { start: Point, current: Point, dragged: bool, square: bool, from_centre: bool },
    /// An oriented rectangle: one or two points placed, the next follows the pointer.
    Oriented { points: Vec<Point>, current: Point },
    /// A polygon, polyline or points shape being placed vertex by vertex.
    Path { points: Vec<Point>, current: Point },
    /// A freehand polyline: vertices are added every `spacing` while the button is down.
    Stroke { points: Vec<Point>, current: Point },
    /// A circle from `start` (centre, or one end of the diameter); like a rectangle, it is
    /// finished by releasing a drag or by a second click.
    Circle { start: Point, current: Point, dragged: bool },
    /// A brush or eraser stroke; its pixels are in the `Stroke` kept beside the gesture.
    Painting,
    /// F with the brush: the outline stays at `centre` (where the pointer was) and the size
    /// changes by twice the pointer's horizontal move: right larger, left smaller. `before` is
    /// restored on cancel.
    BrushSizing { centre: Point, before: f64 },
    /// A fuzzy fill from `seed`, on the tiles of the viewed level; dragging from `anchor` changes
    /// the threshold from `base`. Once `released`, it is committed as soon as growing is complete.
    Filling { seed: Point, anchor: Point, base: f64, erase: bool, released: bool },
    /// Dragging a circle's radius handle.
    Radius { index: usize, centre: Point, radius: f64 },
    /// The selection moved together by (`dx`, `dy`), limited so every shape stays in the image.
    Moving { grab: Point, dx: f64, dy: f64 },
    Resizing { index: usize, handle: Handle, before: Rect, current: Rect },
    /// Dragging one vertex (or rectangle corner) of annotation `index`.
    Vertex { index: usize, vertex: usize, current: Shape },
    Rotating { index: usize, before: OrientedRect, current: OrientedRect },
    /// Dragging a selection box over empty space.
    Selecting { start: Point, current: Point, mode: BoxMode },
}

/// What is under the pointer on the single selected shape.
enum Grip {
    Resize(Handle, Rect),
    Radius(Point),
    Vertex(usize),
    Midpoint(usize),
    Rotate(OrientedRect),
}

#[component]
pub fn AnnotationLayer(
    editor: Editor,
    labels: Vec<Label>,
    image_id: i64,
    image_size: (u32, u32),
    view: RwSignal<Option<View>>,
    size: RwSignal<(f64, f64)>,
    space_held: RwSignal<bool>,
) -> impl IntoView {
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let gesture = RwSignal::new(Gesture::Idle);
    let cursor = RwSignal::new("default");
    let colors: StoredValue<HashMap<i64, String>> =
        StoredValue::new(labels.iter().map(|label| (label.id, label.color.clone())).collect());
    let color_of = move |label: i64| colors.with_value(|colors| colors.get(&label).cloned()).unwrap_or_else(|| "#ffffff".to_owned());
    let names: HashMap<i64, String> = labels.iter().map(|label| (label.id, label.name.clone())).collect();
    let hotkeys: Vec<(String, i64)> =
        labels.iter().filter_map(|label| label.hotkey.clone().map(|key| (key, label.id))).collect();
    let all_labels = StoredValue::new({
        let mut sorted = labels.clone();
        sorted.sort_by_key(|label| label.id);
        sorted
    });
    let (width, height) = (f64::from(image_size.0), f64::from(image_size.1));
    let inside = move |point: Point| point.clamped(width, height);
    let label_allows = move |id: i64, shape: ShapeType| {
        all_labels.with_value(|labels| labels.iter().any(|label| label.id == id && label.shape_types.contains(&shape)))
    };
    // The attribute values new annotations with a label get, and values made to fit a label.
    let conform = move |id: i64, values: &Attributes| {
        all_labels.with_value(|labels| labels.iter().find(|l| l.id == id).map(|l| l.conform_attributes(values)).unwrap_or_default())
    };
    let defaults = move |id: i64| conform(id, &Attributes::new());
    let label_name = move |id: i64| all_labels.with_value(|labels| labels.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_default());

    // Pointer position in canvas pixels and in image coordinates.
    let locate = move |ev: &ev::MouseEvent| -> Option<(Point, Point, View)> {
        let canvas = canvas_ref.get_untracked()?;
        let current = view.get_untracked()?;
        let rect = canvas.get_bounding_client_rect();
        let screen = Point::new(f64::from(ev.client_x()) - rect.left(), f64::from(ev.client_y()) - rect.top());
        Some((screen, current.to_image(screen), current))
    };
    // The box around every selected shape; moving keeps it inside the image.
    let selection_bounds = move || {
        let selection = editor.selection.get_untracked();
        editor.annotations.with_untracked(|list| {
            selection
                .iter()
                .filter_map(|&index| list.get(index).filter(|a| !a.shape.is_tag() && !a.locked && !a.hidden).map(|a| a.shape.bounds()))
                .reduce(|a, b| a.union(&b))
        })
    };
    // The handle of the single selected shape under `point`, if any.
    let grip_at = move |point: Point, scale: f64| -> Option<(usize, Grip)> {
        let index = editor.single_selection()?;
        let shape = editor.annotation(index).filter(|a| !a.locked && !a.hidden)?.shape;
        let reach = HANDLE_REACH / scale;
        let grip = match &shape {
            Shape::Rectangle(rect) => rect.handle_at(point, reach).map(|handle| Grip::Resize(handle, *rect)),
            Shape::OrientedRectangle(rect) => {
                if rect.rotation_handle(ROTATION_HANDLE_DISTANCE / scale).distance(point) <= reach {
                    Some(Grip::Rotate(*rect))
                } else {
                    nearest(&shape.vertices(), point, reach).map(Grip::Vertex)
                }
            }
            Shape::Circle { centre, radius } => {
                if Point::new(centre.x + radius, centre.y).distance(point) <= reach {
                    Some(Grip::Radius(*centre))
                } else {
                    nearest(&shape.vertices(), point, reach).map(Grip::Vertex)
                }
            }
            Shape::Mask(_) | Shape::Tag => None,
            Shape::Polygon { .. } | Shape::Polyline { .. } | Shape::Points { .. } => {
                nearest(&shape.vertices(), point, reach).map(Grip::Vertex).or_else(|| {
                    let midpoints: Vec<Point> = match shape {
                        Shape::Points { .. } => Vec::new(),
                        _ => shape.edges().iter().map(|(a, b)| a.midpoint(*b)).collect(),
                    };
                    nearest(&midpoints, point, reach).map(Grip::Midpoint)
                })
            }
        };
        grip.map(|grip| (index, grip))
    };

    // Adds a finished shape with the current label, if it is valid and the label allows it.
    let add_shape = move |shape: Shape| {
        gesture.set(Gesture::Idle);
        let Some(label) = editor.label.get_untracked() else { return };
        if !label_allows(label, shape.shape_type()) {
            editor.notice.set(Some(format!("“{}” cannot be used for this shape.", label_name(label))));
            return;
        }
        if let Err(reason) = shape.validate(image_size.0, image_size.1) {
            editor.notice.set(Some(format!("Not added: {reason}.")));
            return;
        }
        let index = editor.annotations.with_untracked(Vec::len);
        editor.apply(Edit::Add { index, annotation: Annotation { attributes: defaults(label), ..Annotation::manual(label, shape) } });
    };
    let finish_rectangle = move |start: Point, end: Point, square: bool, from_centre: bool, scale: f64| {
        let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
        if rect.width() * scale < MIN_SHAPE_SIZE || rect.height() * scale < MIN_SHAPE_SIZE {
            gesture.set(Gesture::Idle);
            return;
        }
        add_shape(Shape::Rectangle(rect));
    };
    // A circle from two points, shrunk if needed so it stays inside the image.
    let circle_from = move |start: Point, end: Point| {
        let (centre, radius) = if editor.diameter.get_untracked() {
            (start.midpoint(end), start.distance(end) / 2.0)
        } else {
            (start, start.distance(end))
        };
        let room = centre.x.min(centre.y).min(width - centre.x).min(height - centre.y);
        Shape::Circle { centre, radius: radius.min(room) }
    };
    let finish_circle = move |start: Point, end: Point, scale: f64| {
        let circle = circle_from(start, end);
        match circle {
            Shape::Circle { radius, .. } if radius * scale >= MIN_SHAPE_SIZE / 2.0 => add_shape(circle),
            _ => gesture.set(Gesture::Idle),
        }
    };
    // Finishes the polygon, polyline or points being placed.
    let finish_path = move || {
        let (Gesture::Path { mut points, .. } | Gesture::Stroke { mut points, .. }) = gesture.get_untracked() else { return };
        // A double-click places the same point twice.
        points.dedup_by(|a, b| a.distance(*b) < 0.5);
        let shape = match editor.tool.get_untracked() {
            Tool::Polygon => Shape::Polygon { points },
            Tool::Polyline => Shape::Polyline { points },
            Tool::Points => Shape::Points { points },
            _ => return gesture.set(Gesture::Idle),
        };
        add_shape(shape);
    };
    // Shift (or the snap option) snaps a line from the last placed vertex to 15° steps.
    let snapped = move |from: Option<Point>, to: Point, shift: bool| match from {
        Some(from) if shift != editor.snap.get_untracked() => inside(snap_angle(from, to, SNAP_DEGREES)),
        _ => to,
    };

    // `[` and `]`: the brush size in steps of 25%, but at least one pixel.
    let resize_brush = move |larger: bool| {
        editor.brush_size.update(|size| {
            let next = if larger { (*size * SIZE_STEP).max(*size + 1.0) } else { (*size / SIZE_STEP).min(*size - 1.0) };
            *size = next.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE);
        });
    };
    let change_threshold = move |higher: bool| {
        let step = if higher { THRESHOLD_STEP } else { -THRESHOLD_STEP };
        editor.threshold.update(|threshold| *threshold = (*threshold + step).clamp(0.0, MAX_THRESHOLD));
    };
    // The brush or eraser stroke in progress, sprites of the stored masks, and where the brush
    // outline is drawn.
    let stroke = StoredValue::new_local(None::<Stroke>);
    let sprites = StoredValue::new_local(SpriteCache::default());
    let hover = RwSignal::new(None::<Point>);
    // The fuzzy fill's region before it is committed.
    let fill_preview = StoredValue::new_local(None::<FillPreview>);
    // Drops whatever is being drawn, a stroke included.
    let cancel_gesture = move || {
        stroke.set_value(None);
        fill_preview.set_value(None);
        gesture.set(Gesture::Idle);
    };
    let finish_stroke = move || {
        gesture.set(Gesture::Idle);
        let mut done = None;
        stroke.update_value(|current| done = current.take());
        let Some(edit) = done.and_then(|done| editor.annotations.with_untracked(|list| done.finish(list))) else { return };
        let edit = with_defaults(edit, &defaults);
        let count = editor.annotations.with_untracked(Vec::len);
        editor.apply(edit);
        // Joined or emptied masks were removed, so the selected indices no longer fit.
        if editor.annotations.with_untracked(Vec::len) < count {
            editor.selection.set(BTreeSet::new());
        }
    };

    // ---- Fuzzy fill: grow on the analysis tiles loaded so far, ask for those the region reached,
    // and grow again as they arrive.
    let analysis = Analysis::new(image_id);
    let pyramid = Pyramid::new(image_size.0, image_size.1);
    // Whether the last growth reached unloaded tiles, or stopped at the maximum size.
    let fill_incomplete = StoredValue::new(false);
    let fill_capped = StoredValue::new(false);
    // Bumped when the preview changes, to redraw it.
    let fill_drawn = RwSignal::new(0_u32);
    // The fill's threshold and maximum size (in pixels of a level whose pixels cover `factor` ×
    // `factor` image pixels).
    let fill_settings = move |within: Option<PixelRect>, factor: u32| {
        let image_area = f64::from(image_size.0) * f64::from(image_size.1);
        let level_area = f64::from(factor) * f64::from(factor);
        FillSettings {
            threshold: editor.threshold.get_untracked().clamp(0.0, MAX_THRESHOLD) as u8,
            within,
            max_pixels: (editor.max_fill.get_untracked() / 100.0 * image_area / level_area).max(1.0) as u64,
        }
    };
    let regrow = move || {
        let Gesture::Filling { seed, erase, .. } = gesture.get_untracked() else { return };
        let Some(current) = view.get_untracked() else { return };
        // The level being viewed now: a zoom during a fill applies from then on (ADR 0002).
        let level = pyramid.level_for_scale(current.scale);
        let factor = 1u32 << level;
        let level_area = f64::from(factor) * f64::from(factor);
        let (level_width, level_height) = pyramid.level_size(level);
        let seed = ((seed.x / f64::from(factor)) as u32, (seed.y / f64::from(factor)) as u32);
        let seed = (seed.0.min(level_width - 1), seed.1.min(level_height - 1));
        // Global fills take the visible part of the image.
        let within = (!editor.contiguous.get_untracked()).then(|| {
            let (w, h) = size.get_untracked();
            let (min, max) = (current.to_image(Point::new(0.0, 0.0)), current.to_image(Point::new(w, h)));
            let level_pixel = |v: f64| (v / f64::from(factor)).max(0.0) as u32;
            let (left, top) = (level_pixel(min.x), level_pixel(min.y));
            PixelRect { left, top, width: level_pixel(max.x).saturating_sub(left) + 1, height: level_pixel(max.y).saturating_sub(top) + 1 }
        });
        let settings = fill_settings(within, factor);
        let growth = analysis.with_level(level, |tiles| grow(pyramid, level, tiles, seed, settings));
        analysis.request(level, &growth.missing);
        fill_incomplete.set_value(!growth.missing.is_empty());
        if growth.capped && !fill_capped.get_value() {
            let max = editor.max_fill.get_untracked();
            editor.notice.set(Some(format!("The fill grows past {max:.0}% of the image and will not be added.")));
        }
        fill_capped.set_value(growth.capped);
        let min_area = (editor.clean_up.get_untracked() / level_area).round() as usize;
        let region = growth.region.cropped();
        // Clean-up drops small patches, but never the whole fill.
        let region = match region.clone().cleaned(min_area) {
            cleaned if cleaned.bounds().is_some() => cleaned,
            _ => region,
        };
        let color = if erase { "#ffffff".to_owned() } else { editor.label.get_untracked().map(color_of).unwrap_or_default() };
        fill_preview.set_value(Some(FillPreview::new(region, factor, &color)));
        fill_drawn.update(|n| *n = n.wrapping_add(1));
    };
    // Adds the previewed region to the masks (or erases it from them): one undo step.
    let commit_fill = move || {
        let Gesture::Filling { erase, .. } = gesture.get_untracked() else { return };
        gesture.set(Gesture::Idle);
        let mut preview = None;
        fill_preview.update_value(|current| preview = current.take());
        let Some(preview) = preview else { return };
        // A runaway fill is not added: it would be a cut-off part of a much larger region.
        if fill_capped.get_value() {
            let max = editor.max_fill.get_untracked();
            editor.notice.set(Some(format!("Not added: the fill grew past {max:.0}% of the image. Lower the threshold or raise Max.")));
            return;
        }
        let region = preview.region.scaled_up(preview.factor, image_size);
        let label = editor.label.get_untracked().unwrap_or_default();
        let edit = editor.annotations.with_untracked(|list| Stroke::from_region(erase, label, region, image_size, list).finish(list));
        let edit = edit.map(|edit| with_defaults(edit, &defaults));
        if let Some(edit) = edit {
            let count = editor.annotations.with_untracked(Vec::len);
            editor.apply(edit);
            if editor.annotations.with_untracked(Vec::len) < count {
                editor.selection.set(BTreeSet::new());
            }
        }
    };
    Effect::new(move |_| {
        analysis.loaded.track();
        if !matches!(gesture.get_untracked(), Gesture::Filling { .. }) {
            return;
        }
        regrow();
        if matches!(gesture.get_untracked(), Gesture::Filling { released: true, .. }) && !fill_incomplete.get_value() {
            commit_fill();
        }
    });
    // ---- Edge snapping and the edge-limited brush, on the analysis tiles of the viewed level.
    let level_of = move |current: View| {
        let level = pyramid.level_for_scale(current.scale);
        (level, f64::from(1u32 << level))
    };
    // The live-wire path (image coordinates) from `from` to the strongest edge near `to`, or just
    // that edge point without `from`; `None` while tiles it needs are loading.
    let edge_path = move |from: Option<Point>, to: Point, current: View| -> Option<Vec<Point>> {
        let (level, factor) = level_of(current);
        let to_level = |p: Point| ((p.x / factor).max(0.0) as u32, (p.y / factor).max(0.0) as u32);
        let radius = (editor.snap_radius.get_untracked() / (current.scale * factor)).ceil().max(1.0) as u32;
        let target = to_level(to);
        let anchor = from.map(to_level).unwrap_or(target);
        let margin = radius + 8;
        let (left, top) = (anchor.0.min(target.0).saturating_sub(margin), anchor.1.min(target.1).saturating_sub(margin));
        let window = PixelRect {
            left,
            top,
            width: anchor.0.max(target.0) + margin + 1 - left,
            height: anchor.1.max(target.1) + margin + 1 - top,
        };
        let map = match analysis.with_level(level, |tiles| EdgeMap::new(pyramid, level, tiles, window)) {
            Ok(map) => map,
            Err(missing) => {
                analysis.request(level, &missing);
                return None;
            }
        };
        let snapped = map.strongest_near(target, radius);
        let pixels = if from.is_some() { map.live_wire(anchor, snapped) } else { vec![snapped] };
        let centre = |v: u32| (f64::from(v) + 0.5) * factor;
        let mut path: Vec<Point> = pixels.into_iter().map(|(x, y)| Point::new(centre(x), centre(y))).collect();
        if let (Some(from), Some(first)) = (from, path.first_mut()) {
            *first = from;
        }
        Some(path)
    };
    // "Stop at edges": the pixels similar to `seed` (the fill's threshold, on the tiles loaded so
    // far), grown or shrunk by the offset, in image pixels.
    let edge_limit = move |seed: Point, level: u32| -> Bitmap {
        let factor = 1u32 << level;
        let (width, height) = pyramid.level_size(level);
        let seed = ((seed.x as u32 / factor).min(width - 1), (seed.y as u32 / factor).min(height - 1));
        let growth = analysis.with_level(level, |tiles| grow(pyramid, level, tiles, seed, fill_settings(None, factor)));
        analysis.request(level, &growth.missing);
        let region = growth.region.cropped().scaled_up(factor, image_size);
        // The offset is in image pixels, so it applies at full resolution.
        match editor.edge_offset.get_untracked().round() as i32 {
            0 => region,
            offset => region.expanded(offset, image_size),
        }
    };
    // Where the stroke started and the level its limit was grown on: it is grown again when the
    // view's level changes or tiles arrive.
    let limit_seed = StoredValue::new(None::<Point>);
    let limit_level = StoredValue::new(None::<u32>);
    let shift_held = StoredValue::new(false);
    // Applies "Stop at edges" (inverted by Shift) to the stroke, growing its limit if needed.
    let update_limit = move |shift: bool, current: View, refresh: bool| {
        let Some(seed) = limit_seed.get_value() else { return };
        shift_held.set_value(shift);
        let limited = editor.stop_at_edges.get_untracked() != shift;
        let (level, _) = level_of(current);
        let limit = (limited && (refresh || limit_level.get_value() != Some(level))).then(|| {
            limit_level.set_value(Some(level));
            edge_limit(seed, level)
        });
        stroke.update_value(|s| {
            if let Some(s) = s {
                if let Some(limit) = limit {
                    s.set_limit(limit);
                }
                s.limit(limited);
            }
        });
    };
    Effect::new(move |_| {
        analysis.loaded.track();
        if let (Gesture::Painting, Some(current)) = (gesture.get_untracked(), view.get_untracked()) {
            update_limit(shift_held.get_value(), current, true);
        }
    });
    // Brush and freehand lines load the view's analysis tiles ahead of the first stroke.
    Effect::new(move |_| {
        let wanted = match editor.tool.get() {
            Tool::Brush => true,
            Tool::Polyline => editor.freehand.get(),
            _ => false,
        };
        let (Some(current), (w, h)) = (view.get(), size.get()) else { return };
        if wanted {
            let (level, _) = level_of(current);
            let visible = Rect::from_corners(current.to_image(Point::new(0.0, 0.0)), current.to_image(Point::new(w, h)));
            analysis.request(level, &pyramid.tiles_in(level, visible));
        }
    });

    Effect::new(move |_| {
        if analysis.failed.get() {
            editor.notice.set(Some("Could not load the image's pixels for the fill.".into()));
            fill_preview.set_value(None);
            gesture.set(Gesture::Idle);
            *analysis.failed.write_untracked() = false;
        }
    });

    // ---- Pie menus. `pending_tool` waits for its label after the tool pie; `relabel` means the
    // label pie changes the label of the selected shapes.
    let pie = RwSignal::new(None::<Pie>);
    let pointer = RwSignal::new(Point::new(0.0, 0.0));
    let cursor_at = StoredValue::new(Point::new(0.0, 0.0));
    let search = RwSignal::new(None::<Point>);
    let pending_tool = StoredValue::new(None::<Tool>);
    // Whether Q or the right button that opened the pie is still down.
    let trigger_down = StoredValue::new(false);
    let relabel = StoredValue::new(false);
    // The shape types the next label must allow: those of the shapes being relabelled, of the tool
    // just picked, or of the active tool. None at all allows every label.
    let target_shapes = move || -> Vec<ShapeType> {
        if relabel.get_value() {
            let selection = editor.selection.get_untracked();
            let mut types: Vec<ShapeType> = editor.annotations.with_untracked(|list| {
                selection.iter().filter_map(|&index| list.get(index).map(|a| a.shape.shape_type())).collect()
            });
            types.sort_by_key(|t| *t as u8);
            types.dedup();
            return types;
        }
        pending_tool.get_value().unwrap_or(editor.tool.get_untracked()).shape_type().into_iter().collect()
    };
    let allows = |label: &Label, shapes: &[ShapeType]| shapes.iter().all(|shape| label.shape_types.contains(shape));

    let item = |slot, name: &str, key: Option<&str>, enabled, current, action| PieItem {
        slot,
        name: name.to_owned(),
        key: key.map(str::to_owned),
        swatch: None,
        enabled,
        current,
        action,
    };
    let tool_pie = move |centre: Point, held: bool| {
        let current = editor.tool.get_untracked();
        let tool = |slot, tool: Tool, key: &str| item(slot, tool.name(), Some(key), editor.can_use(tool), tool == current, PieAction::Tool(tool));
        // The approved layout.
        let items = vec![
            tool(Compass::N, Tool::Select, "V"),
            tool(Compass::NE, Tool::Rectangle, "R"),
            tool(Compass::E, Tool::Polygon, "P"),
            tool(Compass::SE, Tool::Points, "."),
            tool(Compass::S, Tool::Polyline, "L"),
            tool(Compass::SW, Tool::Brush, "B"),
            tool(Compass::W, Tool::Circle, "C"),
            tool(Compass::NW, Tool::Tag, "T"),
        ];
        Pie { label: "Tools", items, centre, wide: false, held, opened_at: now() }
    };
    let label_pie = move |centre: Point, held: bool| {
        let shapes = target_shapes();
        let current = editor.label.get_untracked();
        let slots = [Compass::N, Compass::NE, Compass::E, Compass::SE, Compass::S, Compass::SW, Compass::W, Compass::NW];
        let mut items: Vec<PieItem> = all_labels.with_value(|labels| {
            let shown = if labels.len() > slots.len() { slots.len() - 1 } else { slots.len() };
            labels
                .iter()
                .take(shown)
                .zip(slots)
                .map(|(label, slot)| PieItem {
                    slot,
                    name: label.name.clone(),
                    key: label.hotkey.clone(),
                    swatch: Some(label.color.clone()),
                    enabled: allows(label, &shapes),
                    current: current == Some(label.id),
                    action: PieAction::Label(label.id),
                })
                .collect()
        });
        if all_labels.with_value(|labels| labels.len() > slots.len()) {
            items.push(item(Compass::NW, "More…", Some("/"), true, false, PieAction::MoreLabels));
        }
        // Not an empty ring: say why there is nothing to pick.
        if items.is_empty() {
            items.push(item(Compass::N, "No labels yet: add them on the Labels tab", None, false, false, PieAction::MoreLabels));
        }
        Pie { label: "Labels", items, centre, wide: false, held, opened_at: now() }
    };
    let options_pie = move |centre: Point, held: bool| {
        let drawing = matches!(gesture.get_untracked(), Gesture::Path { .. });
        let (label, items) = match editor.tool.get_untracked() {
            Tool::Rectangle => {
                let oriented = editor.oriented.get_untracked();
                let items = vec![
                    item(Compass::N, "2-point", None, true, !oriented, PieAction::SetOriented(false)),
                    item(Compass::E, "3-point oriented", None, true, oriented, PieAction::SetOriented(true)),
                    item(Compass::S, "From center", Some("Alt"), !oriented, editor.from_centre.get_untracked(), PieAction::ToggleFromCentre),
                    item(Compass::W, "Square", Some("Shift"), !oriented, editor.square.get_untracked(), PieAction::ToggleSquare),
                ];
                ("Rectangle options", items)
            }
            Tool::Polygon => {
                let items = vec![
                    item(Compass::N, "Snap 15°", Some("Shift"), true, editor.snap.get_untracked(), PieAction::ToggleSnap),
                    item(Compass::E, "Finish shape", Some("Enter"), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", Some("Backspace"), drawing, false, PieAction::UndoPoint),
                ];
                ("Polygon options", items)
            }
            Tool::Polyline => {
                let freehand = editor.freehand.get_untracked();
                let snap = if freehand {
                    item(Compass::NE, "Snap to edges", Some("Shift"), true, editor.edge_snap.get_untracked(), PieAction::ToggleEdgeSnap)
                } else {
                    item(Compass::NE, "Snap 15°", Some("Shift"), true, editor.snap.get_untracked(), PieAction::ToggleSnap)
                };
                let items = vec![
                    item(Compass::N, "Click", None, true, !freehand, PieAction::SetFreehand(false)),
                    item(Compass::S, "Freehand", None, true, freehand, PieAction::SetFreehand(true)),
                    snap,
                    item(Compass::E, "Finish shape", Some("Enter"), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", Some("Backspace"), drawing, false, PieAction::UndoPoint),
                ];
                ("Polyline options", items)
            }
            Tool::Points => {
                let single = editor.single_point.get_untracked();
                let items = vec![
                    item(Compass::N, "Multiple", None, true, !single, PieAction::SetSinglePoint(false)),
                    item(Compass::S, "Single point", None, true, single, PieAction::SetSinglePoint(true)),
                    item(Compass::E, "Finish shape", Some("Enter"), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", Some("Backspace"), drawing, false, PieAction::UndoPoint),
                ];
                ("Points options", items)
            }
            Tool::Circle => {
                let diameter = editor.diameter.get_untracked();
                let items = vec![
                    item(Compass::N, "Centre and radius", None, true, !diameter, PieAction::SetDiameter(false)),
                    item(Compass::S, "Diameter", None, true, diameter, PieAction::SetDiameter(true)),
                ];
                ("Circle options", items)
            }
            Tool::Brush => {
                let (eraser, fill) = (editor.eraser.get_untracked(), editor.fill.get_untracked());
                // Both modes of each pair, the current one marked; then the mode's own options.
                let mut items = vec![
                    item(Compass::N, "Paint", None, true, !fill, PieAction::SetFill(false)),
                    item(Compass::NE, "Fuzzy fill", Some("U"), true, fill, PieAction::SetFill(true)),
                    item(Compass::S, "Brush", Some("B"), true, !eraser, PieAction::SetEraser(false)),
                    item(Compass::SE, "Eraser", Some("E"), true, eraser, PieAction::SetEraser(true)),
                ];
                if fill {
                    items.extend([
                        item(Compass::E, "Higher threshold", Some("]"), true, false, PieAction::Threshold(true)),
                        item(Compass::W, "Lower threshold", Some("["), true, false, PieAction::Threshold(false)),
                    ]);
                } else {
                    items.extend([
                        item(Compass::E, "Larger", Some("]"), true, false, PieAction::BrushSize(true)),
                        item(Compass::W, "Smaller", Some("["), true, false, PieAction::BrushSize(false)),
                        item(Compass::NW, "Stop at edges", Some("Shift"), true, editor.stop_at_edges.get_untracked(), PieAction::ToggleStopAtEdges),
                    ]);
                }
                ("Brush options", items)
            }
            Tool::Select | Tool::Tag => ("Options", Vec::new()),
        };
        // Four wide buttons, unless an option needs a diagonal slot.
        let wide = items.iter().all(|item| matches!(item.slot, Compass::N | Compass::E | Compass::S | Compass::W));
        Pie { label, items, centre, wide, held, opened_at: now() }
    };

    let close_pie = move || {
        pie.set(None);
        search.set(None);
        pending_tool.set_value(None);
        relabel.set_value(false);
    };
    let open_pie = move |held: bool, labels_only: bool| {
        close_pie();
        let centre = cursor_at.get_value();
        let selecting = editor.tool.get_untracked() == Tool::Select;
        let has_selection = selecting && !editor.selection.with_untracked(BTreeSet::is_empty);
        let opened = if labels_only || has_selection {
            relabel.set_value(has_selection);
            label_pie(centre, held)
        } else if selecting {
            tool_pie(centre, held)
        } else {
            options_pie(centre, held)
        };
        pointer.set(centre);
        pie.set(Some(opened));
    };
    let undo_point = move || match gesture.get_untracked() {
        Gesture::Path { mut points, current } if points.len() > 1 => {
            points.pop();
            gesture.set(Gesture::Path { points, current });
        }
        Gesture::Path { .. } => gesture.set(Gesture::Idle),
        _ => {}
    };
    // Adds `id` as a tag of the image, or removes it if the image has it.
    let toggle_tag = move |id: i64| {
        let existing = editor.annotations.with_untracked(|list| list.iter().position(|a| a.shape.is_tag() && a.label == id));
        match existing {
            Some(index) => editor.remove(index),
            None => {
                let index = editor.annotations.with_untracked(Vec::len);
                let annotation = Annotation { attributes: defaults(id), ..Annotation::manual(id, Shape::Tag) };
                editor.apply(Edit::Add { index, annotation });
            }
        }
    };
    let finish_label = move |id: i64| {
        let (relabelling, tool) = (relabel.get_value(), pending_tool.get_value());
        let shapes = target_shapes();
        let usable = all_labels.with_value(|labels| labels.iter().any(|label| label.id == id && allows(label, &shapes)));
        close_pie();
        if !usable {
            return;
        }
        match (relabelling, tool) {
            (true, _) => {
                // An image has each tag once: a selected tag cannot become one it already has.
                let tagged = editor.annotations.with_untracked(|list| list.iter().any(|a| a.shape.is_tag() && a.label == id));
                if tagged && editor.selection.with_untracked(|set| set.iter().any(|&i| editor.annotation(i).is_some_and(|a| a.shape.is_tag()))) {
                    editor.notice.set(Some(format!("The image already has the tag “{}”.", label_name(id))));
                }
                editor.change_selected(|annotation| {
                    if annotation.shape.is_tag() && tagged {
                        return annotation.clone();
                    }
                    Annotation { label: id, attributes: conform(id, &annotation.attributes), ..annotation.clone() }
                })
            }
            (false, Some(Tool::Tag)) => toggle_tag(id),
            (false, Some(tool)) => {
                editor.tool.set(tool);
                editor.label.set(Some(id));
            }
            (false, None) => editor.label.set(Some(id)),
        }
    };
    let choose = move |action: PieAction| {
        let centre = pie.get_untracked().map_or(cursor_at.get_value(), |open| open.centre);
        pie.set(None);
        match action {
            PieAction::Tool(Tool::Select) => {
                close_pie();
                editor.tool.set(Tool::Select);
            }
            // Picking a tool chains to the label pie, centred where the pointer is so the next
            // flick starts fresh; it stays held while Q or the right button is still down.
            PieAction::Tool(tool) => {
                pending_tool.set_value(Some(tool));
                let at = cursor_at.get_value();
                pointer.set(at);
                pie.set(Some(label_pie(at, trigger_down.get_value())));
            }
            PieAction::Label(id) => finish_label(id),
            PieAction::MoreLabels => search.set(Some(centre)),
            PieAction::ToggleSquare => {
                close_pie();
                editor.square.update(|on| *on = !*on);
            }
            PieAction::ToggleFromCentre => {
                close_pie();
                editor.from_centre.update(|on| *on = !*on);
            }
            PieAction::SetOriented(on) => {
                close_pie();
                gesture.set(Gesture::Idle);
                editor.oriented.set(on);
            }
            PieAction::ToggleSnap => {
                close_pie();
                editor.snap.update(|on| *on = !*on);
            }
            PieAction::SetFreehand(on) => {
                close_pie();
                editor.freehand.set(on);
            }
            PieAction::SetSinglePoint(on) => {
                close_pie();
                editor.single_point.set(on);
            }
            PieAction::SetDiameter(on) => {
                close_pie();
                editor.diameter.set(on);
            }
            PieAction::SetEraser(on) => {
                close_pie();
                editor.eraser.set(on);
            }
            PieAction::SetFill(on) => {
                close_pie();
                editor.fill.set(on);
            }
            PieAction::ToggleEdgeSnap => {
                close_pie();
                editor.edge_snap.update(|on| *on = !*on);
            }
            PieAction::ToggleStopAtEdges => {
                close_pie();
                editor.stop_at_edges.update(|on| *on = !*on);
            }
            PieAction::Threshold(higher) => {
                close_pie();
                change_threshold(higher);
            }
            PieAction::BrushSize(larger) => {
                close_pie();
                resize_brush(larger);
            }
            PieAction::FinishShape => {
                close_pie();
                finish_path();
            }
            PieAction::UndoPoint => {
                close_pie();
                undo_point();
            }
        }
    };
    // Releasing the pie key or button: a tap repeats the last tool, label and modes, from Select
    // or a drawing tool (a label pie, or no last tool, stays open for clicking); otherwise the
    // highlighted item is picked.
    let release = move || {
        let Some(open) = pie.get_untracked().filter(|open| open.held) else { return };
        let at = pointer.get_untracked();
        if is_tap(now() - open.opened_at, at.x - open.centre.x, at.y - open.centre.y) {
            match editor.last.get_untracked() {
                Some((tool, label, modes)) if open.label != "Labels" => {
                    close_pie();
                    editor.restore(tool, label, modes);
                }
                _ => pie.update(|open| {
                    if let Some(open) = open {
                        open.held = false;
                    }
                }),
            }
            return;
        }
        match open.highlighted(at).map(|item| item.action.clone()) {
            Some(action) => choose(action),
            None => close_pie(),
        }
    };
    let search_entries = move || {
        let shapes = target_shapes();
        all_labels.with_value(|labels| {
            labels
                .iter()
                .map(|label| SearchEntry {
                    id: label.id,
                    name: label.name.clone(),
                    color: label.color.clone(),
                    key: label.hotkey.clone(),
                    enabled: allows(label, &shapes),
                })
                .collect::<Vec<_>>()
        })
    };
    // Remember the last drawing tool, label and modes for a tap of the pie key.
    Effect::new(move |_| {
        let (tool, modes) = (editor.tool.get(), editor.modes());
        if let (true, Some(label)) = (tool != Tool::Select, editor.label.get()) {
            editor.last.set(Some((tool, label, modes)));
        }
    });
    // Without labels nothing can be drawn: say so when the image opens.
    if all_labels.with_value(Vec::is_empty) {
        editor.notice.set(Some("This project has no labels yet: create them on the Labels tab to start annotating.".into()));
    }
    // A drawing tool needs a label that allows its shape: switch to the first one that does.
    Effect::new(move |_| {
        let Some(shape) = editor.tool.get().shape_type() else { return };
        if editor.label.get_untracked().is_some_and(|label| label_allows(label, shape)) {
            return;
        }
        let first = all_labels.with_value(|labels| labels.iter().find(|label| label.shape_types.contains(&shape)).map(|l| l.id));
        match first {
            Some(id) => editor.label.set(Some(id)),
            None => editor.notice.set(Some(format!(
                "No label allows {} shapes; allow it on the Labels tab first.",
                editor.tool.get_untracked().name().to_lowercase()
            ))),
        }
    });
    let pointer_moves = window_event_listener(ev::pointermove, move |ev| {
        let at = Point::new(f64::from(ev.client_x()), f64::from(ev.client_y()));
        cursor_at.set_value(at);
        let Some(open) = pie.get_untracked() else { return };
        pointer.set(at);
        // Flick: moving far enough picks at once, whether the pie is held or open for clicking.
        if (at.x - open.centre.x).hypot(at.y - open.centre.y) >= FLICK_DISTANCE
            && let Some(action) = open.highlighted(at).map(|item| item.action.clone())
        {
            choose(action);
        }
    });
    let pointer_ups = window_event_listener(ev::pointerup, move |ev| {
        if ev.button() == 2 {
            trigger_down.set_value(false);
            release();
        }
    });
    let key_ups = window_event_listener(ev::keyup, move |ev| {
        if ev.key().eq_ignore_ascii_case("q") {
            trigger_down.set_value(false);
            release();
        }
    });
    // A click in an open pie picks the item in that direction; in the centre it closes the pie.
    let background_click = move || match pie
        .get_untracked()
        .and_then(|open| open.highlighted(pointer.get_untracked()).map(|item| item.action.clone()))
    {
        Some(action) => choose(action),
        None => close_pie(),
    };
    on_cleanup(move || {
        pointer_moves.remove();
        pointer_ups.remove();
        key_ups.remove();
    });

    // ---- Pointer handling on the canvas.
    let select_press = move |ev: &ev::PointerEvent, point: Point, current: View| {
        let (extend, subtract) = (ev.shift_key(), ev.ctrl_key() || ev.meta_key());
        if !extend && let Some((index, grip)) = grip_at(point, current.scale) {
            let Some(shape) = editor.annotation(index).map(|a| a.shape) else { return };
            match grip {
                // Ctrl+click on a vertex removes it, if the shape keeps enough.
                Grip::Vertex(vertex) if subtract => match shape.without_vertex(vertex) {
                    Some(smaller) => editor.reshape(index, smaller),
                    None => editor.notice.set(Some("That shape cannot lose another point.".into())),
                },
                _ if subtract => {}
                Grip::Resize(handle, rect) => gesture.set(Gesture::Resizing { index, handle, before: rect, current: rect }),
                Grip::Vertex(vertex) => gesture.set(Gesture::Vertex { index, vertex, current: shape }),
                // Dragging an edge's "+" inserts a vertex there and drags it.
                Grip::Midpoint(edge) => {
                    if let Some(grown) = shape.with_inserted_vertex(edge + 1, point) {
                        gesture.set(Gesture::Vertex { index, vertex: edge + 1, current: grown });
                    }
                }
                Grip::Rotate(rect) => gesture.set(Gesture::Rotating { index, before: rect, current: rect }),
                Grip::Radius(centre) => {
                    let radius = match shape {
                        Shape::Circle { radius, .. } => radius,
                        _ => return,
                    };
                    gesture.set(Gesture::Radius { index, centre, radius });
                }
            }
            return;
        }
        let hit = editor.annotations.with_untracked(|list| topmost_at(list, point, PICK_MARGIN / current.scale));
        match hit {
            // Shift+click toggles a shape, Ctrl+click removes it from the selection.
            Some(index) if extend => editor.selection.update(|set| {
                if !set.remove(&index) {
                    set.insert(index);
                }
            }),
            Some(index) if subtract => editor.selection.update(|set| {
                set.remove(&index);
            }),
            // Dragging a selected shape moves the whole selection.
            Some(index) => {
                if !editor.selection.with_untracked(|set| set.contains(&index)) {
                    editor.selection.set(BTreeSet::from([index]));
                }
                gesture.set(Gesture::Moving { grab: point, dx: 0.0, dy: 0.0 });
            }
            // On empty space a drag draws a selection box.
            None => {
                let mode = if extend {
                    BoxMode::Extend
                } else if subtract {
                    BoxMode::Subtract
                } else {
                    editor.selection.set(BTreeSet::new());
                    BoxMode::Replace
                };
                gesture.set(Gesture::Selecting { start: point, current: point, mode });
            }
        }
    };
    let draw_press = move |ev: &ev::PointerEvent, tool: Tool, point: Point, current: View| {
        // Ctrl inverts a brush stroke: the brush erases, the eraser paints.
        let erasing = tool == Tool::Brush && editor.eraser.get_untracked() != (ev.ctrl_key() || ev.meta_key());
        if editor.label.get_untracked().is_none() && !erasing {
            let shape = tool.name().to_lowercase();
            editor.notice.set(Some(format!("Add a label that allows {shape} shapes on the Labels tab first.")));
            return;
        }
        let point = inside(point);
        match (tool, gesture.get_untracked()) {
            (Tool::Rectangle, Gesture::Drawing { start, square, from_centre, .. }) => {
                finish_rectangle(start, point, square, from_centre, current.scale)
            }
            (Tool::Rectangle, Gesture::Oriented { mut points, .. }) => {
                let point = snapped(points.last().copied(), point, ev.shift_key());
                points.push(point);
                if let [a, b, c] = points[..] {
                    add_shape(Shape::OrientedRectangle(OrientedRect::from_three_points(a, b, c)));
                } else {
                    gesture.set(Gesture::Oriented { points, current: point });
                }
            }
            (Tool::Rectangle, _) if editor.oriented.get_untracked() => {
                gesture.set(Gesture::Oriented { points: vec![point], current: point });
            }
            (Tool::Rectangle, _) => gesture.set(Gesture::Drawing {
                start: point,
                current: point,
                dragged: false,
                square: ev.shift_key() != editor.square.get_untracked(),
                from_centre: ev.alt_key() != editor.from_centre.get_untracked(),
            }),
            (Tool::Circle, Gesture::Circle { start, .. }) => finish_circle(start, point, current.scale),
            (Tool::Circle, _) => gesture.set(Gesture::Circle { start: point, current: point, dragged: false }),
            (Tool::Brush, _) if !erasing && !label_allows(editor.label.get_untracked().unwrap_or_default(), ShapeType::Mask) => {
                let label = label_name(editor.label.get_untracked().unwrap_or_default());
                editor.notice.set(Some(format!("“{label}” cannot be used for masks.")));
            }
            (Tool::Brush, _) if editor.fill.get_untracked() => {
                let anchor = Point::new(f64::from(ev.client_x()), f64::from(ev.client_y()));
                let base = editor.threshold.get_untracked();
                gesture.set(Gesture::Filling { seed: point, anchor, base, erase: erasing, released: false });
                regrow();
            }
            (Tool::Brush, _) => {
                let label = editor.label.get_untracked().unwrap_or_default();
                let size = editor.brush_size.get_untracked();
                stroke.set_value(Some(Stroke::start(erasing, label, size, image_size, point, color_of(label))));
                limit_seed.set_value(Some(point));
                limit_level.set_value(None);
                update_limit(ev.shift_key(), current, false);
                stroke.update_value(|s| {
                    if let Some(s) = s {
                        editor.annotations.with_untracked(|list| s.extend(point, list, &color_of));
                    }
                });
                gesture.set(Gesture::Painting);
            }
            (Tool::Polyline, _) if editor.freehand.get_untracked() => {
                let snapping = editor.edge_snap.get_untracked() != ev.shift_key();
                let start = snapping.then(|| edge_path(None, point, current)).flatten().and_then(|path| path.first().copied()).unwrap_or(point);
                gesture.set(Gesture::Stroke { points: vec![start], current: start });
            }
            (Tool::Points, _) if editor.single_point.get_untracked() => add_shape(Shape::Points { points: vec![point] }),
            (_, Gesture::Path { mut points, .. }) => {
                // Clicking a polygon's first vertex closes it.
                let closes = tool == Tool::Polygon
                    && points.len() >= 3
                    && current.to_screen(points[0]).distance(current.to_screen(point)) <= HANDLE_REACH;
                if closes {
                    finish_path();
                } else {
                    let next = if tool == Tool::Points { point } else { snapped(points.last().copied(), point, ev.shift_key()) };
                    points.push(next);
                    gesture.set(Gesture::Path { points, current: next });
                }
            }
            _ => gesture.set(Gesture::Path { points: vec![point], current: point }),
        }
    };
    let on_pointer_down = move |ev: ev::PointerEvent| {
        // While sizing the brush, a click confirms the size and the right button cancels.
        if let Gesture::BrushSizing { before, .. } = gesture.get_untracked() {
            ev.prevent_default();
            if ev.button() == 2 {
                editor.brush_size.set(before);
            }
            gesture.set(Gesture::Idle);
            return;
        }
        // The right button opens the pie, like holding Q.
        if ev.button() == 2 {
            ev.prevent_default();
            cursor_at.set_value(Point::new(f64::from(ev.client_x()), f64::from(ev.client_y())));
            trigger_down.set_value(true);
            open_pie(true, false);
            return;
        }
        // Middle button and Space+drag belong to the viewer (pan); let them bubble up.
        if ev.button() != 0 || space_held.get_untracked() {
            return;
        }
        let Some((_, point, current)) = locate(&ev) else { return };
        if let Some(canvas) = canvas_ref.get_untracked() {
            canvas.set_pointer_capture(ev.pointer_id()).ok();
        }
        match editor.tool.get_untracked() {
            Tool::Select => select_press(&ev, point, current),
            tool => draw_press(&ev, tool, point, current),
        }
    };

    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((screen, point, current)) = locate(&ev) else { return };
        if editor.tool.get_untracked() == Tool::Brush {
            hover.set(Some(point));
        }
        match gesture.get_untracked() {
            Gesture::Drawing { start, dragged, .. } => {
                let moved = current.to_screen(start).distance(screen) > DRAG_THRESHOLD;
                gesture.set(Gesture::Drawing {
                    start,
                    current: inside(point),
                    dragged: dragged || (moved && ev.buttons() & 1 == 1),
                    square: ev.shift_key() != editor.square.get_untracked(),
                    from_centre: ev.alt_key() != editor.from_centre.get_untracked(),
                });
            }
            Gesture::Circle { start, dragged, .. } => {
                let moved = current.to_screen(start).distance(screen) > DRAG_THRESHOLD;
                gesture.set(Gesture::Circle { start, current: inside(point), dragged: dragged || (moved && ev.buttons() & 1 == 1) });
            }
            Gesture::Stroke { mut points, .. } => {
                let to = inside(point);
                let last = *points.last().expect("a stroke starts with a point");
                let spacing = editor.spacing.get_untracked();
                // Snapping follows the edge path from the last vertex; until its tiles are
                // loaded the line follows the pointer.
                let snapping = editor.edge_snap.get_untracked() != ev.shift_key();
                let current = match snapping.then(|| edge_path(Some(last), to, current)).flatten() {
                    Some(path) => {
                        points.extend(steps_along(&path, spacing));
                        *path.last().expect("a path has its ends")
                    }
                    None => {
                        points.extend(steps_towards(last, to, spacing));
                        to
                    }
                };
                gesture.set(Gesture::Stroke { points, current });
            }
            Gesture::BrushSizing { centre, before } => {
                let size = before + 2.0 * (point.x - centre.x);
                editor.brush_size.set(size.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE));
            }
            Gesture::Filling { anchor, base, released: false, .. } => {
                let moved = (f64::from(ev.client_x()) - anchor.x) + (f64::from(ev.client_y()) - anchor.y);
                editor.threshold.set((base + moved * THRESHOLD_PER_PIXEL).round().clamp(0.0, MAX_THRESHOLD));
                regrow();
            }
            // Released and waiting for tiles: the pointer no longer changes it.
            Gesture::Filling { released: true, .. } => {}
            Gesture::Painting => {
                let to = inside(point);
                update_limit(ev.shift_key(), current, false);
                stroke.update_value(|current| {
                    if let Some(current) = current {
                        editor.annotations.with_untracked(|list| current.extend(to, list, &color_of));
                    }
                });
                gesture.set(Gesture::Painting);
            }
            Gesture::Radius { index, centre, .. } => {
                let room = centre.x.min(centre.y).min(width - centre.x).min(height - centre.y);
                gesture.set(Gesture::Radius { index, centre, radius: centre.distance(point).min(room) });
            }
            Gesture::Oriented { points, .. } => {
                let current = snapped(points.last().copied(), inside(point), ev.shift_key() && points.len() == 1);
                gesture.set(Gesture::Oriented { points, current });
            }
            Gesture::Path { points, .. } => {
                let current = if editor.tool.get_untracked() == Tool::Points {
                    inside(point)
                } else {
                    snapped(points.last().copied(), inside(point), ev.shift_key())
                };
                gesture.set(Gesture::Path { points, current });
            }
            Gesture::Moving { grab, .. } => {
                let Some(bounds) = selection_bounds() else { return };
                let moved = bounds.moved_within(point.x - grab.x, point.y - grab.y, width, height);
                gesture.set(Gesture::Moving { grab, dx: moved.min.x - bounds.min.x, dy: moved.min.y - bounds.min.y });
            }
            Gesture::Resizing { index, handle, before, .. } => {
                gesture.set(Gesture::Resizing { index, handle, before, current: before.resized(handle, inside(point)) });
            }
            Gesture::Vertex { index, vertex, current: shape } => {
                gesture.set(Gesture::Vertex { index, vertex, current: shape.with_vertex(vertex, inside(point)) });
            }
            Gesture::Rotating { index, before, .. } => {
                let mut angle = before.angle_towards(point);
                if ev.shift_key() != editor.snap.get_untracked() {
                    let step = SNAP_DEGREES.to_radians();
                    angle = (angle / step).round() * step;
                }
                gesture.set(Gesture::Rotating { index, before, current: before.rotated_to(angle) });
            }
            Gesture::Selecting { start, mode, .. } => gesture.set(Gesture::Selecting { start, current: point, mode }),
            Gesture::Idle => cursor.set(match (editor.tool.get_untracked(), grip_at(point, current.scale)) {
                (Tool::Select, Some((_, Grip::Resize(handle, _)))) => handle.cursor(),
                (Tool::Select, Some((_, Grip::Rotate(_)))) => "grab",
                (Tool::Select, Some((_, Grip::Radius(_)))) => "ew-resize",
                (Tool::Select, Some(_)) => "move",
                _ => "default",
            }),
        }
    };

    let on_pointer_up = move |ev: ev::PointerEvent| {
        let Some((_, _, current)) = locate(&ev) else { return };
        match gesture.get_untracked() {
            // A click without dragging leaves the rectangle open for a second click.
            Gesture::Drawing { start, current: end, dragged: true, square, from_centre } => {
                finish_rectangle(start, end, square, from_centre, current.scale)
            }
            // A click without dragging leaves the circle open for a second click.
            Gesture::Circle { start, current: end, dragged: true } => finish_circle(start, end, current.scale),
            // Releasing ends a freehand stroke; a stroke too short for two vertices keeps its end.
            Gesture::Stroke { mut points, current: end } => {
                if points.len() < 2 && points[0].distance(end) * current.scale >= MIN_SHAPE_SIZE {
                    points.push(end);
                }
                if points.len() < 2 {
                    gesture.set(Gesture::Idle);
                } else {
                    gesture.set(Gesture::Stroke { points, current: end });
                    finish_path();
                }
            }
            Gesture::Painting => finish_stroke(),
            // Committed now, or once the tiles it still needs have arrived.
            Gesture::Filling { seed, anchor, base, erase, released: false } => {
                gesture.set(Gesture::Filling { seed, anchor, base, erase, released: true });
                if !fill_incomplete.get_value() {
                    commit_fill();
                }
            }
            Gesture::Radius { index, centre, radius } => {
                gesture.set(Gesture::Idle);
                if radius * current.scale >= MIN_SHAPE_SIZE / 2.0 {
                    editor.reshape(index, Shape::Circle { centre, radius });
                }
            }
            Gesture::Moving { dx, dy, .. } => {
                gesture.set(Gesture::Idle);
                if dx != 0.0 || dy != 0.0 {
                    let locked = editor.selection.with_untracked(|set| set.iter().any(|&i| editor.annotation(i).is_some_and(|a| a.locked)));
                    if locked {
                        editor.notice.set(Some("Locked shapes stay where they are: unlock them in the objects list first.".into()));
                    }
                    editor.change_selected(|annotation| {
                        if annotation.locked {
                            return annotation.clone();
                        }
                        Annotation { shape: annotation.shape.translated(dx, dy), ..annotation.clone() }
                    });
                }
            }
            Gesture::Resizing { index, before, current: after, .. } => {
                gesture.set(Gesture::Idle);
                let big_enough = after.width() * current.scale >= MIN_SHAPE_SIZE && after.height() * current.scale >= MIN_SHAPE_SIZE;
                if after != before && big_enough {
                    editor.reshape(index, Shape::Rectangle(after));
                }
            }
            Gesture::Vertex { index, current: shape, .. } => {
                gesture.set(Gesture::Idle);
                match shape.validate(image_size.0, image_size.1) {
                    Ok(()) => editor.reshape(index, shape),
                    Err(reason) => editor.notice.set(Some(format!("Not changed: {reason}."))),
                }
            }
            Gesture::Rotating { index, current: rect, .. } => {
                gesture.set(Gesture::Idle);
                let shape = Shape::OrientedRectangle(rect);
                match shape.validate(image_size.0, image_size.1) {
                    Ok(()) => editor.reshape(index, shape),
                    Err(reason) => editor.notice.set(Some(format!("Not changed: {reason}."))),
                }
            }
            Gesture::Selecting { start, current: end, mode } => {
                gesture.set(Gesture::Idle);
                let region = Rect::from_corners(start, end);
                // A click without a drag only cleared the selection (or did nothing with Shift/Ctrl).
                if region.width() * current.scale < DRAG_THRESHOLD && region.height() * current.scale < DRAG_THRESHOLD {
                    return;
                }
                let hits = editor.annotations.with_untracked(|list| overlapping(list, region));
                editor.selection.update(|set| match mode {
                    BoxMode::Replace | BoxMode::Extend => set.extend(hits),
                    BoxMode::Subtract => hits.iter().for_each(|hit| {
                        set.remove(hit);
                    }),
                });
            }
            _ => {}
        }
    };

    let keys = window_event_listener(ev::keydown, move |ev| {
        if typing_in_a_field() || document().query_selector(".backdrop").ok().flatten().is_some() {
            return;
        }
        let key = ev.key();
        let command = ev.ctrl_key() || ev.meta_key();
        // While a pie is open its keys pick its items; Esc closes it.
        if let Some(open) = pie.get_untracked() {
            match key.as_str() {
                "Escape" => close_pie(),
                "q" | "Q" => {}
                _ => match open.item_for_key(&key).map(|item| item.action.clone()) {
                    Some(action) => choose(action),
                    None => return,
                },
            }
            ev.prevent_default();
            return;
        }
        if let Gesture::BrushSizing { before, .. } = gesture.get_untracked() {
            match key.as_str() {
                "f" | "F" | "Enter" => gesture.set(Gesture::Idle),
                "Escape" => {
                    editor.brush_size.set(before);
                    gesture.set(Gesture::Idle);
                }
                _ => {}
            }
            ev.prevent_default();
            return;
        }
        if key.eq_ignore_ascii_case("q") && !command && !ev.alt_key() {
            if !ev.repeat() {
                trigger_down.set_value(true);
                // Shift+Q opens only the label pie; releasing Q over a label picks it too.
                open_pie(true, ev.shift_key());
            }
            ev.prevent_default();
            return;
        }
        let drawing_path = matches!(gesture.get_untracked(), Gesture::Path { .. });
        // A stroke keeps the indices of the masks it changes: no undo or redo under it.
        let painting = gesture.get_untracked() == Gesture::Painting;
        match key.as_str() {
            "z" | "Z" | "y" | "Y" if command && painting => {}
            "z" | "Z" if command && ev.shift_key() => editor.redo(),
            "z" | "Z" if command => editor.undo(),
            "y" | "Y" if command => editor.redo(),
            // Blender: A selects everything, Alt+A nothing.
            "a" | "A" if ev.alt_key() => editor.selection.set(BTreeSet::new()),
            // Everything visible: hidden shapes are not picked.
            "a" | "A" if !command => {
                let visible = editor.annotations.with_untracked(|list| list.iter().enumerate().filter(|(_, a)| !a.hidden).map(|(i, _)| i).collect());
                editor.selection.set(visible);
            }
            _ if command || ev.alt_key() => return,
            "Enter" if drawing_path => finish_path(),
            "[" | "]" if editor.tool.get_untracked() == Tool::Brush && editor.fill.get_untracked() => change_threshold(key == "]"),
            "[" | "]" if editor.tool.get_untracked() == Tool::Brush => resize_brush(key == "]"),
            // F: resize the outline where it is, by moving the pointer right or left.
            "f" | "F" if editor.tool.get_untracked() == Tool::Brush && !editor.fill.get_untracked() && gesture.get_untracked() == Gesture::Idle => {
                let Some(centre) = hover.get_untracked() else { return };
                gesture.set(Gesture::BrushSizing { centre, before: editor.brush_size.get_untracked() });
            }
            "[" | "]" if editor.tool.get_untracked() == Tool::Polyline => {
                let factor = if key == "]" { SIZE_STEP } else { 1.0 / SIZE_STEP };
                editor.spacing.update(|spacing| *spacing = (*spacing * factor).clamp(MIN_SPACING, MAX_SPACING));
                editor.notice.set(Some(format!("Freehand spacing {:.0} px", editor.spacing.get_untracked())));
            }
            "Backspace" if drawing_path => undo_point(),
            "Escape" => {
                if gesture.get_untracked() != Gesture::Idle {
                    cancel_gesture();
                } else if !editor.selection.with_untracked(BTreeSet::is_empty) {
                    editor.selection.set(BTreeSet::new());
                } else {
                    editor.tool.set(Tool::Select);
                }
            }
            "Delete" | "Backspace" | "x" | "X" => editor.delete_selected(),
            // E is the brush in eraser mode, B in paint mode (Blender has no eraser tool either).
            "e" | "E" | "u" | "U" | "b" | "B" | "t" | "T" if !editor.can_use(if key.eq_ignore_ascii_case("t") { Tool::Tag } else { Tool::Brush }) => {
                let tool = if key.eq_ignore_ascii_case("t") { Tool::Tag } else { Tool::Brush };
                editor.notice.set(Some(Editor::unusable_notice(tool)));
            }
            "e" | "E" => {
                cancel_gesture();
                editor.tool.set(Tool::Brush);
                editor.eraser.set(true);
            }
            // T: the label pie, whose label is toggled as a tag of the image.
            "t" | "T" => {
                close_pie();
                pending_tool.set_value(Some(Tool::Tag));
                let at = cursor_at.get_value();
                pointer.set(at);
                pie.set(Some(label_pie(at, false)));
            }
            // U: the brush's fuzzy fill (GIMP's fuzzy select key).
            "u" | "U" => {
                cancel_gesture();
                editor.tool.set(Tool::Brush);
                editor.fill.set(true);
                editor.eraser.set(false);
            }
            _ => {
                if let Some(tool) = Tool::for_key(&key).filter(|tool| !editor.can_use(*tool)) {
                    editor.notice.set(Some(Editor::unusable_notice(tool)));
                } else if let Some(tool) = Tool::for_key(&key) {
                    cancel_gesture();
                    editor.tool.set(tool);
                    if tool == Tool::Brush {
                        editor.eraser.set(false);
                        editor.fill.set(false);
                    }
                } else if let Some((_, id)) = hotkeys.iter().find(|(hotkey, _)| hotkey.eq_ignore_ascii_case(&key)) {
                    match editor.tool.get_untracked().shape_type() {
                        Some(shape) if !label_allows(*id, shape) => editor.notice.set(Some(format!(
                            "“{}” cannot be used for {} shapes.",
                            label_name(*id),
                            editor.tool.get_untracked().name().to_lowercase()
                        ))),
                        _ => editor.label.set(Some(*id)),
                    }
                } else {
                    return;
                }
            }
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let draw = move || {
        let (Some(canvas), Some(current)) = (canvas_ref.get_untracked(), view.get_untracked()) else { return };
        let ratio = window().device_pixel_ratio();
        let (css_width, css_height) = size.get_untracked();
        let (pixel_width, pixel_height) = ((css_width * ratio).round() as u32, (css_height * ratio).round() as u32);
        if canvas.width() != pixel_width || canvas.height() != pixel_height {
            canvas.set_width(pixel_width);
            canvas.set_height(pixel_height);
        }
        let Some(context) = context_2d(&canvas) else { return };
        context.set_transform(ratio, 0.0, 0.0, ratio, 0.0, 0.0).ok();
        context.clear_rect(0.0, 0.0, css_width, css_height);

        let gesture = gesture.get_untracked();
        let selection = editor.selection.get_untracked();
        let single = editor.single_selection();
        editor.annotations.with_untracked(|list| {
            for (index, annotation) in list.iter().enumerate() {
                // Tags are chips over the image, not drawn on it; hidden shapes are not drawn.
                if annotation.shape.is_tag() || annotation.hidden {
                    continue;
                }
                let selected = selection.contains(&index);
                // Shapes being edited show their new form.
                let shape = match &gesture {
                    Gesture::Resizing { index: i, current, .. } if *i == index => Shape::Rectangle(*current),
                    Gesture::Vertex { index: i, current, .. } if *i == index => current.clone(),
                    Gesture::Rotating { index: i, current, .. } if *i == index => Shape::OrientedRectangle(*current),
                    Gesture::Radius { index: i, centre, radius } if *i == index => Shape::Circle { centre: *centre, radius: *radius },
                    Gesture::Moving { dx, dy, .. } if selected && !annotation.locked => annotation.shape.translated(*dx, *dy),
                    _ => annotation.shape.clone(),
                };
                let color = color_of(annotation.label);
                if let Shape::Mask(mask) = &shape {
                    // Masks the stroke changes are drawn by the stroke.
                    let shown = stroke.with_value(|s| s.as_ref().is_some_and(|s| s.draw_changed(&context, current, index, selected)));
                    if !shown {
                        sprites.update_value(|cache| cache.draw(&context, current, mask, &color, selected));
                    }
                    if selected {
                        draw::mask_frame(&context, current, shape.bounds(), &color);
                        let name = names.get(&annotation.label).cloned().unwrap_or_default();
                        draw::tag(&context, current.to_screen(shape.bounds().min), &name, &color);
                    }
                    continue;
                }
                draw::shape(&context, current, &shape, Look { color: &color, selected, dashed: false });
                if selected {
                    let name = names.get(&annotation.label).cloned().unwrap_or_default();
                    draw::tag(&context, current.to_screen(shape.bounds().min), &name, &color);
                }
                // Handles only when one shape is selected: they edit that shape.
                if single == Some(index) {
                    match &shape {
                        Shape::Rectangle(rect) => draw::rectangle_handles(&context, current, *rect, &color),
                        Shape::OrientedRectangle(rect) => {
                            draw::vertex_handles(&context, current, &shape, &color, false);
                            draw::rotation_handle(&context, current, *rect, &color);
                        }
                        Shape::Points { .. } => draw::vertex_handles(&context, current, &shape, &color, false),
                        Shape::Circle { centre, radius } => {
                            draw::vertex_handles(&context, current, &shape, &color, false);
                            draw::radius_handle(&context, current, *centre, *radius, &color);
                        }
                        Shape::Polygon { .. } | Shape::Polyline { .. } => {
                            draw::vertex_handles(&context, current, &shape, &color, true)
                        }
                        Shape::Mask(_) | Shape::Tag => {}
                    }
                }
            }
        });
        sprites.update_value(SpriteCache::sweep);
        stroke.with_value(|s| s.as_ref().map(|s| s.draw_painted(&context, current)));
        fill_preview.with_value(|preview| preview.as_ref().map(|preview| preview.draw(&context, current)));
        let brush_radius = editor.brush_size.get_untracked() / 2.0;
        if let Gesture::BrushSizing { centre, .. } = gesture {
            draw::brush_outline(&context, current, centre, brush_radius);
            let label = format!("{:.0} px", 2.0 * brush_radius);
            draw::tag(&context, current.to_screen(centre.offset(-brush_radius, -brush_radius)), &label, "#e8e6e1");
        } else if let (Tool::Brush, false, Some(at)) = (editor.tool.get_untracked(), editor.fill.get_untracked(), hover.get_untracked()) {
            draw::brush_outline(&context, current, at, brush_radius);
        }
        let pen = editor.label.get_untracked().map(color_of).unwrap_or_else(|| "#ffffff".to_owned());
        match gesture {
            Gesture::Selecting { start, current: end, .. } => draw::selection_box(&context, current, Rect::from_corners(start, end)),
            Gesture::Drawing { start, current: end, square, from_centre, .. } => {
                let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
                draw::shape(&context, current, &Shape::Rectangle(rect), Look { color: &pen, selected: false, dashed: true });
                draw::tag(&context, current.to_screen(rect.min), &format!("{:.0} × {:.0}", rect.width(), rect.height()), &pen);
            }
            Gesture::Oriented { points, current: next } => match points[..] {
                [a] => draw::path_preview(&context, current, &[a], next, false, false, &pen),
                [a, b] => draw::oriented_preview(&context, current, OrientedRect::from_three_points(a, b, next), &pen),
                _ => {}
            },
            Gesture::Path { points, current: next } => {
                let tool = editor.tool.get_untracked();
                draw::path_preview(&context, current, &points, next, tool == Tool::Polygon, tool == Tool::Points, &pen);
            }
            Gesture::Stroke { points, current: next } => draw::path_preview(&context, current, &points, next, false, false, &pen),
            Gesture::Circle { start, current: end, .. } => {
                let circle = circle_from(start, end);
                draw::shape(&context, current, &circle, Look { color: &pen, selected: false, dashed: true });
                if let Shape::Circle { radius, .. } = circle {
                    draw::tag(&context, current.to_screen(circle.bounds().min), &format!("r {radius:.0}"), &pen);
                }
            }
            _ => {}
        }
    };
    Effect::new(move |_| {
        view.track();
        size.track();
        gesture.track();
        editor.annotations.track();
        editor.selection.track();
        editor.label.track();
        editor.tool.track();
        editor.brush_size.track();
        editor.fill.track();
        fill_drawn.track();
        hover.track();
        draw();
    });
    Effect::new(move |_| editor.busy.set(gesture.with(|g| *g != Gesture::Idle)));
    // Drop a half-drawn shape when the tool or a drawing mode changes.
    Effect::new(move |_| {
        editor.tool.track();
        editor.oriented.track();
        editor.freehand.track();
        editor.single_point.track();
        editor.eraser.track();
        editor.fill.track();
        cancel_gesture();
    });

    let canvas_cursor = move || match editor.tool.get() {
        Tool::Select => cursor.get(),
        _ => "crosshair",
    };
    view! {
        <PieMenu pie=pie pointer=pointer on_pick=choose on_background=background_click />
        <div class="tag-chips" aria-label="Image tags">
            {move || editor.annotations.with(|list| {
                list.iter().enumerate().filter(|(_, a)| a.shape.is_tag()).map(|(index, a)| {
                    let name = label_name(a.label);
                    let selected = move || editor.selection.with(|set| set.contains(&index));
                    view! {
                        <span class="tag-chip" class:selected=selected style:border-color=color_of(a.label)>
                            <span class="swatch" style:background=color_of(a.label)></span>
                            <button class="name" title="Select this tag" on:click=move |_| editor.selection.set(BTreeSet::from([index]))>{name.clone()}</button>
                            <button class="remove" title="Remove this tag" aria-label=format!("Remove tag {name}") on:click=move |_| editor.remove(index)>"×"</button>
                        </span>
                    }
                }).collect_view()
            })}
        </div>
        {move || search.get().map(|at| view! {
            <LabelSearch at=at entries=search_entries() on_pick=finish_label on_close=close_pie />
        })}
        <canvas
            class="annotations"
            node_ref=canvas_ref
            // Editor state for browser tests; not used for rendering.
            data-count=move || editor.annotations.with(Vec::len)
            data-selected={move || editor.selection.get().iter().map(usize::to_string).collect::<Vec<_>>().join(",")}
            data-revision=move || editor.revision.get()
            data-drawing={move || match gesture.get() {
                Gesture::Path { points, .. } | Gesture::Oriented { points, .. } | Gesture::Stroke { points, .. } => points.len(),
                Gesture::Painting | Gesture::Filling { .. } => 1,
                _ => 0,
            }}
            style:cursor=canvas_cursor
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:pointerleave=move |_| hover.set(None)
            on:dblclick=move |_| finish_path()
            on:contextmenu=|ev: ev::MouseEvent| ev.prevent_default()
        ></canvas>
    }
}

/// Index of the point nearest to `to` within `reach`, if any.
fn nearest(points: &[Point], to: Point, reach: f64) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| (index, point.distance(to)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// `edit` with new annotations given their label's default attribute values.
fn with_defaults(edit: Edit, defaults: &dyn Fn(i64) -> Attributes) -> Edit {
    match edit {
        Edit::Add { index, mut annotation } => {
            if annotation.attributes.is_empty() {
                annotation.attributes = defaults(annotation.label);
            }
            Edit::Add { index, annotation }
        }
        Edit::Batch(edits) => Edit::Batch(edits.into_iter().map(|edit| with_defaults(edit, defaults)).collect()),
        other => other,
    }
}
