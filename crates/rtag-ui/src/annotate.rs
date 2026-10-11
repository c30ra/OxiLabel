//! Drawing and editing annotations on top of the tile viewer.
//!
//! Select (V) is the default tool: click picks the topmost shape, drag moves it, the handles resize
//! it, Delete or X removes it. Rectangle (R) draws a box from two clicks or one drag; Shift makes a
//! square and Alt draws from the centre. Esc first cancels the shape being drawn, then clears the
//! selection, then returns to Select. Ctrl+Z and Ctrl+Y undo and redo.
//!
//! Holding Q or the right mouse button opens a pie at the cursor: with Select active the tool pie
//! (then the label pie), with a drawing tool its options pie, with a shape selected the label pie
//! to relabel it. Release or flick picks; a quick tap repeats the last tool and label. Shift+Q
//! opens only the label pie.

use std::collections::HashMap;

use leptos::{ev, html, prelude::*};
use rtag_core::{
    Label, ShapeType,
    annotation::{Annotation, Shape, topmost_at},
    geometry::{Handle, Point, Rect},
    history::{Edit, History},
    view::View,
};
use web_sys::CanvasRenderingContext2d;

use crate::{
    pie::{LabelSearch, Pie, PieAction, PieItem, PieMenu, SearchEntry, now},
    viewer::{context_2d, typing_in_a_field},
};
use rtag_core::pie::{Compass, FLICK_DISTANCE, is_tap};

/// How far from a handle or a shape (in screen pixels) a click still hits it.
const HANDLE_REACH: f64 = 8.0;
const PICK_MARGIN: f64 = 3.0;
/// A press that moves less than this (in screen pixels) is a click, not a drag.
const DRAG_THRESHOLD: f64 = 4.0;
/// Shapes smaller than this on screen are treated as accidental and not created.
const MIN_SHAPE_SIZE: f64 = 3.0;
const HANDLE_SIZE: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
}

impl Tool {
    /// The shape this tool draws; `None` for Select.
    pub fn shape_type(self) -> Option<ShapeType> {
        match self {
            Tool::Select => None,
            Tool::Rectangle => Some(ShapeType::Rectangle),
        }
    }
}

/// The annotations of the open image and everything that changes them. All changes go through
/// the undo history.
#[derive(Clone, Copy)]
pub struct Editor {
    pub annotations: RwSignal<Vec<Annotation>>,
    history: StoredValue<History>,
    pub selected: RwSignal<Option<usize>>,
    pub tool: RwSignal<Tool>,
    /// The label new shapes get.
    pub label: RwSignal<Option<i64>>,
    /// Increases with every edit, undo and redo; autosave compares it with the last saved one.
    pub revision: RwSignal<u64>,
    /// A short hint for the user, e.g. why a shape could not be drawn.
    pub notice: RwSignal<Option<String>>,
    /// Rectangle options from the options pie: always square, always from the centre. Shift and
    /// Alt invert them while drawing.
    pub square: RwSignal<bool>,
    pub from_centre: RwSignal<bool>,
    /// The last drawing tool and label, repeated by a tap of the pie key.
    pub last: RwSignal<Option<(Tool, i64)>>,
}

impl Editor {
    pub fn new(annotations: Vec<Annotation>, label: Option<i64>) -> Self {
        Editor {
            annotations: RwSignal::new(annotations),
            history: StoredValue::new(History::default()),
            selected: RwSignal::new(None),
            tool: RwSignal::new(Tool::Select),
            label: RwSignal::new(label),
            revision: RwSignal::new(0),
            notice: RwSignal::new(None),
            square: RwSignal::new(false),
            from_centre: RwSignal::new(false),
            last: RwSignal::new(None),
        }
    }

    /// Replaces everything with `annotations` as loaded from the server; nothing to undo.
    pub fn reset(&self, annotations: Vec<Annotation>) {
        self.history.set_value(History::default());
        self.selected.set(None);
        self.annotations.set(annotations);
    }

    pub fn apply(&self, edit: Edit) {
        self.annotations.update(|list| {
            self.history
                .update_value(|history| history.apply(list, edit))
        });
        self.revision.update(|r| *r += 1);
    }

    pub fn undo(&self) {
        let mut undone = false;
        self.annotations.update(|list| {
            self.history
                .update_value(|history| undone = history.undo(list))
        });
        if undone {
            self.selected.set(None);
            self.revision.update(|r| *r += 1);
        }
    }

    pub fn redo(&self) {
        let mut redone = false;
        self.annotations.update(|list| {
            self.history
                .update_value(|history| redone = history.redo(list))
        });
        if redone {
            self.selected.set(None);
            self.revision.update(|r| *r += 1);
        }
    }

    pub fn delete_selected(&self) {
        let Some(index) = self.selected.get_untracked() else {
            return;
        };
        let Some(annotation) = self
            .annotations
            .with_untracked(|list| list.get(index).cloned())
        else {
            return;
        };
        self.selected.set(None);
        self.apply(Edit::Remove { index, annotation });
    }
}

/// What the pointer is doing right now.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    Idle,
    /// A rectangle from `start`; `dragged` is false while waiting for the second click.
    Drawing {
        start: Point,
        current: Point,
        dragged: bool,
        square: bool,
        from_centre: bool,
    },
    Moving {
        index: usize,
        grab: Point,
        before: Rect,
        current: Rect,
    },
    Resizing {
        index: usize,
        handle: Handle,
        before: Rect,
        current: Rect,
    },
}

#[component]
pub fn AnnotationLayer(
    editor: Editor,
    labels: Vec<Label>,
    image_size: (u32, u32),
    view: RwSignal<Option<View>>,
    size: RwSignal<(f64, f64)>,
    space_held: RwSignal<bool>,
) -> impl IntoView {
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let gesture = RwSignal::new(Gesture::Idle);
    let cursor = RwSignal::new("default");
    let colors: HashMap<i64, String> = labels
        .iter()
        .map(|label| (label.id, label.color.clone()))
        .collect();
    let names: HashMap<i64, String> = labels
        .iter()
        .map(|label| (label.id, label.name.clone()))
        .collect();
    let label_names = names.clone();
    let hotkeys: Vec<(String, i64)> = labels
        .iter()
        .filter_map(|label| label.hotkey.clone().map(|key| (key, label.id)))
        .collect();
    let rectangle_labels: Vec<i64> = labels
        .iter()
        .filter(|label| label.shape_types.contains(&ShapeType::Rectangle))
        .map(|label| label.id)
        .collect();
    let (width, height) = (f64::from(image_size.0), f64::from(image_size.1));
    let inside =
        move |point: Point| Point::new(point.x.clamp(0.0, width), point.y.clamp(0.0, height));

    // Pointer position in canvas pixels and in image coordinates.
    let locate = move |ev: &ev::MouseEvent| -> Option<(Point, Point, View)> {
        let canvas = canvas_ref.get_untracked()?;
        let current = view.get_untracked()?;
        let rect = canvas.get_bounding_client_rect();
        let screen = Point::new(
            f64::from(ev.client_x()) - rect.left(),
            f64::from(ev.client_y()) - rect.top(),
        );
        Some((screen, current.to_image(screen), current))
    };
    let rectangle_of = move |index: usize| {
        editor
            .annotations
            .with_untracked(|list| list.get(index).map(|annotation| match annotation.shape {
                Shape::Rectangle(rect) => rect,
            }))
    };

    let finish_drawing =
        move |start: Point, end: Point, square: bool, from_centre: bool, scale: f64| {
            gesture.set(Gesture::Idle);
            let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
            let Some(label) = editor.label.get_untracked() else {
                return;
            };
            if rect.width() * scale < MIN_SHAPE_SIZE || rect.height() * scale < MIN_SHAPE_SIZE {
                return;
            }
            let index = editor.annotations.with_untracked(Vec::len);
            editor.apply(Edit::Add {
                index,
                annotation: Annotation::manual(label, Shape::Rectangle(rect)),
            });
        };

    // Pie menus. `pending_tool` waits for its label after the tool pie; `relabel` is the shape
    // whose label the label pie changes.
    let pie = RwSignal::new(None::<Pie>);
    let pointer = RwSignal::new(Point::new(0.0, 0.0));
    let cursor_at = StoredValue::new(Point::new(0.0, 0.0));
    let search = RwSignal::new(None::<Point>);
    let pending_tool = StoredValue::new(None::<Tool>);
    let relabel = StoredValue::new(None::<usize>);
    let all_labels = StoredValue::new({
        let mut sorted = labels.clone();
        sorted.sort_by_key(|label| label.id);
        sorted
    });
    let shape_type_of = move |index: usize| {
        editor
            .annotations
            .with_untracked(|list| list.get(index).map(|annotation| annotation.shape.shape_type()))
    };
    // The shape the next label is for: the one being relabelled, the tool just picked, or the
    // active tool's. `None` allows every label.
    let target_shape = move || match (relabel.get_value(), pending_tool.get_value()) {
        (Some(index), _) => shape_type_of(index),
        (None, Some(tool)) => tool.shape_type(),
        (None, None) => editor.tool.get_untracked().shape_type(),
    };
    let allows = |label: &Label, shape: Option<ShapeType>| shape.is_none_or(|shape| label.shape_types.contains(&shape));

    let tool_pie = move |centre: Point, held: bool| {
        let current = editor.tool.get_untracked();
        let item = |slot, name: &str, key: &str, tool: Option<Tool>| PieItem {
            slot,
            name: name.to_owned(),
            key: Some(key.to_owned()),
            swatch: None,
            enabled: tool.is_some(),
            current: tool == Some(current),
            action: tool.map_or(PieAction::Keep, PieAction::Tool),
        };
        // The approved layout; tools of later tickets keep their places, disabled.
        let items = vec![
            item(Compass::N, "Select", "V", Some(Tool::Select)),
            item(Compass::NE, "Rectangle", "R", Some(Tool::Rectangle)),
            item(Compass::E, "Polygon", "P", None),
            item(Compass::SE, "Points", ".", None),
            item(Compass::S, "Polyline", "L", None),
            item(Compass::SW, "Brush", "B", None),
            item(Compass::W, "Eraser", "E", None),
            item(Compass::NW, "Tag", "T", None),
        ];
        Pie { label: "Tools", items, centre, wide: false, held, opened_at: now() }
    };
    let label_pie = move |centre: Point, held: bool| {
        let shape = target_shape();
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
                    enabled: allows(label, shape),
                    current: current == Some(label.id),
                    action: PieAction::Label(label.id),
                })
                .collect()
        });
        if all_labels.with_value(|labels| labels.len() > slots.len()) {
            items.push(PieItem {
                slot: Compass::NW,
                name: "More…".to_owned(),
                key: Some("/".to_owned()),
                swatch: None,
                enabled: true,
                current: false,
                action: PieAction::MoreLabels,
            });
        }
        Pie { label: "Labels", items, centre, wide: false, held, opened_at: now() }
    };
    let options_pie = move |centre: Point, held: bool| {
        let option = |slot, name: &str, key: Option<&str>, enabled, current, action| PieItem {
            slot,
            name: name.to_owned(),
            key: key.map(str::to_owned),
            swatch: None,
            enabled,
            current,
            action,
        };
        let items = vec![
            option(Compass::N, "2-point", None, true, true, PieAction::Keep),
            option(Compass::E, "3-point oriented", None, false, false, PieAction::Keep),
            option(Compass::S, "From center", Some("Alt"), true, editor.from_centre.get_untracked(), PieAction::ToggleFromCentre),
            option(Compass::W, "Square", Some("Shift"), true, editor.square.get_untracked(), PieAction::ToggleSquare),
        ];
        Pie { label: "Rectangle options", items, centre, wide: true, held, opened_at: now() }
    };

    let close_pie = move || {
        pie.set(None);
        search.set(None);
        pending_tool.set_value(None);
        relabel.set_value(None);
    };
    let open_pie = move |held: bool, labels_only: bool| {
        close_pie();
        let centre = cursor_at.get_value();
        let selected = editor.selected.get_untracked().filter(|_| editor.tool.get_untracked() == Tool::Select);
        let opened = if labels_only || selected.is_some() {
            relabel.set_value(selected);
            label_pie(centre, held)
        } else if editor.tool.get_untracked() == Tool::Select {
            tool_pie(centre, held)
        } else {
            options_pie(centre, held)
        };
        pointer.set(centre);
        pie.set(Some(opened));
    };
    let relabel_shape = move |index: usize, id: i64| {
        let Some(old) = editor.annotations.with_untracked(|list| list.get(index).cloned()) else { return };
        if old.label != id {
            let new = Annotation { label: id, ..old.clone() };
            editor.apply(Edit::Replace { index, before: old, after: new });
        }
    };
    let finish_label = move |id: i64| {
        let (relabelling, tool) = (relabel.get_value(), pending_tool.get_value());
        let shape = target_shape();
        let usable = all_labels.with_value(|labels| labels.iter().any(|label| label.id == id && allows(label, shape)));
        close_pie();
        if !usable {
            return;
        }
        match (relabelling, tool) {
            (Some(index), _) => relabel_shape(index, id),
            (None, Some(tool)) => {
                editor.tool.set(tool);
                editor.label.set(Some(id));
            }
            (None, None) => editor.label.set(Some(id)),
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
            // Picking a tool opens the label pie in the same spot.
            PieAction::Tool(tool) => {
                pending_tool.set_value(Some(tool));
                pie.set(Some(label_pie(centre, false)));
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
            PieAction::Keep => close_pie(),
        }
    };
    // Releasing the pie key or button: a tap repeats the last tool and label (or leaves the pie
    // open for clicking), otherwise the highlighted item is picked.
    let release = move || {
        let Some(open) = pie.get_untracked().filter(|open| open.held) else { return };
        let at = pointer.get_untracked();
        if is_tap(now() - open.opened_at, at.x - open.centre.x, at.y - open.centre.y) {
            match editor.last.get_untracked() {
                Some((tool, label)) if open.label == "Tools" => {
                    close_pie();
                    editor.tool.set(tool);
                    editor.label.set(Some(label));
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
        let shape = target_shape();
        all_labels.with_value(|labels| {
            labels
                .iter()
                .map(|label| SearchEntry {
                    id: label.id,
                    name: label.name.clone(),
                    color: label.color.clone(),
                    key: label.hotkey.clone(),
                    enabled: allows(label, shape),
                })
                .collect::<Vec<_>>()
        })
    };
    // Remember the last drawing tool and label for a tap of the pie key.
    Effect::new(move |_| {
        let tool = editor.tool.get();
        if let (true, Some(label)) = (tool != Tool::Select, editor.label.get()) {
            editor.last.set(Some((tool, label)));
        }
    });
    let pointer_moves = window_event_listener(ev::pointermove, move |ev| {
        let at = Point::new(f64::from(ev.client_x()), f64::from(ev.client_y()));
        cursor_at.set_value(at);
        let Some(open) = pie.get_untracked() else { return };
        pointer.set(at);
        // Flick: moving far enough while still holding picks at once.
        if open.held
            && (at.x - open.centre.x).hypot(at.y - open.centre.y) >= FLICK_DISTANCE
            && let Some(action) = open.highlighted(at).map(|item| item.action.clone())
        {
            choose(action);
        }
    });
    let pointer_ups = window_event_listener(ev::pointerup, move |ev| {
        if ev.button() == 2 {
            release();
        }
    });
    let key_ups = window_event_listener(ev::keyup, move |ev| {
        if ev.key().eq_ignore_ascii_case("q") {
            release();
        }
    });
    on_cleanup(move || {
        pointer_moves.remove();
        pointer_ups.remove();
        key_ups.remove();
    });

    let on_pointer_down = move |ev: ev::PointerEvent| {
        // The right button opens the pie, like holding Q.
        if ev.button() == 2 {
            ev.prevent_default();
            cursor_at.set_value(Point::new(f64::from(ev.client_x()), f64::from(ev.client_y())));
            open_pie(true, false);
            return;
        }
        // Middle button and Space+drag belong to the viewer (pan); let them bubble up.
        if ev.button() != 0 || space_held.get_untracked() {
            return;
        }
        let Some((_, point, current)) = locate(&ev) else {
            return;
        };
        if let Some(canvas) = canvas_ref.get_untracked() {
            canvas.set_pointer_capture(ev.pointer_id()).ok();
        }
        match editor.tool.get_untracked() {
            Tool::Rectangle => match gesture.get_untracked() {
                Gesture::Drawing {
                    start,
                    square,
                    from_centre,
                    ..
                } => finish_drawing(start, inside(point), square, from_centre, current.scale),
                _ if editor.label.get_untracked().is_none() => {
                    editor.notice.set(Some(
                        "Add a label that allows rectangles on the Labels tab first.".into(),
                    ));
                }
                _ => {
                    let start = inside(point);
                    gesture.set(Gesture::Drawing {
                        start,
                        current: start,
                        dragged: false,
                        square: ev.shift_key() != editor.square.get_untracked(),
                        from_centre: ev.alt_key() != editor.from_centre.get_untracked(),
                    });
                }
            },
            Tool::Select => {
                let reach = HANDLE_REACH / current.scale;
                if let Some(index) = editor.selected.get_untracked()
                    && let Some(before) = rectangle_of(index)
                    && let Some(handle) = before.handle_at(point, reach)
                {
                    gesture.set(Gesture::Resizing {
                        index,
                        handle,
                        before,
                        current: before,
                    });
                    return;
                }
                let hit = editor
                    .annotations
                    .with_untracked(|list| topmost_at(list, point, PICK_MARGIN / current.scale));
                editor.selected.set(hit);
                if let Some(index) = hit
                    && let Some(before) = rectangle_of(index)
                {
                    gesture.set(Gesture::Moving {
                        index,
                        grab: point,
                        before,
                        current: before,
                    });
                }
            }
        }
    };

    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((screen, point, current)) = locate(&ev) else {
            return;
        };
        match gesture.get_untracked() {
            Gesture::Drawing { start, dragged, .. } => {
                let start_screen = current.to_screen(start);
                let moved =
                    (screen.x - start_screen.x).hypot(screen.y - start_screen.y) > DRAG_THRESHOLD;
                gesture.set(Gesture::Drawing {
                    start,
                    current: inside(point),
                    dragged: dragged || (moved && ev.buttons() & 1 == 1),
                    square: ev.shift_key() != editor.square.get_untracked(),
                    from_centre: ev.alt_key() != editor.from_centre.get_untracked(),
                });
            }
            Gesture::Moving {
                index,
                grab,
                before,
                ..
            } => {
                let current =
                    before.moved_within(point.x - grab.x, point.y - grab.y, width, height);
                gesture.set(Gesture::Moving {
                    index,
                    grab,
                    before,
                    current,
                });
            }
            Gesture::Resizing {
                index,
                handle,
                before,
                ..
            } => {
                gesture.set(Gesture::Resizing {
                    index,
                    handle,
                    before,
                    current: before.resized(handle, inside(point)),
                });
            }
            Gesture::Idle => cursor.set(hover_cursor(editor, point, current, rectangle_of)),
        }
    };

    let on_pointer_up = move |ev: ev::PointerEvent| {
        let Some((_, _, current)) = locate(&ev) else {
            return;
        };
        match gesture.get_untracked() {
            // A click without dragging leaves the rectangle open for a second click.
            Gesture::Drawing {
                start,
                current: end,
                dragged: true,
                square,
                from_centre,
            } => finish_drawing(start, end, square, from_centre, current.scale),
            Gesture::Moving {
                index,
                before,
                current: after,
                ..
            }
            | Gesture::Resizing {
                index,
                before,
                current: after,
                ..
            } => {
                gesture.set(Gesture::Idle);
                let big_enough = after.width() * current.scale >= MIN_SHAPE_SIZE
                    && after.height() * current.scale >= MIN_SHAPE_SIZE;
                if after != before && big_enough {
                    let Some(old) = editor
                        .annotations
                        .with_untracked(|list| list.get(index).cloned())
                    else {
                        return;
                    };
                    let new = Annotation {
                        shape: Shape::Rectangle(after),
                        ..old.clone()
                    };
                    editor.apply(Edit::Replace {
                        index,
                        before: old,
                        after: new,
                    });
                }
            }
            _ => {}
        }
    };

    let keys = window_event_listener(ev::keydown, move |ev| {
        if typing_in_a_field()
            || document()
                .query_selector(".backdrop")
                .ok()
                .flatten()
                .is_some()
        {
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
        if key.eq_ignore_ascii_case("q") && !command && !ev.alt_key() {
            if !ev.repeat() {
                // Shift+Q opens only the label pie, and it stays open for a click.
                open_pie(!ev.shift_key(), ev.shift_key());
            }
            ev.prevent_default();
            return;
        }
        match key.as_str() {
            "z" | "Z" if command && ev.shift_key() => editor.redo(),
            "z" | "Z" if command => editor.undo(),
            "y" | "Y" if command => editor.redo(),
            _ if command || ev.alt_key() => return,
            "Escape" => {
                if gesture.get_untracked() != Gesture::Idle {
                    gesture.set(Gesture::Idle);
                } else if editor.selected.get_untracked().is_some() {
                    editor.selected.set(None);
                } else {
                    editor.tool.set(Tool::Select);
                }
            }
            "Delete" | "Backspace" | "x" | "X" => editor.delete_selected(),
            "v" | "V" => {
                gesture.set(Gesture::Idle);
                editor.tool.set(Tool::Select);
            }
            "r" | "R" => {
                gesture.set(Gesture::Idle);
                editor.tool.set(Tool::Rectangle);
            }
            _ => match hotkeys
                .iter()
                .find(|(hotkey, _)| hotkey.eq_ignore_ascii_case(&key))
            {
                Some((_, id)) if rectangle_labels.contains(id) => editor.label.set(Some(*id)),
                Some((_, id)) => editor.notice.set(Some(format!(
                    "“{}” cannot be used for rectangles.",
                    label_names.get(id).cloned().unwrap_or_default()
                ))),
                None => return,
            },
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let draw = move || {
        let (Some(canvas), Some(current)) = (canvas_ref.get_untracked(), view.get_untracked())
        else {
            return;
        };
        let ratio = window().device_pixel_ratio();
        let (css_width, css_height) = size.get_untracked();
        let (pixel_width, pixel_height) = (
            (css_width * ratio).round() as u32,
            (css_height * ratio).round() as u32,
        );
        if canvas.width() != pixel_width || canvas.height() != pixel_height {
            canvas.set_width(pixel_width);
            canvas.set_height(pixel_height);
        }
        let Some(context) = context_2d(&canvas) else {
            return;
        };
        context.set_transform(ratio, 0.0, 0.0, ratio, 0.0, 0.0).ok();
        context.clear_rect(0.0, 0.0, css_width, css_height);

        let gesture = gesture.get_untracked();
        let selected = editor.selected.get_untracked();
        let color_of = |label: i64| {
            colors
                .get(&label)
                .cloned()
                .unwrap_or_else(|| "#ffffff".to_owned())
        };
        editor.annotations.with_untracked(|list| {
            for (index, annotation) in list.iter().enumerate() {
                let Shape::Rectangle(stored) = &annotation.shape;
                let rect = match gesture {
                    Gesture::Moving {
                        index: moving,
                        current,
                        ..
                    }
                    | Gesture::Resizing {
                        index: moving,
                        current,
                        ..
                    } if moving == index => current,
                    _ => *stored,
                };
                let color = color_of(annotation.label);
                let screen = to_screen_rect(current, rect);
                stroke_rect(&context, screen, &color, selected == Some(index), false);
                if selected == Some(index) {
                    draw_handles(&context, screen, &color);
                    let name = names.get(&annotation.label).cloned().unwrap_or_default();
                    draw_tag(&context, screen, &name, &color);
                }
            }
        });
        if let Gesture::Drawing {
            start,
            current: end,
            square,
            from_centre,
            ..
        } = gesture
        {
            let rect = Rect::from_drag(start, end, square, from_centre).clamped(width, height);
            let color = editor
                .label
                .get_untracked()
                .map(color_of)
                .unwrap_or_else(|| "#ffffff".to_owned());
            let screen = to_screen_rect(current, rect);
            stroke_rect(&context, screen, &color, false, true);
            draw_tag(
                &context,
                screen,
                &format!("{:.0} × {:.0}", rect.width(), rect.height()),
                &color,
            );
        }
    };
    Effect::new(move |_| {
        view.track();
        size.track();
        gesture.track();
        editor.annotations.track();
        editor.selected.track();
        editor.label.track();
        draw();
    });
    // Drop a half-drawn shape when the tool changes.
    Effect::new(move |_| {
        editor.tool.track();
        gesture.set(Gesture::Idle);
    });

    let canvas_cursor = move || match editor.tool.get() {
        Tool::Rectangle => "crosshair",
        Tool::Select => cursor.get(),
    };
    view! {
        <PieMenu pie=pie pointer=pointer on_pick=choose on_close=close_pie />
        {move || search.get().map(|at| view! {
            <LabelSearch at=at entries=search_entries() on_pick=finish_label on_close=close_pie />
        })}
        <canvas
            class="annotations"
            node_ref=canvas_ref
            // Editor state for browser tests; not used for rendering.
            data-count=move || editor.annotations.with(Vec::len)
            data-selected=move || editor.selected.get().map(|i| i.to_string()).unwrap_or_default()
            data-revision=move || editor.revision.get()
            style:cursor=canvas_cursor
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:contextmenu=|ev: ev::MouseEvent| ev.prevent_default()
        ></canvas>
    }
}

/// The cursor for hovering at `point` with the Select tool: a resize arrow over the selected
/// shape's handles, a move cross over a shape, the arrow elsewhere.
fn hover_cursor(
    editor: Editor,
    point: Point,
    view: View,
    rectangle_of: impl Fn(usize) -> Option<Rect>,
) -> &'static str {
    if let Some(index) = editor.selected.get_untracked()
        && let Some(rect) = rectangle_of(index)
        && let Some(handle) = rect.handle_at(point, HANDLE_REACH / view.scale)
    {
        return handle.cursor();
    }
    let over_shape = editor
        .annotations
        .with_untracked(|list| topmost_at(list, point, PICK_MARGIN / view.scale).is_some());
    if over_shape { "move" } else { "default" }
}

fn to_screen_rect(view: View, rect: Rect) -> Rect {
    Rect {
        min: view.to_screen(rect.min),
        max: view.to_screen(rect.max),
    }
}

fn stroke_rect(
    context: &CanvasRenderingContext2d,
    rect: Rect,
    color: &str,
    selected: bool,
    dashed: bool,
) {
    context.set_fill_style_str(color);
    context.set_global_alpha(if selected { 0.22 } else { 0.12 });
    context.fill_rect(rect.min.x, rect.min.y, rect.width(), rect.height());
    context.set_global_alpha(1.0);
    context.set_stroke_style_str(color);
    context.set_line_width(if selected { 2.5 } else { 1.5 });
    let dashes = js_dashes(if dashed { &[6.0, 4.0] } else { &[] });
    context.set_line_dash(&dashes).ok();
    context.stroke_rect(rect.min.x, rect.min.y, rect.width(), rect.height());
    context.set_line_dash(&js_dashes(&[])).ok();
}

fn draw_handles(context: &CanvasRenderingContext2d, rect: Rect, color: &str) {
    context.set_line_width(1.5);
    for handle in Handle::ALL {
        let at = rect.handle_position(handle);
        let (x, y) = (at.x - HANDLE_SIZE / 2.0, at.y - HANDLE_SIZE / 2.0);
        context.set_fill_style_str("#ffffff");
        context.fill_rect(x, y, HANDLE_SIZE, HANDLE_SIZE);
        context.set_stroke_style_str(color);
        context.stroke_rect(x, y, HANDLE_SIZE, HANDLE_SIZE);
    }
}

/// A small filled tag with `text` above the rectangle's top-left corner.
fn draw_tag(context: &CanvasRenderingContext2d, rect: Rect, text: &str, color: &str) {
    if text.is_empty() {
        return;
    }
    context.set_font("500 12px 'IBM Plex Sans', system-ui, sans-serif");
    let width = context.measure_text(text).map(|m| m.width()).unwrap_or(0.0) + 10.0;
    let (x, y) = (rect.min.x, (rect.min.y - 20.0).max(0.0));
    context.set_fill_style_str(color);
    context.fill_rect(x, y, width, 18.0);
    context.set_fill_style_str("#141517");
    context.fill_text(text, x + 5.0, y + 13.0).ok();
}

fn js_dashes(dashes: &[f64]) -> wasm_bindgen::JsValue {
    dashes
        .iter()
        .map(|value| wasm_bindgen::JsValue::from_f64(*value))
        .collect::<web_sys::js_sys::Array>()
        .into()
}
