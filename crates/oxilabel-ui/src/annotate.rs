//! Drawing and editing annotations on top of the tile viewer. The image's annotations, undo
//! history, selection and the Select tool's gestures live in the editor session
//! (`oxilabel_core::editor`); this layer is its adapter: browser events in, signals and canvas
//! drawing out.
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
//! Ellipse (C) draws from its centre or across its box by a drag or two clicks; Shift makes a
//! circle and Alt switches between centre and box, as for rectangles. Brush (B) paints
//! masks and its eraser mode (E) erases them (see `masks`); Ctrl inverts a stroke, and `[` and
//! `]` or F (then move the pointer right or left and click; Esc cancels) change the size. Its fuzzy fill mode
//! (U) fills the similar region around a click (`fill`), a drag changing the threshold. Freehand
//! polylines can snap to edges (live-wire, `edges`) and the brush can stop at edges (painting
//! only the similar region around the stroke's start); Shift inverts either while drawing. Esc
//! first cancels the shape being drawn, then clears the selection, then returns to Select. Ctrl+Z
//! and Ctrl+Y undo and redo. Ctrl+C copies the selection and Ctrl+X cuts it; Ctrl+V pastes the
//! copies on any image of the project at the same place, selected (see `oxilabel_core::clipboard`).
//!
//! Holding Q or the right mouse button opens a pie at the cursor: with Select active the tool pie
//! (then the label pie), with a drawing tool its options pie, with a shape selected the label pie
//! to relabel it. Release or flick picks; a quick tap repeats the last tool and label. Shift+Q
//! opens only the label pie. Tag (T, or the tool pie) opens the label pie and toggles that label
//! as a tag of the whole image; tags show as chips over the image.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
};

use leptos::{ev, html, prelude::*};
use serde::{Deserialize, Serialize};
use oxilabel_core::{
    Footprint, Label, ShapeType,
    annotation::{Annotation, Shape},
    attribute::Attributes,
    clipboard,
    constraint::ShapeConstraints,
    edges::EdgeMap,
    editor::{DRAG_THRESHOLD, HANDLE_REACH, MIN_SHAPE_SIZE, Modifiers, Options, Pointer, SNAP_DEGREES, Session, grip::GripKind, snap_turn},
    ellipse::Ellipse,
    fill::{FillSettings, grow},
    geometry::{Point, Rect, snap_angle, steps_along, steps_towards},
    mask::{Bitmap, PixelRect},
    pyramid::Pyramid,
    history::Edit,
    oriented::OrientedRect,
    keymap::Action,
    pie::{Compass, is_tap},
    settings::LabelStyle,
    stroke::{MaskStroke, StrokeOutcome},
    view::View,
};

use crate::{
    draw::{self, Look},
    analysis::Analysis,
    masks::{FillPreview, SpriteCache, Stroke},
    pie::{LabelSearch, Pie, PieAction, PieItem, PieMenu, SearchEntry, now},
    prefs::{prefs, press},
    side_panel,
    viewer::{context_2d, typing_in_a_field},
};

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

thread_local! {
    /// The copied annotations and their project: kept while moving between the project's images,
    /// not across a reload.
    static CLIPBOARD: RefCell<Option<(i64, Vec<Annotation>)>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Select,
    Rectangle,
    Polygon,
    Polyline,
    Points,
    Ellipse,
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

/// The tools a key selects directly (Tag opens its label pie instead).
const KEY_TOOLS: [Tool; 7] = [Tool::Select, Tool::Rectangle, Tool::Polygon, Tool::Polyline, Tool::Points, Tool::Ellipse, Tool::Brush];

/// The editor of the open image: its session (annotations, undo history, selection, the Select
/// tool's gesture) and the tool state around it. Readers track the session through the getters
/// below; every change goes through `update`, which notifies what changed.
#[derive(Clone, Copy)]
pub struct Editor {
    session: StoredValue<Session>,
    /// Notified when the annotations, the selection or the pre-annotation threshold change;
    /// `changed` after every update (the gesture included).
    annotations_changed: Trigger,
    selection_changed: Trigger,
    threshold_changed: Trigger,
    changed: Trigger,
    pub tool: RwSignal<Tool>,
    /// The label new shapes get.
    pub label: RwSignal<Option<i64>>,
    /// A short hint for the user, e.g. why a shape could not be drawn.
    pub notice: RwSignal<Option<String>>,
    /// The drawing tools' options and the last tool, kept while moving between images.
    pub tools: Tools,
    /// The enhancement filter shown (its key; `None` for the original), set by the view filter.
    pub view_filter: RwSignal<Option<String>>,
    /// The shape types some label of the project allows; tools for others are disabled.
    pub usable_shapes: RwSignal<Vec<ShapeType>>,
    /// Whether a shape, stroke or other gesture is in progress (moving to another image waits).
    pub busy: RwSignal<bool>,
}

/// The drawing tools' options and the last tool. They belong to the workspace rather than to one
/// image's editor, so they stay the same when another image is opened, and are kept in session
/// storage per project, so leaving the editor or reloading the page keeps them too.
#[derive(Clone, Copy)]
pub struct Tools {
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
    /// Ellipse: drawn from its centre out (true) or across its bounding box (false); Alt inverts
    /// it while drawing.
    pub ellipse_from_centre: RwSignal<bool>,
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
    /// Whether edge snapping, the edge-limited brush and the fuzzy fill analyse the filtered view
    /// instead of the original (ADR 0002).
    pub snap_filtered: RwSignal<bool>,
    pub edges_filtered: RwSignal<bool>,
    pub fill_filtered: RwSignal<bool>,
    /// The last drawing tool, label and modes, repeated by a tap of the pie key.
    pub last: RwSignal<Option<(Tool, i64, Modes)>>,
}

/// What `Tools` keeps in session storage: options by name, and the last tool.
#[derive(Default, Serialize, Deserialize)]
struct SavedTools {
    switches: HashMap<String, bool>,
    numbers: HashMap<String, f64>,
    last: Option<(Tool, i64, Modes)>,
}

impl Tools {
    /// The options and last tool of `project` as last left in this browser tab, or the defaults.
    pub fn for_project(project: Option<i64>) -> Self {
        let tools = Tools::new();
        let Some(project) = project else { return tools };
        let key = format!("oxilabel.tools.{project}");
        let storage = || window().session_storage().ok().flatten();
        let saved: SavedTools = storage()
            .and_then(|storage| storage.get_item(&key).ok().flatten())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        for (name, signal) in tools.switches() {
            if let Some(value) = saved.switches.get(name) {
                signal.set(*value);
            }
        }
        for (name, signal) in tools.numbers() {
            if let Some(value) = saved.numbers.get(name).filter(|value| value.is_finite()) {
                signal.set(*value);
            }
        }
        tools.last.set(saved.last);
        Effect::new(move |_| {
            let saved = SavedTools {
                switches: tools.switches().into_iter().map(|(name, signal)| (name.to_owned(), signal.get())).collect(),
                numbers: tools.numbers().into_iter().map(|(name, signal)| (name.to_owned(), signal.get())).collect(),
                last: tools.last.get(),
            };
            if let (Some(storage), Ok(text)) = (storage(), serde_json::to_string(&saved)) {
                storage.set_item(&key, &text).ok();
            }
        });
        tools
    }

    fn switches(&self) -> [(&'static str, RwSignal<bool>); 15] {
        [
            ("square", self.square),
            ("from_centre", self.from_centre),
            ("oriented", self.oriented),
            ("snap", self.snap),
            ("freehand", self.freehand),
            ("single_point", self.single_point),
            ("ellipse_from_centre", self.ellipse_from_centre),
            ("eraser", self.eraser),
            ("fill", self.fill),
            ("contiguous", self.contiguous),
            ("edge_snap", self.edge_snap),
            ("stop_at_edges", self.stop_at_edges),
            ("snap_filtered", self.snap_filtered),
            ("edges_filtered", self.edges_filtered),
            ("fill_filtered", self.fill_filtered),
        ]
    }

    fn numbers(&self) -> [(&'static str, RwSignal<f64>); 7] {
        [
            ("spacing", self.spacing),
            ("brush_size", self.brush_size),
            ("threshold", self.threshold),
            ("clean_up", self.clean_up),
            ("max_fill", self.max_fill),
            ("snap_radius", self.snap_radius),
            ("edge_offset", self.edge_offset),
        ]
    }

    fn new() -> Self {
        Tools {
            square: RwSignal::new(false),
            from_centre: RwSignal::new(false),
            oriented: RwSignal::new(false),
            snap: RwSignal::new(false),
            freehand: RwSignal::new(false),
            spacing: RwSignal::new(DEFAULT_SPACING),
            single_point: RwSignal::new(false),
            ellipse_from_centre: RwSignal::new(true),
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
            snap_filtered: RwSignal::new(false),
            edges_filtered: RwSignal::new(false),
            fill_filtered: RwSignal::new(false),
            last: RwSignal::new(None),
        }
    }
}

/// The tools' drawing modes, remembered with the last tool.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Modes {
    oriented: bool,
    freehand: bool,
    single_point: bool,
    ellipse_from_centre: bool,
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
        [self.tools.oriented, self.tools.freehand, self.tools.single_point, self.tools.ellipse_from_centre, self.tools.fill, self.tools.eraser]
    }

    /// The current modes (tracked).
    pub fn modes(&self) -> Modes {
        let [oriented, freehand, single_point, ellipse_from_centre, fill, eraser] = self.mode_signals().map(|mode| mode.get());
        Modes { oriented, freehand, single_point, ellipse_from_centre, fill, eraser }
    }

    /// Switches to `tool`, `label` and `modes`, touching only what differs, so a shape being
    /// drawn with them is kept.
    fn restore(&self, tool: Tool, label: i64, modes: Modes) {
        let Modes { oriented, freehand, single_point, ellipse_from_centre, fill, eraser } = modes;
        for (signal, value) in self.mode_signals().into_iter().zip([oriented, freehand, single_point, ellipse_from_centre, fill, eraser]) {
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

    pub fn new(session: Session, label: Option<i64>, tools: Tools) -> Self {
        Editor {
            session: StoredValue::new(session),
            annotations_changed: Trigger::new(),
            selection_changed: Trigger::new(),
            threshold_changed: Trigger::new(),
            changed: Trigger::new(),
            tool: RwSignal::new(Tool::Select),
            label: RwSignal::new(label),
            notice: RwSignal::new(None),
            tools,
            view_filter: RwSignal::new(None),
            usable_shapes: RwSignal::new(ShapeType::ALL.to_vec()),
            busy: RwSignal::new(false),
        }
    }

    /// The pixels a tool analyses: the shown filter's when the tool's `filtered` switch is on.
    pub fn analysed(&self, filtered: RwSignal<bool>) -> Option<String> {
        if filtered.get_untracked() { self.view_filter.get_untracked() } else { None }
    }

    /// Reads the session without tracking it.
    pub fn peek<T>(&self, read: impl FnOnce(&Session) -> T) -> T {
        self.session.with_value(read)
    }

    /// Changes the session, then notifies whatever changed and shows its notice.
    pub fn update<T>(&self, change: impl FnOnce(&mut Session) -> T) -> T {
        let (revision, selection, threshold) = self.peek(|s| (s.revision(), s.selection().clone(), s.threshold()));
        let mut notice = None;
        let result = self
            .session
            .try_update_value(|session| {
                let result = change(session);
                notice = session.take_notice();
                result
            })
            .expect("the editor's session lives as long as the editor");
        let (changed_list, changed_selection, changed_threshold) =
            self.peek(|s| (s.revision() != revision, *s.selection() != selection, s.threshold() != threshold));
        if changed_list {
            self.annotations_changed.notify();
        }
        if changed_selection {
            self.selection_changed.notify();
        }
        if changed_threshold {
            self.threshold_changed.notify();
        }
        self.changed.notify();
        if notice.is_some() {
            self.notice.set(notice);
        }
        result
    }

    /// Reads the annotations (tracked).
    pub fn with_annotations<T>(&self, read: impl FnOnce(&[Annotation]) -> T) -> T {
        self.annotations_changed.track();
        self.peek(|s| read(s.annotations()))
    }

    /// The selected indices (tracked).
    pub fn selection(&self) -> BTreeSet<usize> {
        self.selection_changed.track();
        self.peek(|s| s.selection().clone())
    }

    /// Whether annotation `index` is selected (tracked).
    pub fn is_selected(&self, index: usize) -> bool {
        self.selection_changed.track();
        self.peek(|s| s.selection().contains(&index))
    }

    /// Increases with every change to the annotations (tracked); autosave compares it with the
    /// last saved one.
    pub fn revision(&self) -> u64 {
        self.annotations_changed.track();
        self.peek(Session::revision)
    }

    /// Pending pre-annotations less confident than this are neither drawn nor picked (tracked).
    pub fn threshold(&self) -> f64 {
        self.threshold_changed.track();
        self.peek(Session::threshold)
    }

    /// The pending pre-annotations not hidden, shown and below the threshold (tracked).
    pub fn pending(&self) -> (Vec<usize>, Vec<usize>) {
        self.annotations_changed.track();
        self.threshold_changed.track();
        self.peek(Session::pending)
    }

    /// Tracks every change to the session, its gesture included.
    pub fn track_changes(&self) {
        self.changed.track();
    }
}

/// What the pointer is doing right now with a drawing tool (the Select tool's gestures are the
/// session's).
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
    /// An ellipse from `start` (its centre, or a corner of its box); like a rectangle, it is
    /// finished by releasing a drag or by a second click.
    Ellipse { start: Point, current: Point, dragged: bool, circle: bool, from_centre: bool },
    /// A fixed-size shape centred on `centre`, its angle following the pointer until a second
    /// click sets it.
    Placing { centre: Point, angle: f64 },
    /// A brush or eraser stroke; its pixels are in the `Stroke` kept beside the gesture.
    Painting,
    /// F with the brush: the outline stays at `centre` (where the pointer was) and the size
    /// changes by twice the pointer's horizontal move: right larger, left smaller. `before` is
    /// restored on cancel.
    BrushSizing { centre: Point, before: f64 },
    /// A fuzzy fill from `seed`, on the tiles of the viewed level; dragging from `anchor` changes
    /// the threshold from `base`. Once `released`, it is committed as soon as growing is complete.
    Filling { seed: Point, anchor: Point, base: f64, erase: bool, released: bool },
}

#[component]
pub fn AnnotationLayer(
    editor: Editor,
    project: i64,
    labels: Vec<Label>,
    image_id: i64,
    image_size: (u32, u32),
    view: RwSignal<Option<View>>,
    size: RwSignal<(f64, f64)>,
    space_held: RwSignal<bool>,
) -> impl IntoView {
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let prefs = prefs();
    let keymap = prefs.keymap;
    // A key's label in the user's keymap, for the pies.
    let key_of = move |action: Action| Some(keymap.with_untracked(|keys| keys.label(action))).filter(|label| !label.is_empty());
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
    let constraints_of = move |id: i64| all_labels.with_value(|labels| labels.iter().find(|l| l.id == id).map(|l| l.constraints).unwrap_or_default());
    // The footprints the editor shows, by label (labels that show theirs).
    let footprints: StoredValue<HashMap<i64, Footprint>> =
        StoredValue::new(labels.iter().filter(|label| label.show_footprint).map(|label| (label.id, label.footprint)).collect());
    // The outlines of the footprints drawn last, by annotation index, with the shape and footprint
    // they were made from: a region is only made again when either changes.
    let ghosts: StoredValue<HashMap<usize, (Shape, Footprint, Vec<Vec<Point>>)>> = StoredValue::new(HashMap::new());
    // The shape type the current tool draws and the current label's constraints, when the label
    // fixes that type's size: the tool then places shapes instead of dragging them out.
    let fixed_tool = move || -> Option<(ShapeType, ShapeConstraints)> {
        let shape = match editor.tool.get_untracked() {
            Tool::Rectangle if editor.tools.oriented.get_untracked() => ShapeType::OrientedRectangle,
            Tool::Rectangle => ShapeType::Rectangle,
            Tool::Ellipse => ShapeType::Ellipse,
            _ => return None,
        };
        let constraints = constraints_of(editor.label.get_untracked()?);
        constraints.fixes(shape).then_some((shape, constraints))
    };

    // Pointer position in canvas pixels and in image coordinates.
    let locate = move |ev: &ev::MouseEvent| -> Option<(Point, Point, View)> {
        let canvas = canvas_ref.get_untracked()?;
        let current = view.get_untracked()?;
        let rect = canvas.get_bounding_client_rect();
        let screen = Point::new(f64::from(ev.client_x()) - rect.left(), f64::from(ev.client_y()) - rect.top());
        Some((screen, current.to_image(screen), current))
    };
    // A pointer event for the session, at `point` in the image.
    let pointer_at = move |ev: &ev::MouseEvent, point: Point, current: View| Pointer {
        at: point,
        scale: current.scale,
        modifiers: Modifiers { shift: ev.shift_key(), ctrl: ev.ctrl_key() || ev.meta_key(), alt: ev.alt_key() },
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
        let annotation = Annotation { attributes: defaults(label), ..Annotation::manual(label, shape) };
        editor.update(|session| session.apply(Edit::Add { index: session.annotations().len(), annotation }));
    };
    let finish_rectangle = move |start: Point, end: Point, square: bool, from_centre: bool, scale: f64| {
        let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
        if rect.width() * scale < MIN_SHAPE_SIZE || rect.height() * scale < MIN_SHAPE_SIZE {
            gesture.set(Gesture::Idle);
            return;
        }
        add_shape(Shape::Rectangle(rect));
    };
    // The ellipse filling the box a drag spans (as for rectangles), shrunk around its centre
    // to stay inside the image.
    let ellipse_from = move |start: Point, end: Point, circle: bool, from_centre: bool| {
        let rect = Rect::from_drag(start, end, circle, from_centre);
        let ellipse = Ellipse::in_box(rect.min, rect.max);
        // Exactly round: the box's halves can differ in the last bit.
        let ellipse = if circle { Ellipse { ry: ellipse.rx, ..ellipse } } else { ellipse };
        ellipse.fitted(image_size.0, image_size.1)
    };
    let finish_ellipse = move |start: Point, end: Point, circle: bool, from_centre: bool, scale: f64| {
        let ellipse = ellipse_from(start, end, circle, from_centre);
        if ellipse.rx.min(ellipse.ry) * scale >= MIN_SHAPE_SIZE / 2.0 {
            add_shape(Shape::Ellipse(ellipse));
        } else {
            gesture.set(Gesture::Idle);
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
        Some(from) if shift != editor.tools.snap.get_untracked() => inside(snap_angle(from, to, SNAP_DEGREES)),
        _ => to,
    };

    // `[` and `]`: the brush size in steps of 25%, but at least one pixel.
    let resize_brush = move |larger: bool| {
        editor.tools.brush_size.update(|size| {
            let next = if larger { (*size * SIZE_STEP).max(*size + 1.0) } else { (*size / SIZE_STEP).min(*size - 1.0) };
            *size = next.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE);
        });
    };
    let change_threshold = move |higher: bool| {
        let step = if higher { THRESHOLD_STEP } else { -THRESHOLD_STEP };
        editor.tools.threshold.update(|threshold| *threshold = (*threshold + step).clamp(0.0, MAX_THRESHOLD));
    };
    // The brush or eraser stroke in progress, sprites of the stored masks, and where the brush
    // outline is drawn.
    let stroke = StoredValue::new_local(None::<Stroke>);
    let sprites = StoredValue::new_local(SpriteCache::default());
    let hover = RwSignal::new(None::<Point>);
    // The fuzzy fill's region before it is committed.
    let fill_preview = StoredValue::new_local(None::<FillPreview>);
    // Drops whatever is being drawn, a stroke included, and the session's gesture.
    let cancel_gesture = move || {
        stroke.set_value(None);
        fill_preview.set_value(None);
        gesture.set(Gesture::Idle);
        if editor.peek(Session::is_busy) {
            editor.update(Session::cancel);
        }
    };
    // Applies a finished stroke and drops the annotations it removed from the selection.
    let commit_stroke = move |outcome: StrokeOutcome| {
        editor.update(|session| {
            let selection = outcome.selection_after(session.selection());
            session.set_selection(selection);
            session.apply(with_defaults(outcome.edit, &defaults));
        });
    };
    let finish_stroke = move || {
        gesture.set(Gesture::Idle);
        let mut done = None;
        stroke.update_value(|current| done = current.take());
        let Some(outcome) = done.and_then(|done| editor.peek(|session| done.core.finish(session.annotations()))) else { return };
        commit_stroke(outcome);
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
            threshold: editor.tools.threshold.get_untracked().clamp(0.0, MAX_THRESHOLD) as u8,
            within,
            max_pixels: (editor.tools.max_fill.get_untracked() / 100.0 * image_area / level_area).max(1.0) as u64,
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
        let within = (!editor.tools.contiguous.get_untracked()).then(|| {
            let (w, h) = size.get_untracked();
            let (min, max) = (current.to_image(Point::new(0.0, 0.0)), current.to_image(Point::new(w, h)));
            let level_pixel = |v: f64| (v / f64::from(factor)).max(0.0) as u32;
            let (left, top) = (level_pixel(min.x), level_pixel(min.y));
            PixelRect { left, top, width: level_pixel(max.x).saturating_sub(left) + 1, height: level_pixel(max.y).saturating_sub(top) + 1 }
        });
        let settings = fill_settings(within, factor);
        let source = editor.analysed(editor.tools.fill_filtered);
        let growth = analysis.with_level(&source, level, |tiles| grow(pyramid, level, tiles, seed, settings));
        analysis.request(&source, level, &growth.missing);
        fill_incomplete.set_value(!growth.missing.is_empty());
        if growth.capped && !fill_capped.get_value() {
            let max = editor.tools.max_fill.get_untracked();
            editor.notice.set(Some(format!("The fill grows past {max:.0}% of the image and will not be added.")));
        }
        fill_capped.set_value(growth.capped);
        let min_area = (editor.tools.clean_up.get_untracked() / level_area).round() as usize;
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
            let max = editor.tools.max_fill.get_untracked();
            editor.notice.set(Some(format!("Not added: the fill grew past {max:.0}% of the image. Lower the threshold or raise Max.")));
            return;
        }
        let region = preview.region.scaled_up(preview.factor, image_size);
        let label = editor.label.get_untracked().unwrap_or_default();
        let outcome = editor.peek(|session| {
            let list = session.annotations();
            MaskStroke::from_region(erase, label, region, image_size, list).finish(list)
        });
        if let Some(outcome) = outcome {
            commit_stroke(outcome);
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
        let radius = (editor.tools.snap_radius.get_untracked() / (current.scale * factor)).ceil().max(1.0) as u32;
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
        let source = editor.analysed(editor.tools.snap_filtered);
        let map = match analysis.with_level(&source, level, |tiles| EdgeMap::new(pyramid, level, tiles, window)) {
            Ok(map) => map,
            Err(missing) => {
                analysis.request(&source, level, &missing);
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
        let source = editor.analysed(editor.tools.edges_filtered);
        let growth = analysis.with_level(&source, level, |tiles| grow(pyramid, level, tiles, seed, fill_settings(None, factor)));
        analysis.request(&source, level, &growth.missing);
        let region = growth.region.cropped().scaled_up(factor, image_size);
        // The offset is in image pixels, so it applies at full resolution.
        match editor.tools.edge_offset.get_untracked().round() as i32 {
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
        let limited = editor.tools.stop_at_edges.get_untracked() != shift;
        let (level, _) = level_of(current);
        let limit = (limited && (refresh || limit_level.get_value() != Some(level))).then(|| {
            limit_level.set_value(Some(level));
            edge_limit(seed, level)
        });
        stroke.update_value(|s| {
            if let Some(s) = s {
                if let Some(limit) = limit {
                    s.core.set_limit(limit);
                }
                s.core.limit(limited);
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
        let switch = match editor.tool.get() {
            Tool::Brush if editor.tools.fill.get() => Some(editor.tools.fill_filtered),
            Tool::Brush => Some(editor.tools.edges_filtered),
            Tool::Polyline if editor.tools.freehand.get() => Some(editor.tools.snap_filtered),
            _ => None,
        };
        let (Some(current), (w, h)) = (view.get(), size.get()) else { return };
        if let Some(switch) = switch {
            let source = if switch.get() { editor.view_filter.get() } else { None };
            let (level, _) = level_of(current);
            let visible = Rect::from_corners(current.to_image(Point::new(0.0, 0.0)), current.to_image(Point::new(w, h)));
            analysis.request(&source, level, &pyramid.tiles_in(level, visible));
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
            let mut types: Vec<ShapeType> = editor.peek(|session| {
                let list = session.annotations();
                session.selection().iter().filter_map(|&index| list.get(index).map(|a| a.shape.shape_type())).collect()
            });
            types.sort_by_key(|t| *t as u8);
            types.dedup();
            return types;
        }
        pending_tool.get_value().unwrap_or(editor.tool.get_untracked()).shape_type().into_iter().collect()
    };
    let allows = |label: &Label, shapes: &[ShapeType]| shapes.iter().all(|shape| label.shape_types.contains(shape));

    let item = |slot, name: &str, key: Option<String>, enabled, current, action| PieItem {
        slot,
        name: name.to_owned(),
        key,
        swatch: None,
        enabled,
        current,
        action,
    };
    let tool_pie = move |centre: Point, held: bool| {
        let current = editor.tool.get_untracked();
        let tool = |slot, tool: Tool| item(slot, tool.name(), key_of(tool.action()), editor.can_use(tool), tool == current, PieAction::Tool(tool));
        // The approved layout.
        let items = vec![
            tool(Compass::N, Tool::Select),
            tool(Compass::NE, Tool::Rectangle),
            tool(Compass::E, Tool::Polygon),
            tool(Compass::SE, Tool::Points),
            tool(Compass::S, Tool::Polyline),
            tool(Compass::SW, Tool::Brush),
            tool(Compass::W, Tool::Ellipse),
            tool(Compass::NW, Tool::Tag),
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
            items.push(item(Compass::NW, "More…", Some("/".to_owned()), true, false, PieAction::MoreLabels));
        }
        // Not an empty ring: say why there is nothing to pick.
        if items.is_empty() {
            items.push(item(Compass::N, "No labels yet: add them on the Labels tab", None, false, false, PieAction::MoreLabels));
        }
        Pie { label: "Labels", items, centre, wide: false, held, opened_at: now() }
    };
    let options_pie = move |centre: Point, held: bool| {
        let drawing = matches!(gesture.get_untracked(), Gesture::Path { .. });
        let (label, mut items) = match editor.tool.get_untracked() {
            Tool::Rectangle => {
                let oriented = editor.tools.oriented.get_untracked();
                let items = vec![
                    item(Compass::N, "2-point", None, true, !oriented, PieAction::SetOriented(false)),
                    item(Compass::E, "3-point oriented", None, true, oriented, PieAction::SetOriented(true)),
                    item(Compass::S, "From center", Some("Alt".to_owned()), !oriented, editor.tools.from_centre.get_untracked(), PieAction::ToggleFromCentre),
                    item(Compass::W, "Square", Some("Shift".to_owned()), !oriented, editor.tools.square.get_untracked(), PieAction::ToggleSquare),
                ];
                ("Rectangle options", items)
            }
            Tool::Polygon => {
                let items = vec![
                    item(Compass::N, "Snap 15°", Some("Shift".to_owned()), true, editor.tools.snap.get_untracked(), PieAction::ToggleSnap),
                    item(Compass::E, "Finish shape", key_of(Action::Finish), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", key_of(Action::RemoveLastPoint), drawing, false, PieAction::UndoPoint),
                ];
                ("Polygon options", items)
            }
            Tool::Polyline => {
                let freehand = editor.tools.freehand.get_untracked();
                let snap = if freehand {
                    item(Compass::NE, "Snap to edges", Some("Shift".to_owned()), true, editor.tools.edge_snap.get_untracked(), PieAction::ToggleEdgeSnap)
                } else {
                    item(Compass::NE, "Snap 15°", Some("Shift".to_owned()), true, editor.tools.snap.get_untracked(), PieAction::ToggleSnap)
                };
                let items = vec![
                    item(Compass::N, "Click", None, true, !freehand, PieAction::SetFreehand(false)),
                    item(Compass::S, "Freehand", None, true, freehand, PieAction::SetFreehand(true)),
                    snap,
                    item(Compass::E, "Finish shape", key_of(Action::Finish), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", key_of(Action::RemoveLastPoint), drawing, false, PieAction::UndoPoint),
                ];
                ("Polyline options", items)
            }
            Tool::Points => {
                let single = editor.tools.single_point.get_untracked();
                let items = vec![
                    item(Compass::N, "Multiple", None, true, !single, PieAction::SetSinglePoint(false)),
                    item(Compass::S, "Single point", None, true, single, PieAction::SetSinglePoint(true)),
                    item(Compass::E, "Finish shape", key_of(Action::Finish), drawing, false, PieAction::FinishShape),
                    item(Compass::W, "Remove last point", key_of(Action::RemoveLastPoint), drawing, false, PieAction::UndoPoint),
                ];
                ("Points options", items)
            }
            Tool::Ellipse => {
                let from_centre = editor.tools.ellipse_from_centre.get_untracked();
                let items = vec![
                    item(Compass::N, "From center", Some("Alt".to_owned()), true, from_centre, PieAction::SetEllipseFromCentre(true)),
                    item(Compass::S, "Across the box", None, true, !from_centre, PieAction::SetEllipseFromCentre(false)),
                ];
                ("Ellipse options", items)
            }
            Tool::Brush => {
                let (eraser, fill) = (editor.tools.eraser.get_untracked(), editor.tools.fill.get_untracked());
                // Both modes of each pair, the current one marked; then the mode's own options.
                let mut items = vec![
                    item(Compass::N, "Paint", None, true, !fill, PieAction::SetFill(false)),
                    item(Compass::NE, "Fuzzy fill", key_of(Action::FuzzyFill), true, fill, PieAction::SetFill(true)),
                    item(Compass::S, "Brush", key_of(Action::Brush), true, !eraser, PieAction::SetEraser(false)),
                    item(Compass::SE, "Eraser", key_of(Action::Eraser), true, eraser, PieAction::SetEraser(true)),
                ];
                if fill {
                    items.extend([
                        item(Compass::E, "Higher threshold", key_of(Action::Larger), true, false, PieAction::Threshold(true)),
                        item(Compass::W, "Lower threshold", key_of(Action::Smaller), true, false, PieAction::Threshold(false)),
                    ]);
                } else {
                    items.extend([
                        item(Compass::E, "Larger", key_of(Action::Larger), true, false, PieAction::BrushSize(true)),
                        item(Compass::W, "Smaller", key_of(Action::Smaller), true, false, PieAction::BrushSize(false)),
                        item(Compass::NW, "Stop at edges", Some("Shift".to_owned()), true, editor.tools.stop_at_edges.get_untracked(), PieAction::ToggleStopAtEdges),
                    ]);
                }
                ("Brush options", items)
            }
            Tool::Select | Tool::Tag => ("Options", Vec::new()),
        };
        // The "label slice" style: the labels are a flick away from the tool's options.
        if prefs.settings.with_untracked(|s| s.label_style) == LabelStyle::OptionsRing && !items.is_empty() {
            let free = [Compass::SW, Compass::NE, Compass::SE, Compass::NW, Compass::E, Compass::W, Compass::S, Compass::N]
                .into_iter()
                .find(|slot| items.iter().all(|item| item.slot != *slot));
            if let Some(slot) = free {
                items.push(item(slot, "Label…", key_of(Action::LabelPie), true, false, PieAction::Labels));
            }
        }
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
        let has_selection = selecting && !editor.peek(|session| session.selection().is_empty());
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
        let existing = editor.peek(|session| session.annotations().iter().position(|a| a.shape.is_tag() && a.label == id));
        match existing {
            Some(index) => editor.update(|session| session.remove(index)),
            None => {
                let annotation = Annotation { attributes: defaults(id), ..Annotation::manual(id, Shape::Tag) };
                editor.update(|session| session.apply(Edit::Add { index: session.annotations().len(), annotation }));
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
                let (tagged, tags_selected) = editor.peek(|session| {
                    let list = session.annotations();
                    let tagged = list.iter().any(|a| a.shape.is_tag() && a.label == id);
                    (tagged, session.selection().iter().any(|&i| list.get(i).is_some_and(|a| a.shape.is_tag())))
                });
                if tagged && tags_selected {
                    editor.notice.set(Some(format!("The image already has the tag “{}”.", label_name(id))));
                }
                editor.update(|session| {
                    session.change_selected(|annotation| {
                        if annotation.shape.is_tag() && tagged {
                            return annotation.clone();
                        }
                        Annotation { label: id, attributes: conform(id, &annotation.attributes), ..annotation.clone() }
                    })
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
            PieAction::Labels => {
                let at = cursor_at.get_value();
                pointer.set(at);
                pie.set(Some(label_pie(at, trigger_down.get_value())));
            }
            PieAction::ToggleSquare => {
                close_pie();
                editor.tools.square.update(|on| *on = !*on);
            }
            PieAction::ToggleFromCentre => {
                close_pie();
                editor.tools.from_centre.update(|on| *on = !*on);
            }
            PieAction::SetOriented(on) => {
                close_pie();
                gesture.set(Gesture::Idle);
                editor.tools.oriented.set(on);
            }
            PieAction::ToggleSnap => {
                close_pie();
                editor.tools.snap.update(|on| *on = !*on);
            }
            PieAction::SetFreehand(on) => {
                close_pie();
                editor.tools.freehand.set(on);
            }
            PieAction::SetSinglePoint(on) => {
                close_pie();
                editor.tools.single_point.set(on);
            }
            PieAction::SetEllipseFromCentre(on) => {
                close_pie();
                editor.tools.ellipse_from_centre.set(on);
            }
            PieAction::SetEraser(on) => {
                close_pie();
                editor.tools.eraser.set(on);
            }
            PieAction::SetFill(on) => {
                close_pie();
                editor.tools.fill.set(on);
            }
            PieAction::ToggleEdgeSnap => {
                close_pie();
                editor.tools.edge_snap.update(|on| *on = !*on);
            }
            PieAction::ToggleStopAtEdges => {
                close_pie();
                editor.tools.stop_at_edges.update(|on| *on = !*on);
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
            match editor.tools.last.get_untracked() {
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
            editor.tools.last.set(Some((tool, label, modes)));
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
        if (at.x - open.centre.x).hypot(at.y - open.centre.y) >= prefs.settings.with_untracked(|s| s.flick_distance)
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
        let keys = keymap.get_untracked();
        if keys.is_key(Action::Pie, &ev.key()) || keys.is_key(Action::LabelPie, &ev.key()) {
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
    let draw_press = move |ev: &ev::PointerEvent, tool: Tool, point: Point, current: View| {
        // Ctrl inverts a brush stroke: the brush erases, the eraser paints.
        let erasing = tool == Tool::Brush && editor.tools.eraser.get_untracked() != (ev.ctrl_key() || ev.meta_key());
        if editor.label.get_untracked().is_none() && !erasing {
            let shape = tool.name().to_lowercase();
            editor.notice.set(Some(format!("Add a label that allows {shape} shapes on the Labels tab first.")));
            return;
        }
        let point = inside(point);
        // A label with a fixed size places its shape: one click, and one more for its angle.
        if let Some((shape_type, constraints)) = fixed_tool() {
            let (centre, angle) = match gesture.get_untracked() {
                Gesture::Placing { centre, angle } => (centre, angle),
                _ if constraints.needs_angle(shape_type) => return gesture.set(Gesture::Placing { centre: point, angle: 0.0 }),
                _ => (point, 0.0),
            };
            match constraints.place(shape_type, centre, angle, image_size.0, image_size.1) {
                Some(shape) => add_shape(shape),
                None => {
                    gesture.set(Gesture::Idle);
                    editor.notice.set(Some(format!("“{}” has a fixed size that does not fit this image.", label_name(editor.label.get_untracked().unwrap_or_default()))));
                }
            }
            return;
        }
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
            (Tool::Rectangle, _) if editor.tools.oriented.get_untracked() => {
                gesture.set(Gesture::Oriented { points: vec![point], current: point });
            }
            (Tool::Rectangle, _) => gesture.set(Gesture::Drawing {
                start: point,
                current: point,
                dragged: false,
                square: ev.shift_key() != editor.tools.square.get_untracked(),
                from_centre: ev.alt_key() != editor.tools.from_centre.get_untracked(),
            }),
            (Tool::Ellipse, Gesture::Ellipse { start, circle, from_centre, .. }) => finish_ellipse(start, point, circle, from_centre, current.scale),
            (Tool::Ellipse, _) => gesture.set(Gesture::Ellipse {
                start: point,
                current: point,
                dragged: false,
                circle: ev.shift_key(),
                from_centre: ev.alt_key() != editor.tools.ellipse_from_centre.get_untracked(),
            }),
            (Tool::Brush, _) if !erasing && !label_allows(editor.label.get_untracked().unwrap_or_default(), ShapeType::Mask) => {
                let label = label_name(editor.label.get_untracked().unwrap_or_default());
                editor.notice.set(Some(format!("“{label}” cannot be used for masks.")));
            }
            (Tool::Brush, _) if editor.tools.fill.get_untracked() => {
                let anchor = Point::new(f64::from(ev.client_x()), f64::from(ev.client_y()));
                let base = editor.tools.threshold.get_untracked();
                gesture.set(Gesture::Filling { seed: point, anchor, base, erase: erasing, released: false });
                regrow();
            }
            (Tool::Brush, _) => {
                let label = editor.label.get_untracked().unwrap_or_default();
                let size = editor.tools.brush_size.get_untracked();
                let core = MaskStroke::begin(erasing, label, size, image_size, point);
                stroke.set_value(Some(Stroke::start(core, color_of(label))));
                limit_seed.set_value(Some(point));
                limit_level.set_value(None);
                update_limit(ev.shift_key(), current, false);
                stroke.update_value(|s| {
                    if let Some(s) = s {
                        editor.peek(|session| s.extend(point, session.annotations(), &color_of));
                    }
                });
                gesture.set(Gesture::Painting);
            }
            (Tool::Polyline, _) if editor.tools.freehand.get_untracked() => {
                let snapping = editor.tools.edge_snap.get_untracked() != ev.shift_key();
                let start = snapping.then(|| edge_path(None, point, current)).flatten().and_then(|path| path.first().copied()).unwrap_or(point);
                gesture.set(Gesture::Stroke { points: vec![start], current: start });
            }
            (Tool::Points, _) if editor.tools.single_point.get_untracked() => add_shape(Shape::Points { points: vec![point] }),
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
                editor.tools.brush_size.set(before);
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
            Tool::Select => editor.update(|session| session.press(pointer_at(&ev, point, current))),
            tool => draw_press(&ev, tool, point, current),
        }
    };

    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((screen, point, current)) = locate(&ev) else { return };
        if editor.tool.get_untracked() == Tool::Brush || fixed_tool().is_some() {
            hover.set(Some(inside(point)));
        }
        match gesture.get_untracked() {
            // Turning a fixed-size shape: its width side points at the pointer.
            Gesture::Placing { centre, .. } => {
                let angle = snap_turn((point.y - centre.y).atan2(point.x - centre.x), ev.shift_key() != editor.tools.snap.get_untracked());
                gesture.set(Gesture::Placing { centre, angle });
            }
            Gesture::Drawing { start, dragged, .. } => {
                let moved = current.to_screen(start).distance(screen) > DRAG_THRESHOLD;
                gesture.set(Gesture::Drawing {
                    start,
                    current: inside(point),
                    dragged: dragged || (moved && ev.buttons() & 1 == 1),
                    square: ev.shift_key() != editor.tools.square.get_untracked(),
                    from_centre: ev.alt_key() != editor.tools.from_centre.get_untracked(),
                });
            }
            Gesture::Ellipse { start, dragged, .. } => {
                let moved = current.to_screen(start).distance(screen) > DRAG_THRESHOLD;
                gesture.set(Gesture::Ellipse {
                    start,
                    current: inside(point),
                    dragged: dragged || (moved && ev.buttons() & 1 == 1),
                    circle: ev.shift_key(),
                    from_centre: ev.alt_key() != editor.tools.ellipse_from_centre.get_untracked(),
                });
            }
            Gesture::Stroke { mut points, .. } => {
                let to = inside(point);
                let last = *points.last().expect("a stroke starts with a point");
                let spacing = editor.tools.spacing.get_untracked();
                // Snapping follows the edge path from the last vertex; until its tiles are
                // loaded the line follows the pointer.
                let snapping = editor.tools.edge_snap.get_untracked() != ev.shift_key();
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
                editor.tools.brush_size.set(size.round().clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE));
            }
            Gesture::Filling { anchor, base, released: false, .. } => {
                let moved = (f64::from(ev.client_x()) - anchor.x) + (f64::from(ev.client_y()) - anchor.y);
                editor.tools.threshold.set((base + moved * THRESHOLD_PER_PIXEL).round().clamp(0.0, MAX_THRESHOLD));
                regrow();
            }
            // Released and waiting for tiles: the pointer no longer changes it.
            Gesture::Filling { released: true, .. } => {}
            Gesture::Painting => {
                let to = inside(point);
                update_limit(ev.shift_key(), current, false);
                stroke.update_value(|current| {
                    if let Some(current) = current {
                        editor.peek(|session| current.extend(to, session.annotations(), &color_of));
                    }
                });
                gesture.set(Gesture::Painting);
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
            // The Select tool's gestures are the session's.
            Gesture::Idle if editor.peek(Session::is_busy) => editor.update(|session| session.move_to(pointer_at(&ev, point, current))),
            Gesture::Idle => cursor.set(match editor.tool.get_untracked() {
                Tool::Select => match editor.peek(|session| session.grip_at(point, current.scale)).map(|grip| grip.kind) {
                    Some(GripKind::Resize(handle)) => handle.cursor(),
                    Some(GripKind::Turn { .. }) => "grab",
                    Some(GripKind::Radius(0)) => "ew-resize",
                    Some(GripKind::Radius(_)) => "ns-resize",
                    Some(GripKind::Vertex(_) | GripKind::Insert(_)) => "move",
                    None => "default",
                },
                _ => "default",
            }),
        }
    };

    let on_pointer_up = move |ev: ev::PointerEvent| {
        let Some((_, point, current)) = locate(&ev) else { return };
        match gesture.get_untracked() {
            // A click without dragging leaves the rectangle open for a second click.
            Gesture::Drawing { start, current: end, dragged: true, square, from_centre } => {
                finish_rectangle(start, end, square, from_centre, current.scale)
            }
            // A click without dragging leaves the ellipse open for a second click.
            Gesture::Ellipse { start, current: end, dragged: true, circle, from_centre } => finish_ellipse(start, end, circle, from_centre, current.scale),
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
            // The Select tool's gestures are the session's.
            Gesture::Idle if editor.peek(Session::is_busy) => editor.update(|session| session.release(pointer_at(&ev, point, current))),
            _ => {}
        }
    };

    let keys = window_event_listener(ev::keydown, move |ev| {
        if typing_in_a_field() || document().query_selector(".backdrop").ok().flatten().is_some() {
            return;
        }
        let key = ev.key();
        let keys = keymap.get_untracked();
        let press = press(&ev);
        let is = |action| keys.matches(action, &press);
        // While a pie is open its keys pick its items; Esc closes it.
        if let Some(open) = pie.get_untracked() {
            if key == "Escape" || is(Action::Cancel) {
                close_pie();
            } else if !is(Action::Pie) && !is(Action::LabelPie) {
                match open.item_for_key(&key).map(|item| item.action.clone()) {
                    Some(action) => choose(action),
                    None => return,
                }
            }
            ev.prevent_default();
            return;
        }
        if let Gesture::BrushSizing { before, .. } = gesture.get_untracked() {
            if is(Action::ResizeBrush) || is(Action::Finish) {
                gesture.set(Gesture::Idle);
            } else if is(Action::Cancel) {
                editor.tools.brush_size.set(before);
                gesture.set(Gesture::Idle);
            }
            ev.prevent_default();
            return;
        }
        if is(Action::Pie) || is(Action::LabelPie) {
            if !ev.repeat() {
                trigger_down.set_value(true);
                // The label pie key opens only the label pie; releasing it over a label picks it too.
                open_pie(true, is(Action::LabelPie));
            }
            ev.prevent_default();
            return;
        }
        let drawing_path = matches!(gesture.get_untracked(), Gesture::Path { .. });
        // A stroke keeps the indices of the masks it changes: no undo or redo under it.
        let painting = gesture.get_untracked() == Gesture::Painting;
        let tool = editor.tool.get_untracked();
        let (smaller, larger) = (is(Action::Smaller), is(Action::Larger));
        let brush_tool = |tool: Tool| {
            if editor.can_use(tool) {
                true
            } else {
                editor.notice.set(Some(Editor::unusable_notice(tool)));
                false
            }
        };
        if (is(Action::Undo) || is(Action::Redo)) && painting {
        } else if let Some(action) = [Action::Redo, Action::Undo, Action::SelectNone, Action::SelectAll].into_iter().find(|&action| is(action)) {
            editor.update(|session| session.key(action));
        } else if is(Action::Finish) && drawing_path {
            finish_path();
        } else if (smaller || larger) && tool == Tool::Brush && editor.tools.fill.get_untracked() {
            change_threshold(larger);
        } else if (smaller || larger) && tool == Tool::Brush {
            resize_brush(larger);
        } else if is(Action::ResizeBrush) && tool == Tool::Brush && !editor.tools.fill.get_untracked() && gesture.get_untracked() == Gesture::Idle {
            // Resize the outline where it is, by moving the pointer right or left.
            let Some(centre) = hover.get_untracked() else { return };
            gesture.set(Gesture::BrushSizing { centre, before: editor.tools.brush_size.get_untracked() });
        } else if (smaller || larger) && tool == Tool::Polyline {
            let factor = if larger { SIZE_STEP } else { 1.0 / SIZE_STEP };
            editor.tools.spacing.update(|spacing| *spacing = (*spacing * factor).clamp(MIN_SPACING, MAX_SPACING));
            editor.notice.set(Some(format!("Freehand spacing {:.0} px", editor.tools.spacing.get_untracked())));
        } else if is(Action::RemoveLastPoint) && drawing_path {
            undo_point();
        } else if is(Action::Cancel) {
            // Esc first drops the shape being drawn; the session then ends its gesture or clears
            // the selection; otherwise it returns to Select.
            if gesture.get_untracked() != Gesture::Idle {
                cancel_gesture();
            } else if !editor.update(|session| session.key(Action::Cancel)) {
                editor.tool.set(Tool::Select);
            }
        } else if is(Action::Delete) {
            editor.update(|session| session.key(Action::Delete));
        } else if is(Action::Copy) || is(Action::Cut) {
            let selected: Vec<Annotation> =
                editor.peek(|session| session.selection().iter().filter_map(|&index| session.annotations().get(index).cloned()).collect());
            let copies = clipboard::copied(&selected);
            // Tags are not copied, so a selection of tags alone keeps the clipboard as it was.
            if copies.is_empty() {
                return;
            }
            CLIPBOARD.set(Some((project, copies)));
            if is(Action::Cut) {
                // Cut removes only what it copied: tags stay.
                editor.update(|session| {
                    let list = session.annotations();
                    let copied = session.selection().iter().copied().filter(|&index| list.get(index).is_some_and(|a| !a.shape.is_tag())).collect();
                    session.set_selection(copied);
                    session.delete_selected();
                });
            }
        } else if is(Action::Paste) {
            let copies = CLIPBOARD.with_borrow(|copied| copied.as_ref().filter(|(of, _)| *of == project).map(|(_, copies)| copies.clone()));
            let Some(copies) = copies.filter(|copies| !copies.is_empty()) else { return };
            cancel_gesture();
            editor.tool.set(Tool::Select);
            let pasted = all_labels.with_value(|labels| clipboard::paste(&copies, labels, image_size.0, image_size.1));
            editor.notice.set(pasted.notice());
            editor.update(|session| {
                let start = session.annotations().len();
                let count = pasted.annotations.len();
                if count > 0 {
                    let adds = pasted.annotations.into_iter().enumerate().map(|(offset, annotation)| Edit::Add { index: start + offset, annotation });
                    session.apply(Edit::Batch(adds.collect()));
                }
                session.set_selection((start..start + count).collect());
            });
        } else if is(Action::Eraser) {
            // The brush in eraser mode (Blender has no eraser tool either).
            if brush_tool(Tool::Brush) {
                cancel_gesture();
                editor.tool.set(Tool::Brush);
                editor.tools.eraser.set(true);
            }
        } else if is(Action::FuzzyFill) {
            // The brush's fuzzy fill (U, GIMP's fuzzy select key).
            if brush_tool(Tool::Brush) {
                cancel_gesture();
                editor.tool.set(Tool::Brush);
                editor.tools.fill.set(true);
                editor.tools.eraser.set(false);
            }
        } else if is(Action::Tag) {
            // The label pie, whose label is toggled as a tag of the image.
            if brush_tool(Tool::Tag) {
                close_pie();
                pending_tool.set_value(Some(Tool::Tag));
                let at = cursor_at.get_value();
                pointer.set(at);
                pie.set(Some(label_pie(at, false)));
            }
        } else if let Some(tool) = KEY_TOOLS.into_iter().find(|tool| is(tool.action())) {
            if brush_tool(tool) {
                cancel_gesture();
                editor.tool.set(tool);
                if tool == Tool::Brush {
                    editor.tools.eraser.set(false);
                    editor.tools.fill.set(false);
                }
            }
        } else if let Some((_, id)) = hotkeys.iter().find(|(hotkey, _)| !press.ctrl && !press.alt && hotkey.eq_ignore_ascii_case(&key)) {
            match tool.shape_type() {
                Some(shape) if !label_allows(*id, shape) => {
                    editor.notice.set(Some(format!("“{}” cannot be used for {} shapes.", label_name(*id), tool.name().to_lowercase())))
                }
                _ => editor.label.set(Some(*id)),
            }
        } else {
            return;
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
        editor.peek(|session| {
            let (list, selection, threshold) = (session.annotations(), session.selection(), session.threshold());
            let visible = |annotation: &Annotation| !annotation.shape.is_tag() && annotation.shown(threshold);
            // The footprints of lines and points under everything else (see `draw::footprint`).
            if prefs.settings.with_untracked(|s| s.show_footprint) {
                let mut previous = ghosts.try_update_value(std::mem::take).unwrap_or_default();
                let mut kept = HashMap::new();
                for (index, annotation) in list.iter().enumerate().filter(|(_, a)| visible(a)) {
                    let Some(footprint) = footprints.with_value(|map| map.get(&annotation.label).copied()) else { continue };
                    let shape = session.shown_shape(index, annotation).into_owned();
                    let outlines = match previous.remove(&index) {
                        Some((before, made_with, outlines)) if before == shape && made_with == footprint => outlines,
                        _ => match footprint.region(&shape, image_size) {
                            Some(region) => region.outlines(),
                            None => continue,
                        },
                    };
                    draw::footprint(&context, current, &outlines, &color_of(annotation.label));
                    kept.insert(index, (shape, footprint, outlines));
                }
                ghosts.set_value(kept);
            }
            for (index, annotation) in list.iter().enumerate() {
                // Tags are chips over the image, not drawn on it; hidden shapes are not drawn.
                if !visible(annotation) {
                    continue;
                }
                let selected = selection.contains(&index);
                // Shapes being edited show their new form.
                let shape = session.shown_shape(index, annotation);
                let color = color_of(annotation.label);
                if let Shape::Mask(mask) = &*shape {
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
                // Pre-annotations not yet reviewed are dashed.
                draw::shape(&context, current, &shape, Look { color: &color, selected, dashed: annotation.pending });
                if selected {
                    let name = names.get(&annotation.label).cloned().unwrap_or_default();
                    draw::tag(&context, current.to_screen(shape.bounds().min), &name, &color);
                }
            }
            // The handles of a single selected shape: the list the session hit-tests.
            if let Some(annotation) = session.single_selection().and_then(|index| list.get(index)) {
                let color = color_of(annotation.label);
                for grip in session.grips(current.scale) {
                    draw::grip(&context, current, grip, &color);
                }
            }
            if let Some(region) = session.selection_box() {
                draw::selection_box(&context, current, region);
            }
        });
        sprites.update_value(SpriteCache::sweep);
        stroke.with_value(|s| s.as_ref().map(|s| s.draw_painted(&context, current)));
        fill_preview.with_value(|preview| preview.as_ref().map(|preview| preview.draw(&context, current)));
        let brush_radius = editor.tools.brush_size.get_untracked() / 2.0;
        if let Gesture::BrushSizing { centre, .. } = gesture {
            draw::brush_outline(&context, current, centre, brush_radius);
            let label = format!("{:.0} px", 2.0 * brush_radius);
            draw::tag(&context, current.to_screen(centre.offset(-brush_radius, -brush_radius)), &label, "#e8e6e1");
        } else if let (Tool::Brush, false, Some(at)) = (editor.tool.get_untracked(), editor.tools.fill.get_untracked(), hover.get_untracked()) {
            draw::brush_outline(&context, current, at, brush_radius);
        }
        let pen = editor.label.get_untracked().map(color_of).unwrap_or_else(|| "#ffffff".to_owned());
        // The fixed-size shape the next click places: at the pointer, or turning.
        if let Some((shape_type, constraints)) = fixed_tool() {
            let placing = match gesture {
                Gesture::Placing { centre, angle } => Some((centre, angle, true)),
                Gesture::Idle => hover.get_untracked().map(|at| (at, 0.0, false)),
                _ => None,
            };
            if let Some((centre, angle, turning)) = placing {
                if let Some(shape) = constraints.place(shape_type, centre, angle, image_size.0, image_size.1) {
                    draw::shape(&context, current, &shape, Look { color: &pen, selected: false, dashed: true });
                    let text = match (turning, constraints.size(shape_type)) {
                        (true, _) => format!("{:.0}°", angle.to_degrees()),
                        (false, Some((a, b))) => format!("{a:.0} × {b:.0}"),
                        (false, None) => String::new(),
                    };
                    draw::tag(&context, current.to_screen(shape.bounds().min), &text, &pen);
                }
            }
        }
        match gesture {
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
            Gesture::Ellipse { start, current: end, circle, from_centre, .. } => {
                let ellipse = ellipse_from(start, end, circle, from_centre);
                let shape = Shape::Ellipse(ellipse);
                draw::shape(&context, current, &shape, Look { color: &pen, selected: false, dashed: true });
                draw::tag(&context, current.to_screen(shape.bounds().min), &side_panel::ellipse_size(&ellipse), &pen);
            }
            _ => {}
        }
    };
    Effect::new(move |_| {
        view.track();
        size.track();
        gesture.track();
        editor.track_changes();
        editor.label.track();
        editor.tool.track();
        editor.tools.oriented.track();
        prefs.settings.track();
        editor.tools.brush_size.track();
        editor.tools.fill.track();
        fill_drawn.track();
        hover.track();
        draw();
    });
    Effect::new(move |_| {
        editor.track_changes();
        let busy = gesture.with(|g| *g != Gesture::Idle) || editor.peek(Session::is_busy);
        if editor.busy.get_untracked() != busy {
            editor.busy.set(busy);
        }
    });
    // The session's gestures follow the snap option.
    Effect::new(move |_| {
        let snap = editor.tools.snap.get();
        editor.update(|session| session.set_options(Options { snap }));
    });
    // Drop a half-drawn shape when the tool or a drawing mode changes.
    // Another label can fix another size, or none: drop a shape being placed.
    Effect::new(move |_| {
        editor.label.track();
        if matches!(gesture.get_untracked(), Gesture::Placing { .. }) {
            gesture.set(Gesture::Idle);
        }
    });
    Effect::new(move |_| {
        editor.tool.track();
        editor.tools.oriented.track();
        editor.tools.freehand.track();
        editor.tools.single_point.track();
        editor.tools.eraser.track();
        editor.tools.fill.track();
        cancel_gesture();
    });

    let canvas_cursor = move || match editor.tool.get() {
        Tool::Select => cursor.get(),
        _ => "crosshair",
    };
    view! {
        <PieMenu pie=pie pointer=pointer on_pick=choose on_background=background_click />
        <div class="tag-chips" aria-label="Image tags">
            {move || editor.with_annotations(|list| {
                list.iter().enumerate().filter(|(_, a)| a.shape.is_tag()).map(|(index, a)| {
                    let name = label_name(a.label);
                    let selected = move || editor.is_selected(index);
                    view! {
                        <span class="tag-chip" class:selected=selected style:border-color=color_of(a.label)>
                            <span class="swatch" style:background=color_of(a.label)></span>
                            <button class="name" title="Select this tag" on:click=move |_| editor.update(|session| session.set_selection(BTreeSet::from([index])))>{name.clone()}</button>
                            <button class="remove" title="Remove this tag" aria-label=format!("Remove tag {name}") on:click=move |_| editor.update(|session| session.remove(index))>"×"</button>
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
            data-count=move || editor.with_annotations(<[Annotation]>::len)
            data-selected={move || editor.selection().iter().map(usize::to_string).collect::<Vec<_>>().join(",")}
            data-revision=move || editor.revision()
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
