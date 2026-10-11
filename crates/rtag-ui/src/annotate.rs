//! Drawing and editing annotations on top of the tile viewer.
//!
//! Select (V) is the default tool: click picks the topmost shape, drag moves it, the handles resize
//! it, Delete or X removes it. Rectangle (R) draws a box from two clicks or one drag; Shift makes a
//! square and Alt draws from the centre. Esc first cancels the shape being drawn, then clears the
//! selection, then returns to Select. Ctrl+Z and Ctrl+Y undo and redo.

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

use crate::viewer::{context_2d, typing_in_a_field};

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
            editor.selected.set(Some(index));
        };

    let on_pointer_down = move |ev: ev::PointerEvent| {
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
                        square: ev.shift_key(),
                        from_centre: ev.alt_key(),
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
                    square: ev.shift_key(),
                    from_centre: ev.alt_key(),
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
