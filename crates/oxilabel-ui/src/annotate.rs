//! Drawing and editing annotations on top of the tile viewer. The image's annotations, undo
//! history, selection and every gesture of the Select and drawing tools live in the editor
//! session (`oxilabel_core::editor`); this layer is its adapter: browser events in, signals and
//! canvas drawing out. It forwards presses, moves, releases, Enter, Backspace and Esc to the
//! session, hands it the pixels its gestures need (the edge path a freehand polyline follows, the
//! region a fuzzy fill grows on, the pixels a "Stop at edges" brush may paint) and draws the
//! session's preview and stroke sprites.
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
//! `]` or F (then move the pointer right or left and click; Esc restores the size) change the
//! size. Its fuzzy fill mode (U) fills the similar region around a click (`fill`), a drag
//! changing the threshold. Freehand polylines can snap to edges (live-wire, `edges`) and the
//! brush can stop at edges (painting only the similar region around the stroke's start); Shift
//! inverts either while drawing. Esc ends the session's gesture (a shape being drawn, a stroke,
//! a fill or the selection), and then the layer returns to Select. Ctrl+Z and Ctrl+Y undo and redo. Ctrl+C
//! copies the selection and Ctrl+X cuts it; Ctrl+V pastes the copies on any image of the project
//! at the same place, selected (see `oxilabel_core::clipboard`).
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
    edges::EdgeMap,
editor::{BrushMode, FillRegion, Layer, Modifiers, Options, Pointer, Preview, Session, grip::GripKind},
    fill::{FillSettings, grow},
    geometry::{Point, Rect},
    mask::{Bitmap, PixelRect},
    pyramid::Pyramid,
    history::Edit,
    keymap::Action,
    pie::{Compass, is_tap},
    settings::LabelStyle,
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

/// The fuzzy fill's largest region, as a share of the image.
pub const DEFAULT_MAX_FILL_PERCENT: f64 = 25.0;
/// Edge snapping looks this far around the pointer, in screen pixels, by default.
pub const DEFAULT_SNAP_RADIUS: f64 = 12.0;
/// The largest "Stop at edges" offset either way, in image pixels.
pub const MAX_EDGE_OFFSET: f64 = 200.0;
/// `[` and `]` change the freehand spacing by this factor.
const SIZE_STEP: f64 = 1.25;

/// Builds the session's borrowed analysis callbacks for one view and runs its operation while
/// those callbacks live. Pointer gestures include edge paths; tile-ready updates do not.
fn with_analysis_layer<R>(
    current: View,
    include_edges: bool,
    edge_path: impl Fn(Option<Point>, Point, View) -> Option<Vec<Point>>,
    fill_region: impl Fn(Point, f64, View) -> FillRegion,
    edge_limit: impl Fn(Point, View) -> Bitmap,
    run: impl FnOnce(Layer<'_>) -> R,
) -> R {
    let edges = |from, to| edge_path(from, to, current);
    let fill = |seed, threshold| fill_region(seed, threshold, current);
    let paintable = |seed| edge_limit(seed, current);
    let layer = Layer {
        edges: if include_edges { Some(&edges) } else { None },
        fill: Some(&fill),
        paintable: Some(&paintable),
    };
    run(layer)
}

thread_local! {
    /// The copied annotations and their project: kept while moving between the project's images,
    /// not across a reload.
    static CLIPBOARD: RefCell<Option<(i64, Vec<Annotation>)>> = const { RefCell::new(None) };
}

pub use oxilabel_core::editor::{
    DEFAULT_BRUSH_SIZE, DEFAULT_SPACING, DEFAULT_THRESHOLD, MAX_BRUSH_SIZE, MAX_SPACING, MAX_THRESHOLD, MIN_BRUSH_SIZE, MIN_SPACING, Tool,
};

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
    /// Notified after every successful tool switch, so the layer drops what it draws.
    switched: Trigger,
    pub tool: RwSignal<Tool>,
    /// The label new shapes get.
    pub label: RwSignal<Option<i64>>,
    /// A short hint for the user, e.g. why a shape could not be drawn.
    pub notice: RwSignal<Option<String>>,
    /// The drawing tools' options and the last tool, kept while moving between images.
    pub tools: Tools,
    /// The enhancement filter shown (its key; `None` for the original), set by the view filter.
    pub view_filter: RwSignal<Option<String>>,
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

/// The drawing modes remembered with the last tool, for a tap of the pie key. The brush's eraser
/// and fuzzy fill modes are not among them: choosing Brush resets it to plain painting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Modes {
    oriented: bool,
    freehand: bool,
    single_point: bool,
    ellipse_from_centre: bool,
}

impl Editor {
    /// Whether `tool` can be used: Select always, a drawing tool when some label allows its shape.
    pub fn can_use(&self, tool: Tool) -> bool {
        self.peek(|session| session.can_use(tool))
    }

    fn mode_signals(&self) -> [RwSignal<bool>; 4] {
        [self.tools.oriented, self.tools.freehand, self.tools.single_point, self.tools.ellipse_from_centre]
    }

    /// The current modes (tracked).
    pub fn modes(&self) -> Modes {
        let [oriented, freehand, single_point, ellipse_from_centre] = self.mode_signals().map(|mode| mode.get());
        Modes { oriented, freehand, single_point, ellipse_from_centre }
    }

    /// Repeats the last tool, label and modes (a tap of the pie key): the drawing modes are set
    /// first, then the tool goes through the session's one switching rule, and the saved label is
    /// restored only while it allows the tool's shape.
    fn restore(&self, tool: Tool, label: i64, modes: Modes) {
        let Modes { oriented, freehand, single_point, ellipse_from_centre } = modes;
        for (signal, value) in self.mode_signals().into_iter().zip([oriented, freehand, single_point, ellipse_from_centre]) {
            if signal.get_untracked() != value {
                signal.set(value);
            }
        }
        if self.switch_tool(tool)
            && tool.shape_type().is_some_and(|shape| self.peek(|session| session.label_allows(label, shape)))
        {
            self.set_label(Some(label));
        }
    }

    pub fn new(mut session: Session, label: Option<i64>, tools: Tools) -> Self {
        session.set_label(label);
        // The saved switches are only read here; from then on the session's mode is the truth
        // and the switches mirror it for the toolbar and the saved tools.
        session.set_brush_eraser(tools.eraser.get_untracked());
        session.set_brush_fill(tools.fill.get_untracked());
        Editor {
            session: StoredValue::new(session),
            annotations_changed: Trigger::new(),
            selection_changed: Trigger::new(),
            threshold_changed: Trigger::new(),
            changed: Trigger::new(),
            switched: Trigger::new(),
            tool: RwSignal::new(Tool::Select),
            label: RwSignal::new(label),
            notice: RwSignal::new(None),
            tools,
            view_filter: RwSignal::new(None),
            busy: RwSignal::new(false),
        }
    }

    /// Switches to `tool` through the session's one rule (the tool pie, the rail and a tap of the
    /// pie key come here; tool keys come through [`Editor::key`]).
    pub fn switch_tool(&self, tool: Tool) -> bool {
        let switched = self.update(|session| session.switch_tool(tool));
        self.mirror_tool_state();
        if switched {
            // Cancels what the annotation layer is drawing, even when the tool is chosen again.
            self.switched.notify();
        }
        switched
    }

    /// Passes a key action to the session and mirrors what it changed: a tool key switches tools
    /// through the session's one rule, and Esc returns to Select.
    pub fn key(&self, action: Action) -> bool {
        let was = self.peek(|session| session.tool());
        let handled = self.update(|session| session.key(action));
        self.mirror_tool_state();
        if was != self.tool.get_untracked() {
            self.switched.notify();
        }
        handled
    }

    /// Mirrors the session's tool, label and brush mode after either switching entry point.
    fn mirror_tool_state(&self) {
        let (tool, label, brush) = self.peek(|session| (session.tool(), session.label(), session.brush_mode()));
        if self.tool.get_untracked() != tool {
            self.tool.set(tool);
        }
        if self.label.get_untracked() != label {
            self.label.set(label);
        }
        self.mirror_brush_mode(brush);
    }

    /// Sets the label new shapes get, in the session and the signal the view reads.
    pub fn set_label(&self, label: Option<i64>) {
        self.update(|session| session.set_label(label));
        if self.label.get_untracked() != label {
            self.label.set(label);
        }
    }

    /// Sets the brush's mode (the eraser and fuzzy fill keys, the options pie).
    pub fn set_brush_mode(&self, mode: BrushMode) {
        self.update(|session| session.set_brush_mode(mode));
        self.mirror_brush_mode(mode);
    }

    /// The brush's mode, from the session's one rule; the two switches only mirror it for the
    /// toolbar and the saved tools.
    pub fn brush_mode(&self) -> BrushMode {
        self.peek(Session::brush_mode)
    }

    /// Turns the eraser on or off, keeping fuzzy fill (the E key and the options pie).
    pub fn set_brush_eraser(&self, eraser: bool) {
        self.set_brush_mode(self.brush_mode().with_eraser(eraser));
    }

    /// Turns fuzzy fill on or off, keeping the eraser (the options pie).
    pub fn set_brush_fill(&self, fill: bool) {
        self.set_brush_mode(self.brush_mode().with_fill(fill));
    }

    /// The brush switch signals follow the session's mode.
    fn mirror_brush_mode(&self, mode: BrushMode) {
        let (eraser, fill) = (mode.erases(), mode.fills());
        if self.tools.eraser.get_untracked() != eraser {
            self.tools.eraser.set(eraser);
        }
        if self.tools.fill.get_untracked() != fill {
            self.tools.fill.set(fill);
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

    /// Changes the session, then notifies whatever changed and shows its notice. The brush's size
    /// and the fill's threshold are kept in the session while a gesture changes them; a change
    /// the session made is mirrored into their signals, which persist the tools.
    pub fn update<T>(&self, change: impl FnOnce(&mut Session) -> T) -> T {
        let (revision, selection, threshold, brush) = self.peek(|s| (s.revision(), s.selection().clone(), s.threshold(), (s.brush_size(), s.fill_threshold())));
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
        let (brush_size, threshold) = self.peek(|s| (s.brush_size(), s.fill_threshold()));
        if brush_size != brush.0 && self.tools.brush_size.get_untracked() != brush_size {
            self.tools.brush_size.set(brush_size);
        }
        if threshold != brush.1 && self.tools.threshold.get_untracked() != threshold {
            self.tools.threshold.set(threshold);
        }
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

    /// Relabels the annotations at `indices` through the session's one rule, as the objects list
    /// does for a single row.
    pub fn relabel(&self, indices: &[usize], label: i64) -> bool {
        self.update(|session| session.relabel(indices, label))
    }

    /// Relabels the selection through the session's one rule, as the label pie does.
    pub fn relabel_selection(&self, label: i64) -> bool {
        self.update(|session| session.relabel_selection(label))
    }
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
    // The footprints the editor shows, by label (labels that show theirs).
    let footprints: StoredValue<HashMap<i64, Footprint>> =
        StoredValue::new(labels.iter().filter(|label| label.show_footprint).map(|label| (label.id, label.footprint)).collect());
    // The outlines of the footprints drawn last, by annotation index, with the shape and footprint
    // they were made from: a region is only made again when either changes.
    let ghosts: StoredValue<HashMap<usize, (Shape, Footprint, Vec<Vec<Point>>)>> = StoredValue::new(HashMap::new());
    // The shape type the current tool draws and the current label's constraints, when that label
    // constrains the type (the session then places shapes instead of dragging them out).
    let constrained_shape = move || editor.peek(Session::constrained_shape);

    // Pointer position in canvas pixels and in image coordinates.
    let locate = move |ev: &ev::MouseEvent| -> Option<(Point, Point, View)> {
        let canvas = canvas_ref.get_untracked()?;
        let current = view.get_untracked()?;
        let rect = canvas.get_bounding_client_rect();
        let screen = Point::new(f64::from(ev.client_x()) - rect.left(), f64::from(ev.client_y()) - rect.top());
        Some((screen, current.to_image(screen), current))
    };
    // A pointer event for the session, at `point` in the image.
    let pointer_at = move |ev: &ev::MouseEvent, point: Point, current: View, primary: bool| Pointer {
        at: point,
        scale: current.scale,
        modifiers: Modifiers { shift: ev.shift_key(), ctrl: ev.ctrl_key() || ev.meta_key(), alt: ev.alt_key() },
        primary,
    };

    // The brush or eraser stroke's sprites, the stored masks' sprites, and where the brush
    // outline is drawn.
    let stroke = StoredValue::new_local(None::<Stroke>);
    let sprites = StoredValue::new_local(SpriteCache::default());
    let hover = RwSignal::new(None::<Point>);
    // The fuzzy fill's region before it is committed.
    let fill_preview = StoredValue::new_local(None::<FillPreview>);
    // Drops what the session is drawing and the sprites showing it.
    let cancel_gesture = move || {
        stroke.set_value(None);
        fill_preview.set_value(None);
        if editor.peek(Session::is_busy) {
            editor.update(Session::cancel);
        }
    };
    // Updates the stroke's sprites for the pixels the session's stroke changed, making them when
    // the stroke starts and dropping them when it ends.
    let sync_stroke = move || {
        if !editor.peek(Session::is_brushing) {
            stroke.set_value(None);
            return;
        }
        let Some(change) = editor.update(Session::take_stroke_change) else { return };
        if stroke.with_value(Option::is_none) {
            let color = editor.label.get_untracked().map(color_of).unwrap_or_default();
            stroke.set_value(Some(Stroke::new(color)));
        }
        editor.peek(|session| {
            stroke.update_value(|slot| {
                if let Some(stroke) = slot.as_mut() {
                    let color = |index: usize| session.annotations().get(index).map(|a| color_of(a.label)).unwrap_or_default();
                    stroke.extend(change, session, &color);
                }
            });
        });
    };
    // Shows the session's fill region as the preview sprite, rebuilding it only when it changed.
    let sync_fill = move || {
        editor.peek(|session| {
            let built = session.fill_preview().map(|region| (region, session.fill_erases()));
            fill_preview.update_value(|slot| match built {
                None => *slot = None,
                Some((region, erase)) => {
                    let color = if erase { "#ffffff".to_owned() } else { editor.label.get_untracked().map(color_of).unwrap_or_default() };
                    let same = slot.as_ref().is_some_and(|preview| preview.factor == region.factor && preview.color == color && preview.region == region.region);
                    if !same {
                        *slot = Some(FillPreview::new(region.region.clone(), region.factor, &color));
                    }
                }
            });
        });
    };

    // ---- Fuzzy fill and edge snapping: the analysed pixels the session's gestures read, grown on
    // the tiles loaded so far, asking for those a region reached and growing again as they arrive.
    let analysis = Analysis::new(image_id);
    let pyramid = Pyramid::new(image_size.0, image_size.1);
    // The pixels of a fill at `threshold` (in pixels of a level whose pixels cover `factor` ×
    // `factor` image pixels), with its maximum size.
    let fill_settings = move |within: Option<PixelRect>, factor: u32, threshold: f64| {
        let image_area = f64::from(image_size.0) * f64::from(image_size.1);
        let level_area = f64::from(factor) * f64::from(factor);
        FillSettings {
            threshold: threshold.clamp(0.0, MAX_THRESHOLD) as u8,
            within,
            max_pixels: (editor.tools.max_fill.get_untracked() / 100.0 * image_area / level_area).max(1.0) as u64,
        }
    };
    // The fuzzy fill's region around `seed` at `threshold`, on the tiles of the level being viewed
    // (a zoom during a fill applies from then on, ADR 0002).
    let fill_region = move |seed: Point, threshold: f64, current: View| -> FillRegion {
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
        let settings = fill_settings(within, factor, threshold);
        let source = editor.analysed(editor.tools.fill_filtered);
        let growth = analysis.with_level(&source, level, |tiles| grow(pyramid, level, tiles, seed, settings));
        analysis.request(&source, level, &growth.missing);
        let min_area = (editor.tools.clean_up.get_untracked() / level_area).round() as usize;
        let region = growth.region.cropped();
        // Clean-up drops small patches, but never the whole fill.
        let region = match region.clone().cleaned(min_area) {
            cleaned if cleaned.bounds().is_some() => cleaned,
            _ => region,
        };
        FillRegion { region, factor, incomplete: !growth.missing.is_empty(), capped: growth.capped }
    };
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
    let edge_limit = move |seed: Point, current: View| -> Bitmap {
        let (level, _) = level_of(current);
        let factor = 1u32 << level;
        let (width, height) = pyramid.level_size(level);
        let seed = ((seed.x as u32 / factor).min(width - 1), (seed.y as u32 / factor).min(height - 1));
        let source = editor.analysed(editor.tools.edges_filtered);
        let threshold = editor.tools.threshold.get_untracked();
        let growth = analysis.with_level(&source, level, |tiles| grow(pyramid, level, tiles, seed, fill_settings(None, factor, threshold)));
        analysis.request(&source, level, &growth.missing);
        let region = growth.region.cropped().scaled_up(factor, image_size);
        // The offset is in image pixels, so it applies at full resolution.
        match editor.tools.edge_offset.get_untracked().round() as i32 {
            0 => region,
            offset => region.expanded(offset, image_size),
        }
    };
    // The analysed pixels the session's gestures still need, grown again as they arrive or the
    // view changes: a fill's region (committed once it is complete and released) and the "Stop at
    // edges" limit of a stroke.
    Effect::new(move |_| {
        analysis.loaded.track();
        let Some(current) = view.get() else { return };
        if !editor.peek(Session::is_brushing) {
            return;
        }
        with_analysis_layer(current, false, edge_path, fill_region, edge_limit, |layer| {
            editor.update(|session| session.pixels_ready(layer));
        });
        sync_stroke();
        sync_fill();
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
            editor.update(Session::cancel);
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
        let drawing = editor.peek(|session| matches!(session.preview(), Some(Preview::Path { .. })));
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
                        item(Compass::E, "Higher threshold", key_of(Action::Larger), true, false, PieAction::BrushStep(true)),
                        item(Compass::W, "Lower threshold", key_of(Action::Smaller), true, false, PieAction::BrushStep(false)),
                    ]);
                } else {
                    items.extend([
                        item(Compass::E, "Larger", key_of(Action::Larger), true, false, PieAction::BrushStep(true)),
                        item(Compass::W, "Smaller", key_of(Action::Smaller), true, false, PieAction::BrushStep(false)),
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
    // Ticket 14's unified entry-point rule: Tag uses keyboard T's label-choice semantics from
    // every pie too. Tag is never active, so the current tool and its gesture stay untouched.
    // `held` keeps the label pie held while Q or the right button is still down from the tool pie,
    // so releasing over a label picks it; keyboard T opens it for clicking.
    let open_tag_pie = move |held: bool| {
        if editor.can_use(Tool::Tag) {
            close_pie();
            pending_tool.set_value(Some(Tool::Tag));
            let at = cursor_at.get_value();
            pointer.set(at);
            pie.set(Some(label_pie(at, held)));
        } else {
            editor.notice.set(Some(Session::unusable_notice(Tool::Tag)));
        }
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
                editor.relabel_selection(id);
            }
            (false, Some(Tool::Tag)) => toggle_tag(id),
            // The tool was chosen through the session's rule when its pie item was picked.
            (false, _) => editor.set_label(Some(id)),
        }
    };
    let choose = move |action: PieAction| {
        let centre = pie.get_untracked().map_or(cursor_at.get_value(), |open| open.centre);
        pie.set(None);
        match action {
            PieAction::Tool(Tool::Select) => {
                close_pie();
                editor.switch_tool(Tool::Select);
            }
            PieAction::Tool(Tool::Tag) => open_tag_pie(trigger_down.get_value()),
            // Picking a drawing tool chains to the label pie, centred where the pointer is so the next
            // flick starts fresh; it stays held while Q or the right button is still down.
            PieAction::Tool(tool) => {
                if !editor.switch_tool(tool) {
                    close_pie();
                    return;
                }
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
                editor.set_brush_eraser(on);
            }
            PieAction::SetFill(on) => {
                close_pie();
                editor.set_brush_fill(on);
            }
            PieAction::ToggleEdgeSnap => {
                close_pie();
                editor.tools.edge_snap.update(|on| *on = !*on);
            }
            PieAction::ToggleStopAtEdges => {
                close_pie();
                editor.tools.stop_at_edges.update(|on| *on = !*on);
            }
            PieAction::BrushStep(larger) => {
                close_pie();
                editor.key(if larger { Action::Larger } else { Action::Smaller });
            }
            PieAction::FinishShape => {
                close_pie();
                editor.key(Action::Finish);
            }
            PieAction::UndoPoint => {
                close_pie();
                editor.key(Action::RemoveLastPoint);
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

    // ---- Pointer handling on the canvas. Every gesture is the session's; the layer supplies the
    // pixels its gestures read and keeps the sprites showing them.
    let on_pointer_down = move |ev: ev::PointerEvent| {
        // A click while sizing the brush keeps the size; the right button restores it.
        if editor.peek(Session::is_sizing_brush) {
            ev.prevent_default();
            let action = if ev.button() == 2 { Action::Cancel } else { Action::Finish };
            editor.key(action);
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
        with_analysis_layer(current, true, edge_path, fill_region, edge_limit, |layer| {
            editor.update(|session| session.press_in(pointer_at(&ev, point, current, true), layer));
        });
        sync_stroke();
        sync_fill();
    };

    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((_, point, current)) = locate(&ev) else { return };
        if editor.tool.get_untracked() == Tool::Brush || constrained_shape().is_some() {
            hover.set(Some(inside(point)));
        }
        if !editor.peek(Session::is_busy) {
            cursor.set(match editor.tool.get_untracked() {
                Tool::Select => match editor.peek(|session| session.grip_at(point, current.scale)).map(|grip| grip.kind) {
                    Some(GripKind::Resize(handle)) => handle.cursor(),
                    Some(GripKind::Turn { .. }) => "grab",
                    Some(GripKind::Radius(0)) => "ew-resize",
                    Some(GripKind::Radius(_)) => "ns-resize",
                    Some(GripKind::Vertex(_) | GripKind::Insert(_)) => "move",
                    None => "default",
                },
                _ => "default",
            });
            return;
        }
        let primary = ev.buttons() & 1 == 1;
        with_analysis_layer(current, true, edge_path, fill_region, edge_limit, |layer| {
            editor.update(|session| session.move_to_in(pointer_at(&ev, point, current, primary), layer));
        });
        sync_stroke();
        sync_fill();
    };

    let on_pointer_up = move |ev: ev::PointerEvent| {
        // Only the primary button ends a gesture (the pie's right button has its own release).
        if ev.button() != 0 || !editor.peek(Session::is_busy) {
            return;
        }
        let Some((_, point, current)) = locate(&ev) else { return };
        editor.update(|session| session.release(pointer_at(&ev, point, current, true)));
        sync_stroke();
        sync_fill();
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
        // The brush's F gesture owns F, Enter and Esc while it runs, and swallows other keys,
        // except that choosing a tool (a tool key, E, U, or Q's pies and tap) cancels the sizing
        // and then takes the same path as outside the gesture (ticket 14's one rule).
        if editor.peek(Session::is_sizing_brush) {
            if let Some(action) = [Action::ResizeBrush, Action::Finish, Action::Cancel].into_iter().find(|&action| is(action)) {
                editor.key(action);
                ev.prevent_default();
                return;
            }
            let chooses_tool = [Action::Pie, Action::LabelPie, Action::Eraser, Action::FuzzyFill]
                .into_iter()
                .chain(KEY_TOOLS.into_iter().map(Tool::action))
                .any(|action| is(action));
            if !chooses_tool {
                ev.prevent_default();
                return;
            }
        }
        if is(Action::Pie) || is(Action::LabelPie) {
            if !ev.repeat() {
                // A pie's flick must not resize the brush: sizing is cancelled first. Other
                // gestures stay, as the options pie can finish or edit them.
                if editor.peek(Session::is_sizing_brush) {
                    editor.update(Session::cancel);
                }
                trigger_down.set_value(true);
                // The label pie key opens only the label pie; releasing it over a label picks it too.
                open_pie(true, is(Action::LabelPie));
            }
            ev.prevent_default();
            return;
        }
        let tool = editor.tool.get_untracked();
        let (smaller, larger) = (is(Action::Smaller), is(Action::Larger));
        // A stroke keeps annotation indices, so the session's list rule cancels it on undo or redo.
        if let Some(action) = [Action::Redo, Action::Undo, Action::SelectNone, Action::SelectAll].into_iter().find(|&action| is(action)) {
            editor.key(action);
        } else if is(Action::Finish) {
            if !editor.key(Action::Finish) {
                return;
            }
        } else if (smaller || larger) && tool == Tool::Brush {
            editor.key(if larger { Action::Larger } else { Action::Smaller });
        } else if is(Action::ResizeBrush) && tool == Tool::Brush && !editor.tools.fill.get_untracked() && !editor.peek(Session::is_busy) {
            // Resize the outline where it is, by moving the pointer right or left.
            let Some(centre) = hover.get_untracked() else { return };
            editor.update(|session| session.begin_brush_sizing(centre));
        } else if (smaller || larger) && tool == Tool::Polyline {
            let factor = if larger { SIZE_STEP } else { 1.0 / SIZE_STEP };
            editor.tools.spacing.update(|spacing| *spacing = (*spacing * factor).clamp(MIN_SPACING, MAX_SPACING));
            editor.notice.set(Some(format!("Freehand spacing {:.0} px", editor.tools.spacing.get_untracked())));
        } else if is(Action::RemoveLastPoint) {
            if !editor.key(Action::RemoveLastPoint) {
                return;
            }
        } else if is(Action::Cancel) {
            // The session's rule: end the gesture, then clear the selection, then return to Select.
            editor.key(Action::Cancel);
        } else if is(Action::Delete) {
            editor.key(Action::Delete);
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
            editor.switch_tool(Tool::Select);
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
            // The brush in eraser mode (Blender has no eraser tool either). Fuzzy fill stays: a
            // fill in eraser mode cuts the region from the masks. Choosing Brush itself resets to
            // painting; this key then puts the eraser back on.
            let mode = editor.brush_mode();
            if editor.switch_tool(Tool::Brush) {
                cancel_gesture();
                editor.set_brush_mode(mode.with_eraser(true));
            }
        } else if is(Action::FuzzyFill) {
            // The brush's fuzzy fill (U, GIMP's fuzzy select key).
            if editor.switch_tool(Tool::Brush) {
                cancel_gesture();
                editor.set_brush_mode(BrushMode::Fill);
            }
        } else if is(Action::Tag) {
            open_tag_pie(false);
        } else if let Some(action) = KEY_TOOLS.into_iter().map(Tool::action).find(|&action| is(action)) {
            // The session's `key` maps the action to its tool and switches through the one rule.
            if editor.key(action) {
                cancel_gesture();
            }
        } else if let Some((_, id)) = hotkeys.iter().find(|(hotkey, _)| !press.ctrl && !press.alt && hotkey.eq_ignore_ascii_case(&key)) {
            match tool.shape_type() {
                Some(shape) if !label_allows(*id, shape) => {
                    editor.notice.set(Some(format!("“{}” cannot be used for {} shapes.", label_name(*id), tool.name().to_lowercase())))
                }
                _ => editor.set_label(Some(*id)),
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

        // The session may have ended a stroke or a fill itself (Esc, an undo).
        if stroke.with_value(Option::is_some) && editor.peek(|session| !session.is_brushing()) {
            stroke.set_value(None);
        }
        if fill_preview.with_value(Option::is_some) && editor.peek(|session| session.fill_preview().is_none()) {
            fill_preview.set_value(None);
        }
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
                    let shown = stroke.with_value(|s| s.as_ref().is_some_and(|s| s.draw_changed(&context, current, index, selected, session)));
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
        editor.peek(|session| stroke.with_value(|s| s.as_ref().map(|s| s.draw_painted(&context, current, session))));
        fill_preview.with_value(|preview| preview.as_ref().map(|preview| preview.draw(&context, current)));
        if let Some((centre, size)) = editor.peek(Session::brush_sizing) {
            let radius = size / 2.0;
            draw::brush_outline(&context, current, centre, radius);
            let label = format!("{size:.0} px");
            draw::tag(&context, current.to_screen(centre.offset(-radius, -radius)), &label, "#e8e6e1");
        } else if let (Tool::Brush, false, Some(at)) = (editor.tool.get_untracked(), editor.tools.fill.get_untracked(), hover.get_untracked()) {
            draw::brush_outline(&context, current, at, editor.tools.brush_size.get_untracked() / 2.0);
        }
        let pen = editor.label.get_untracked().map(color_of).unwrap_or_else(|| "#ffffff".to_owned());
        // The constrained shape the next click places: it hangs at the pointer until a click fixes
        // its centre, after which the session turns it with the pointer.
        if editor.peek(Session::preview).is_none() {
            if let Some(at) = hover.get_untracked() {
                if let Some((shape, size)) = editor.peek(|session| session.constrained_preview(at)) {
                    draw::shape(&context, current, &shape, Look { color: &pen, selected: false, dashed: true });
                    let text = match size {
                        Some((a, b)) => format!("{a:.0} × {b:.0}"),
                        None => String::new(),
                    };
                    draw::tag(&context, current.to_screen(shape.bounds().min), &text, &pen);
                }
            }
        }
        match editor.peek(Session::preview) {
            Some(Preview::Rectangle(rect)) => {
                draw::shape(&context, current, &Shape::Rectangle(rect), Look { color: &pen, selected: false, dashed: true });
                draw::tag(&context, current.to_screen(rect.min), &format!("{:.0} × {:.0}", rect.width(), rect.height()), &pen);
            }
            Some(Preview::Oriented { points, current: next, shape }) => match shape {
                Some(rect) => draw::oriented_preview(&context, current, rect, &pen),
                None => match points[..] {
                    [a] => draw::path_preview(&context, current, &[a], next, false, false, &pen),
                    _ => {}
                },
            },
            Some(Preview::Path { points, current: next, shape }) => {
                draw::path_preview(&context, current, &points, next, shape == ShapeType::Polygon, shape == ShapeType::Points, &pen);
            }
            Some(Preview::Freehand { points, current: next }) => draw::path_preview(&context, current, &points, next, false, false, &pen),
            Some(Preview::Ellipse(ellipse)) => {
                let shape = Shape::Ellipse(ellipse);
                draw::shape(&context, current, &shape, Look { color: &pen, selected: false, dashed: true });
                draw::tag(&context, current.to_screen(shape.bounds().min), &side_panel::ellipse_size(&ellipse), &pen);
            }
            Some(Preview::Placing { shape, angle }) => {
                draw::shape(&context, current, &shape, Look { color: &pen, selected: false, dashed: true });
                let text = format!("{:.0}°", angle.to_degrees());
                draw::tag(&context, current.to_screen(shape.bounds().min), &text, &pen);
            }
            None => {}
        }
    };
    Effect::new(move |_| {
        view.track();
        size.track();
        editor.track_changes();
        editor.label.track();
        editor.tool.track();
        editor.tools.oriented.track();
        prefs.settings.track();
        editor.tools.brush_size.track();
        editor.tools.fill.track();
        hover.track();
        draw();
    });
    Effect::new(move |_| {
        editor.track_changes();
        let busy = editor.peek(Session::is_busy);
        if editor.busy.get_untracked() != busy {
            editor.busy.set(busy);
        }
    });
    // The session's gestures follow the tool options.
    Effect::new(move |_| {
let options = Options {
            snap: editor.tools.snap.get(),
            square: editor.tools.square.get(),
            from_centre: editor.tools.from_centre.get(),
            oriented: editor.tools.oriented.get(),
            freehand: editor.tools.freehand.get(),
            spacing: editor.tools.spacing.get(),
            single_point: editor.tools.single_point.get(),
            ellipse_from_centre: editor.tools.ellipse_from_centre.get(),
            edge_snap: editor.tools.edge_snap.get(),
            brush_size: editor.tools.brush_size.get(),
            threshold: editor.tools.threshold.get(),
            stop_at_edges: editor.tools.stop_at_edges.get(),
        };
        editor.update(|session| session.set_options(options));
    });
    // The session acts with the editor's label; a label change drops a stale constrained placement.
    Effect::new(move |_| {
        let label = editor.label.get();
        editor.update(|session| session.set_label(label));
    });
    // Drop a half-drawn shape when the tool or a drawing mode changes.
    Effect::new(move |_| {
        editor.switched.track();
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
            data-drawing={move || {
                editor.track_changes();
                let drawn = editor.peek(|session| match session.preview() {
                    Some(Preview::Path { points, .. } | Preview::Oriented { points, .. } | Preview::Freehand { points, .. }) => points.len(),
                    _ => 0,
                });
                if drawn > 0 {
                    drawn
                } else if editor.peek(Session::is_brushing) {
                    1
                } else {
                    0
                }
            }}
            style:cursor=canvas_cursor
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:pointerleave=move |_| hover.set(None)
            on:dblclick=move |_| {
                editor.key(Action::Finish);
            }
            on:contextmenu=|ev: ev::MouseEvent| ev.prevent_default()
        ></canvas>
    }
}
