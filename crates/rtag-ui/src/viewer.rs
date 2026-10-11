//! Canvas that shows one image through its tile pyramid. Coarse tiles are drawn first and finer
//! ones on top as they arrive, so the image appears at once and sharpens while zooming in.
//!
//! Navigation follows image editors: scroll to zoom around the cursor, Space-drag or middle-drag
//! to pan, Home to fit, + and − to zoom around the centre.

use std::collections::HashMap;

use leptos::{ev, html, prelude::*};
use rtag_core::{
    Image,
    geometry::Point,
    pyramid::Pyramid,
    view::View,
};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

/// Free space kept around the image when it is fitted to the canvas, in CSS pixels.
pub const FIT_MARGIN: f64 = 24.0;
const BACKGROUND: &str = "#0E0E0F";
/// Scroll distance (in pixels) that doubles or halves the zoom is about 460 px.
const WHEEL_ZOOM_SPEED: f64 = 0.0015;
const KEY_ZOOM_STEP: f64 = 1.25;

type TileKey = (u32, u32, u32);

/// `view` is `None` until the canvas has a size; setting it to `None` fits the image again.
/// `size` reports the canvas size in CSS pixels.
#[component]
pub fn TileViewer(image: Image, view: RwSignal<Option<View>>, size: RwSignal<(f64, f64)>) -> impl IntoView {
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let pyramid = Pyramid::new(image.width, image.height);
    let image_size = (image.width, image.height);
    let id = image.id;
    let tiles = StoredValue::new_local(HashMap::<TileKey, HtmlImageElement>::new());
    // Bumped whenever a tile finishes loading, which triggers a redraw.
    let loaded = RwSignal::new(0_u32);
    let space_held = RwSignal::new(false);
    let dragging = RwSignal::new(None::<(f64, f64)>);

    let request_tile = move |key: TileKey| {
        let Ok(element) = HtmlImageElement::new() else { return };
        let on_load = Closure::once_into_js(move || loaded.update(|n| *n = n.wrapping_add(1)));
        element.set_onload(Some(on_load.unchecked_ref()));
        let (level, x, y) = key;
        element.set_src(&format!("/api/images/{id}/tiles/{level}/{x}/{y}"));
        tiles.update_value(|tiles| {
            tiles.insert(key, element);
        });
    };

    let draw = move || {
        let (Some(canvas), Some(current)) = (canvas_ref.get_untracked(), view.get_untracked()) else { return };
        let Some(context) = context_2d(&canvas) else { return };
        let ratio = window().device_pixel_ratio();
        context.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0).ok();
        context.set_fill_style_str(BACKGROUND);
        context.fill_rect(0.0, 0.0, f64::from(canvas.width()), f64::from(canvas.height()));
        // Past 100% show crisp pixels, as image editors do; below it, smooth downscaling.
        context.set_image_smoothing_enabled(current.scale * ratio < 1.0);

        let visible = current.visible(size.get_untracked());
        let target = pyramid.level_for_scale(current.scale * ratio);
        let coarsest = pyramid.levels() - 1;
        for level in (target..=coarsest).rev() {
            let scale = Pyramid::level_scale(level);
            for (column, row) in pyramid.tiles_in(level, visible) {
                let key = (level, column, row);
                let Some(element) = tiles.with_value(|tiles| tiles.get(&key).cloned()) else {
                    // Only the target level and the always-useful coarsest level are fetched.
                    if level == target || level == coarsest {
                        request_tile(key);
                    }
                    continue;
                };
                if !element.complete() || element.natural_width() == 0 {
                    continue;
                }
                let bounds = pyramid.tile_bounds(level, column, row).expect("listed tiles exist");
                // The last level pixel of an edge may cover less than `scale` image pixels.
                let image_x = |level_x: u32| (f64::from(level_x) * scale).min(f64::from(image_size.0));
                let image_y = |level_y: u32| (f64::from(level_y) * scale).min(f64::from(image_size.1));
                let top_left = current.to_screen(Point::new(image_x(bounds.x), image_y(bounds.y)));
                let bottom_right =
                    current.to_screen(Point::new(image_x(bounds.x + bounds.width), image_y(bounds.y + bounds.height)));
                // Whole device pixels, so neighbouring tiles meet without seams.
                let (x0, y0) = ((top_left.x * ratio).round(), (top_left.y * ratio).round());
                let (x1, y1) = ((bottom_right.x * ratio).round(), (bottom_right.y * ratio).round());
                context
                    .draw_image_with_html_image_element_and_dw_and_dh(&element, x0, y0, x1 - x0, y1 - y0)
                    .ok();
            }
        }
    };

    // Redraw whenever the view, the size or the loaded tiles change. Effects run once per batch of
    // signal changes, so a burst of wheel events or tile loads does not draw more than needed.
    Effect::new(move |_| {
        view.track();
        size.track();
        loaded.track();
        draw();
    });

    // Match the canvas backing store to its displayed size, and fit the image when there is no view yet.
    let resize = move || {
        let Some(canvas) = canvas_ref.get_untracked() else { return };
        let ratio = window().device_pixel_ratio();
        let (width, height) = (f64::from(canvas.client_width()), f64::from(canvas.client_height()));
        canvas.set_width((width * ratio).round() as u32);
        canvas.set_height((height * ratio).round() as u32);
        size.set((width, height));
    };
    Effect::new(move |_| {
        if canvas_ref.get().is_some() {
            resize();
        }
    });
    Effect::new(move |_| {
        let size = size.get();
        if view.read().is_none() && size.0 > 0.0 {
            view.set(Some(View::fit(image_size, size, FIT_MARGIN)));
        }
    });
    let on_resize = window_event_listener(ev::resize, move |_| resize());

    let zoom_at_centre = move |factor: f64| {
        let (width, height) = size.get_untracked();
        view.update(|view| *view = view.map(|v| v.zoom_at(Point::new(width / 2.0, height / 2.0), factor)));
    };
    let on_key_down = window_event_listener(ev::keydown, move |ev| {
        if typing_in_a_field() || ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            return;
        }
        match ev.key().as_str() {
            " " => space_held.set(true),
            "Home" => view.set(None),
            "+" | "=" => zoom_at_centre(KEY_ZOOM_STEP),
            "-" => zoom_at_centre(1.0 / KEY_ZOOM_STEP),
            _ => return,
        }
        ev.prevent_default();
    });
    let on_key_up = window_event_listener(ev::keyup, move |ev| {
        if ev.key() == " " {
            space_held.set(false);
        }
    });
    on_cleanup(move || {
        on_resize.remove();
        on_key_down.remove();
        on_key_up.remove();
    });

    let on_wheel = move |ev: ev::WheelEvent| {
        ev.prevent_default();
        let pixels = match ev.delta_mode() {
            1 => ev.delta_y() * 40.0,  // lines
            2 => ev.delta_y() * 800.0, // pages
            _ => ev.delta_y(),
        };
        let anchor = Point::new(f64::from(ev.offset_x()), f64::from(ev.offset_y()));
        let factor = (-pixels * WHEEL_ZOOM_SPEED).exp();
        view.update(|view| *view = view.map(|v| v.zoom_at(anchor, factor)));
    };
    let on_pointer_down = move |ev: ev::PointerEvent| {
        let pans = ev.button() == 1 || (ev.button() == 0 && space_held.get_untracked());
        if !pans {
            return;
        }
        ev.prevent_default();
        if let Some(canvas) = canvas_ref.get_untracked() {
            canvas.set_pointer_capture(ev.pointer_id()).ok();
        }
        dragging.set(Some((f64::from(ev.client_x()), f64::from(ev.client_y()))));
    };
    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((last_x, last_y)) = dragging.get_untracked() else { return };
        let (x, y) = (f64::from(ev.client_x()), f64::from(ev.client_y()));
        view.update(|view| *view = view.map(|v| v.pan(x - last_x, y - last_y)));
        dragging.set(Some((x, y)));
    };
    let stop_drag = move |_: ev::PointerEvent| dragging.set(None);

    view! {
        <canvas
            class="viewer"
            class:pan-ready=move || space_held.get()
            class:panning=move || dragging.get().is_some()
            node_ref=canvas_ref
            aria-label=format!("Image {}", image.path)
            on:wheel=on_wheel
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=stop_drag
            on:pointercancel=stop_drag
            // Middle-click would otherwise start autoscroll or paste.
            on:mousedown=|ev: ev::MouseEvent| if ev.button() == 1 { ev.prevent_default() }
        ></canvas>
    }
}

fn context_2d(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    // `getContext("2d")` always returns a 2D context; skip the `instanceof` check, which fails in
    // some headless engines (Obscura) whose context class has another name.
    Some(canvas.get_context("2d").ok().flatten()?.unchecked_into())
}

fn typing_in_a_field() -> bool {
    document()
        .active_element()
        .is_some_and(|element| matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"))
}
