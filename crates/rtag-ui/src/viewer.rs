//! Canvas that shows one image through its tile pyramid. Coarse tiles are drawn first and finer
//! ones on top as they arrive, so the image appears at once and sharpens while zooming in.
//!
//! Navigation follows image editors: scroll to zoom around the cursor, Space-drag or middle-drag
//! to pan, Home to fit, + and − to zoom around the centre.
//!
//! With an enhancement filter the tiles are the filtered pyramid's. While a new filter's tiles
//! load, the previous filter's stay on screen.

use std::collections::HashMap;

use leptos::{ev, html, prelude::*};
use rtag_core::{Image, geometry::Point, keymap::Action, pyramid::Pyramid, view::View};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

use crate::prefs::{prefs, press};

/// Free space kept around the image when it is fitted to the canvas, in CSS pixels.
pub const FIT_MARGIN: f64 = 24.0;
const BACKGROUND: &str = "#0E0E0F";
/// Scroll distance (in pixels) that doubles or halves the zoom is about 460 px.
const WHEEL_ZOOM_SPEED: f64 = 0.0015;
const KEY_ZOOM_STEP: f64 = 1.25;

/// The filter (its key, `None` for the original), level, column and row of a tile.
type TileKey = (Option<String>, u32, u32, u32);

/// `view` is `None` until the canvas has a size; setting it to `None` fits the image again.
/// `size` reports the canvas size in CSS pixels. `space_held` tells layers drawn on top (the
/// `children`, e.g. annotations) that a left-drag pans. Pointer events those layers do not use
/// bubble up here for zooming and panning. `filter` is the enhancement filter shown, `css_filter`
/// a CSS filter (brightness, contrast) for the image, and `loading` tells whether tiles of the
/// shown filter are still on their way.
#[component]
pub fn TileViewer(
    image: Image,
    view: RwSignal<Option<View>>,
    size: RwSignal<(f64, f64)>,
    space_held: RwSignal<bool>,
    filter: Signal<Option<String>>,
    css_filter: Signal<String>,
    loading: RwSignal<bool>,
    children: Children,
) -> impl IntoView {
    let stage_ref = NodeRef::<html::Div>::new();
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let pyramid = Pyramid::new(image.width, image.height);
    let image_size = (image.width, image.height);
    let id = image.id;
    let tiles = StoredValue::new_local(HashMap::<TileKey, HtmlImageElement>::new());
    // Bumped whenever a tile finishes loading, which triggers a redraw.
    let loaded = RwSignal::new(0_u32);
    let dragging = RwSignal::new(None::<(f64, f64)>);
    // The filter shown before the current one, whose tiles fill in while the current one's load.
    let previous = StoredValue::new(None::<Option<String>>);
    // Tiles asked for and not arrived yet.
    let pending = StoredValue::new(std::collections::HashSet::<TileKey>::new());
    let settle = move |key: &TileKey| {
        pending.update_value(|pending| {
            pending.remove(key);
        });
        let current = filter.get_untracked();
        loading.set(pending.with_value(|pending| pending.iter().any(|k| k.0 == current)));
        loaded.update(|n| *n = n.wrapping_add(1));
    };

    let request_tile = move |key: TileKey| {
        let Ok(element) = HtmlImageElement::new() else {
            return;
        };
        let (on_load, on_error) = (key.clone(), key.clone());
        let on_load = Closure::once_into_js(move || settle(&on_load));
        // A failed tile stays blank (no retry storm); it is asked for again with the next filter.
        let on_error = Closure::once_into_js(move || settle(&on_error));
        element.set_onload(Some(on_load.unchecked_ref()));
        element.set_onerror(Some(on_error.unchecked_ref()));
        let (filter, level, x, y) = &key;
        let query = filter.as_ref().map(|key| format!("?filter={key}")).unwrap_or_default();
        element.set_src(&format!("/api/images/{id}/tiles/{level}/{x}/{y}{query}"));
        pending.update_value(|pending| {
            pending.insert(key.clone());
        });
        loading.set(true);
        tiles.update_value(|tiles| {
            tiles.insert(key, element);
        });
    };
    // The filter drawn last time.
    let drawn = StoredValue::new(None::<Option<String>>);

    let draw = move || {
        let (Some(canvas), Some(current)) = (canvas_ref.get_untracked(), view.get_untracked())
        else {
            return;
        };
        let Some(context) = context_2d(&canvas) else {
            return;
        };
        let ratio = window().device_pixel_ratio();
        context.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0).ok();
        context.set_fill_style_str(BACKGROUND);
        context.fill_rect(
            0.0,
            0.0,
            f64::from(canvas.width()),
            f64::from(canvas.height()),
        );
        // Past 100% show crisp pixels, as image editors do; below it, smooth downscaling.
        context.set_image_smoothing_enabled(current.scale * ratio < 1.0);

        let visible = current.visible(size.get_untracked());
        let target = pyramid.level_for_scale(current.scale * ratio);
        let coarsest = pyramid.levels() - 1;
        let shown = filter.get_untracked();
        // On a new filter, keep the tiles of the one before (to fill in) and drop the rest.
        if let Some(last) = drawn.get_value().filter(|last| *last != shown) {
            tiles.update_value(|tiles| tiles.retain(|key, _| key.0 == last || key.0 == shown));
            pending.update_value(|pending| pending.retain(|key| key.0 == shown));
            loading.set(pending.with_value(|pending| !pending.is_empty()));
            previous.set_value(Some(last));
        }
        drawn.set_value(Some(shown.clone()));
        // The current filter's coarser levels, then the previous filter's sharp tiles where the
        // current one's have not come yet, then the current filter's sharp tiles.
        let mut layers: Vec<(Option<String>, u32)> = ((target + 1)..=coarsest).rev().map(|level| (shown.clone(), level)).collect();
        if let Some(before) = previous.get_value() {
            layers.push((before, target));
        }
        layers.push((shown.clone(), target));
        for (layer, level) in layers {
            let scale = Pyramid::level_scale(level);
            for (column, row) in pyramid.tiles_in(level, visible) {
                let key = (layer.clone(), level, column, row);
                let Some(element) = tiles.with_value(|tiles| tiles.get(&key).cloned()) else {
                    // Only the shown filter's target level and the always-useful coarsest level
                    // are fetched.
                    if layer == shown && (level == target || level == coarsest) {
                        request_tile(key);
                    }
                    continue;
                };
                if !element.complete() || element.natural_width() == 0 {
                    continue;
                }
                let bounds = pyramid
                    .tile_bounds(level, column, row)
                    .expect("listed tiles exist");
                // The last level pixel of an edge may cover less than `scale` image pixels.
                let image_x =
                    |level_x: u32| (f64::from(level_x) * scale).min(f64::from(image_size.0));
                let image_y =
                    |level_y: u32| (f64::from(level_y) * scale).min(f64::from(image_size.1));
                let top_left = current.to_screen(Point::new(image_x(bounds.x), image_y(bounds.y)));
                let bottom_right = current.to_screen(Point::new(
                    image_x(bounds.x + bounds.width),
                    image_y(bounds.y + bounds.height),
                ));
                // Whole device pixels, so neighbouring tiles meet without seams.
                let (x0, y0) = ((top_left.x * ratio).round(), (top_left.y * ratio).round());
                let (x1, y1) = (
                    (bottom_right.x * ratio).round(),
                    (bottom_right.y * ratio).round(),
                );
                context
                    .draw_image_with_html_image_element_and_dw_and_dh(
                        &element,
                        x0,
                        y0,
                        x1 - x0,
                        y1 - y0,
                    )
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
        filter.track();
        draw();
    });

    // Match the canvas backing store to its displayed size, and fit the image when there is no view yet.
    let resize = move || {
        let Some(canvas) = canvas_ref.get_untracked() else {
            return;
        };
        let ratio = window().device_pixel_ratio();
        let (width, height) = (
            f64::from(canvas.client_width()),
            f64::from(canvas.client_height()),
        );
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
        view.update(|view| {
            *view = view.map(|v| v.zoom_at(Point::new(width / 2.0, height / 2.0), factor))
        });
    };
    let keymap = prefs().keymap;
    let on_key_down = window_event_listener(ev::keydown, move |ev| {
        if typing_in_a_field() {
            return;
        }
        let keys = keymap.get_untracked();
        let press = press(&ev);
        if keys.matches(Action::Pan, &press) {
            space_held.set(true);
        } else if keys.matches(Action::Fit, &press) {
            view.set(None);
        } else if keys.matches(Action::ZoomIn, &press) {
            zoom_at_centre(KEY_ZOOM_STEP);
        } else if keys.matches(Action::ZoomOut, &press) {
            zoom_at_centre(1.0 / KEY_ZOOM_STEP);
        } else {
            return;
        }
        ev.prevent_default();
    });
    let on_key_up = window_event_listener(ev::keyup, move |ev| {
        if keymap.with_untracked(|keys| keys.is_key(Action::Pan, &ev.key())) {
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
        let Some(anchor) = stage_point(stage_ref, &ev) else {
            return;
        };
        let factor = (-pixels * WHEEL_ZOOM_SPEED).exp();
        view.update(|view| *view = view.map(|v| v.zoom_at(anchor, factor)));
    };
    let on_pointer_down = move |ev: ev::PointerEvent| {
        let pans = ev.button() == 1 || (ev.button() == 0 && space_held.get_untracked());
        if !pans {
            return;
        }
        ev.prevent_default();
        if let Some(stage) = stage_ref.get_untracked() {
            stage.set_pointer_capture(ev.pointer_id()).ok();
        }
        dragging.set(Some((f64::from(ev.client_x()), f64::from(ev.client_y()))));
    };
    let on_pointer_move = move |ev: ev::PointerEvent| {
        let Some((last_x, last_y)) = dragging.get_untracked() else {
            return;
        };
        let (x, y) = (f64::from(ev.client_x()), f64::from(ev.client_y()));
        view.update(|view| *view = view.map(|v| v.pan(x - last_x, y - last_y)));
        dragging.set(Some((x, y)));
    };
    let stop_drag = move |_: ev::PointerEvent| dragging.set(None);

    view! {
        <div
            class="stage"
            class:pan-ready=move || space_held.get()
            class:panning=move || dragging.get().is_some()
            node_ref=stage_ref
            on:wheel=on_wheel
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=stop_drag
            on:pointercancel=stop_drag
            // Middle-click would otherwise start autoscroll or paste.
            on:mousedown=|ev: ev::MouseEvent| if ev.button() == 1 { ev.prevent_default() }
        >
            <canvas class="viewer" node_ref=canvas_ref aria-label=format!("Image {}", image.path) style:filter=move || css_filter.get()></canvas>
            {children()}
        </div>
    }
}

/// Where a mouse event happened, in CSS pixels relative to the stage (and so to its canvases).
pub(crate) fn stage_point(stage: NodeRef<html::Div>, ev: &ev::MouseEvent) -> Option<Point> {
    let rect = stage.get_untracked()?.get_bounding_client_rect();
    Some(Point::new(
        f64::from(ev.client_x()) - rect.left(),
        f64::from(ev.client_y()) - rect.top(),
    ))
}

pub(crate) fn context_2d(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    // `getContext("2d")` always returns a 2D context; skip the `instanceof` check, which fails in
    // some headless engines (Obscura) whose context class has another name.
    Some(canvas.get_context("2d").ok().flatten()?.unchecked_into())
}

pub(crate) fn typing_in_a_field() -> bool {
    document().active_element().is_some_and(|element| {
        matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
    })
}
