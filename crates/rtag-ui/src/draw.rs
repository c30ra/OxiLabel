//! Painting annotations and editing aids on the overlay canvas. Everything takes image coordinates
//! and a `View`, and draws in CSS pixels (the caller sets the device-pixel transform).

use rtag_core::{
    annotation::Shape,
    geometry::{Handle, Point, Rect},
    oriented::OrientedRect,
    view::View,
};
use web_sys::CanvasRenderingContext2d;

pub const HANDLE_SIZE: f64 = 8.0;
/// Screen distance from an oriented rectangle's edge to its rotation handle.
pub const ROTATION_HANDLE_DISTANCE: f64 = 24.0;
const POINT_RADIUS: f64 = 4.0;
const TAG_HEIGHT: f64 = 18.0;

/// How a shape is drawn: selected shapes are stronger, previews dashed.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub color: &'a str,
    pub selected: bool,
    pub dashed: bool,
}

pub fn shape(context: &CanvasRenderingContext2d, view: View, shape: &Shape, look: Look) {
    match shape {
        Shape::Points { points } => {
            for point in points {
                dot(context, view.to_screen(*point), look.color, if look.selected { POINT_RADIUS + 1.0 } else { POINT_RADIUS });
            }
        }
        Shape::Circle { centre, radius } => {
            let at = view.to_screen(*centre);
            context.begin_path();
            context.arc(at.x, at.y, radius * view.scale, 0.0, std::f64::consts::TAU).ok();
            context.set_fill_style_str(look.color);
            context.set_global_alpha(if look.selected { 0.22 } else { 0.12 });
            context.fill();
            context.set_global_alpha(1.0);
            stroke(context, look, if look.selected { 2.5 } else { 1.5 });
        }
        Shape::Polyline { points } => {
            trace(context, view, points, false);
            stroke(context, look, if look.selected { 3.0 } else { 2.0 });
        }
        _ => {
            trace(context, view, &shape.vertices(), true);
            context.set_fill_style_str(look.color);
            context.set_global_alpha(if look.selected { 0.22 } else { 0.12 });
            context.fill();
            context.set_global_alpha(1.0);
            stroke(context, look, if look.selected { 2.5 } else { 1.5 });
        }
    }
}

/// An unfinished polygon, polyline or points shape: the placed vertices and a rubber band to the
/// pointer.
pub fn path_preview(context: &CanvasRenderingContext2d, view: View, points: &[Point], pointer: Point, closed: bool, only_points: bool, color: &str) {
    let look = Look { color, selected: false, dashed: true };
    if !only_points {
        let mut line = points.to_vec();
        line.push(pointer);
        trace(context, view, &line, closed && line.len() > 2);
        stroke(context, look, 1.5);
    }
    for point in points {
        dot(context, view.to_screen(*point), color, 3.5);
    }
}

pub fn oriented_preview(context: &CanvasRenderingContext2d, view: View, rect: OrientedRect, color: &str) {
    shape(context, view, &Shape::OrientedRectangle(rect), Look { color, selected: false, dashed: true });
}

/// The eight resize handles of an axis-aligned rectangle.
pub fn rectangle_handles(context: &CanvasRenderingContext2d, view: View, rect: Rect, color: &str) {
    let screen = Rect { min: view.to_screen(rect.min), max: view.to_screen(rect.max) };
    for handle in Handle::ALL {
        square(context, screen.handle_position(handle), color);
    }
}

/// Square handles on every vertex, and round "+" handles on edge midpoints when vertices can be
/// inserted there.
pub fn vertex_handles(context: &CanvasRenderingContext2d, view: View, shape: &Shape, color: &str, midpoints: bool) {
    if midpoints {
        for (a, b) in shape.edges() {
            let at = view.to_screen(a.midpoint(b));
            context.begin_path();
            context.arc(at.x, at.y, 3.5, 0.0, std::f64::consts::TAU).ok();
            context.set_fill_style_str("#141517");
            context.fill();
            context.set_line_width(1.5);
            context.set_stroke_style_str(color);
            context.stroke();
        }
    }
    for vertex in shape.vertices() {
        square(context, view.to_screen(vertex), color);
    }
}

/// The round handle above an oriented rectangle that turns it, joined to the rectangle by a line.
pub fn rotation_handle(context: &CanvasRenderingContext2d, view: View, rect: OrientedRect, color: &str) {
    let handle = view.to_screen(rect.rotation_handle(ROTATION_HANDLE_DISTANCE / view.scale));
    let edge = view.to_screen(rect.rotation_handle(0.0));
    context.begin_path();
    context.move_to(edge.x, edge.y);
    context.line_to(handle.x, handle.y);
    context.set_line_width(1.5);
    context.set_stroke_style_str(color);
    context.stroke();
    context.begin_path();
    context.arc(handle.x, handle.y, 5.0, 0.0, std::f64::consts::TAU).ok();
    context.set_fill_style_str("#ffffff");
    context.fill();
    context.stroke();
}

/// The round handle on a circle's right edge that changes its radius.
pub fn radius_handle(context: &CanvasRenderingContext2d, view: View, centre: Point, radius: f64, color: &str) {
    let at = view.to_screen(Point::new(centre.x + radius, centre.y));
    context.begin_path();
    context.arc(at.x, at.y, 5.0, 0.0, std::f64::consts::TAU).ok();
    context.set_fill_style_str("#ffffff");
    context.fill();
    context.set_line_width(1.5);
    context.set_stroke_style_str(color);
    context.stroke();
}

/// The brush outline at the pointer: its size in the image, light with a dark edge so it shows
/// on any background.
pub fn brush_outline(context: &CanvasRenderingContext2d, view: View, centre: Point, radius: f64) {
    let at = view.to_screen(centre);
    context.begin_path();
    context.arc(at.x, at.y, (radius * view.scale).max(1.0), 0.0, std::f64::consts::TAU).ok();
    context.set_line_width(3.0);
    context.set_stroke_style_str("#141517");
    context.stroke();
    context.set_line_width(1.5);
    context.set_stroke_style_str("#e8e6e1");
    context.stroke();
}

/// The dashed box around a selected mask.
pub fn mask_frame(context: &CanvasRenderingContext2d, view: View, bounds: Rect, color: &str) {
    let (min, max) = (view.to_screen(bounds.min), view.to_screen(bounds.max));
    context.begin_path();
    context.rect(min.x, min.y, max.x - min.x, max.y - min.y);
    stroke(context, Look { color, selected: true, dashed: true }, 1.5);
}

/// A small filled tag with `text` just above `at` (screen coordinates).
pub fn tag(context: &CanvasRenderingContext2d, at: Point, text: &str, color: &str) {
    if text.is_empty() {
        return;
    }
    context.set_font("500 12px 'IBM Plex Sans', system-ui, sans-serif");
    let width = context.measure_text(text).map(|m| m.width()).unwrap_or(0.0) + 10.0;
    let (x, y) = (at.x, (at.y - TAG_HEIGHT - 2.0).max(0.0));
    context.set_fill_style_str(color);
    context.fill_rect(x, y, width, TAG_HEIGHT);
    context.set_fill_style_str("#141517");
    context.fill_text(text, x + 5.0, y + 13.0).ok();
}

/// The dashed box of a box selection.
pub fn selection_box(context: &CanvasRenderingContext2d, view: View, region: Rect) {
    let (min, max) = (view.to_screen(region.min), view.to_screen(region.max));
    context.set_fill_style_str("#ffffff");
    context.set_global_alpha(0.06);
    context.fill_rect(min.x, min.y, max.x - min.x, max.y - min.y);
    context.set_global_alpha(1.0);
    context.set_stroke_style_str("#e8e6e1");
    context.set_line_width(1.0);
    context.set_line_dash(&dashes(&[4.0, 4.0])).ok();
    context.stroke_rect(min.x, min.y, max.x - min.x, max.y - min.y);
    context.set_line_dash(&dashes(&[])).ok();
}

fn trace(context: &CanvasRenderingContext2d, view: View, points: &[Point], closed: bool) {
    context.begin_path();
    for (i, point) in points.iter().enumerate() {
        let at = view.to_screen(*point);
        if i == 0 {
            context.move_to(at.x, at.y);
        } else {
            context.line_to(at.x, at.y);
        }
    }
    if closed {
        context.close_path();
    }
}

fn stroke(context: &CanvasRenderingContext2d, look: Look, width: f64) {
    context.set_stroke_style_str(look.color);
    context.set_line_width(width);
    context.set_line_dash(&dashes(if look.dashed { &[6.0, 4.0] } else { &[] })).ok();
    context.stroke();
    context.set_line_dash(&dashes(&[])).ok();
}

fn dot(context: &CanvasRenderingContext2d, at: Point, color: &str, radius: f64) {
    context.begin_path();
    context.arc(at.x, at.y, radius, 0.0, std::f64::consts::TAU).ok();
    context.set_fill_style_str(color);
    context.fill();
    context.set_line_width(1.5);
    context.set_stroke_style_str("#141517");
    context.stroke();
}

fn square(context: &CanvasRenderingContext2d, at: Point, color: &str) {
    let (x, y) = (at.x - HANDLE_SIZE / 2.0, at.y - HANDLE_SIZE / 2.0);
    context.set_fill_style_str("#ffffff");
    context.fill_rect(x, y, HANDLE_SIZE, HANDLE_SIZE);
    context.set_line_width(1.5);
    context.set_stroke_style_str(color);
    context.stroke_rect(x, y, HANDLE_SIZE, HANDLE_SIZE);
}

fn dashes(pattern: &[f64]) -> wasm_bindgen::JsValue {
    pattern.iter().map(|value| wasm_bindgen::JsValue::from_f64(*value)).collect::<web_sys::js_sys::Array>().into()
}
