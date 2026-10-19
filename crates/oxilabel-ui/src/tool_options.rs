//! The tool settings bar above the image (as in Blender's tool header): the active tool's options,
//! always visible and editable. The tool's options pie (Q) changes the same settings.

use leptos::prelude::*;

use crate::annotate::{Editor, MAX_BRUSH_SIZE, MAX_EDGE_OFFSET, MAX_SPACING, MAX_THRESHOLD, MIN_BRUSH_SIZE, MIN_SPACING, Tool};

#[component]
pub fn ToolOptions(editor: Editor) -> impl IntoView {
    view! {
        <div class="tool-options" role="toolbar" aria-label="Tool settings">
            <span class="tool-name">{move || editor.tool.get().name()}</span>
            {move || match editor.tool.get() {
                // Tag is never the active tool.
                Tool::Select | Tool::Tag => view! {
                    <span class="muted hint">"Click or drag a box to select · Shift adds · Ctrl removes · A all · Q menu"</span>
                }.into_any(),
                Tool::Rectangle => view! {
                    <Segmented
                        choices=vec![("2-point", false), ("3-point oriented", true)]
                        value=editor.oriented
                    />
                    <Toggle label="From center" key="Alt" value=editor.from_centre disabled=Signal::derive(move || editor.oriented.get()) />
                    <Toggle label="Square" key="Shift" value=editor.square disabled=Signal::derive(move || editor.oriented.get()) />
                }.into_any(),
                Tool::Polygon => view! {
                    <Toggle label="Snap 15°" key="Shift" value=editor.snap disabled=Signal::derive(|| false) />
                    <span class="muted hint">"Enter or double-click finishes · Backspace removes the last point"</span>
                }.into_any(),
                Tool::Polyline => view! {
                    <Segmented choices=vec![("Click", false), ("Freehand", true)] value=editor.freehand />
                    <Show
                        when=move || editor.freehand.get()
                        fallback=move || view! { <Toggle label="Snap 15°" key="Shift" value=editor.snap disabled=Signal::derive(|| false) /> }
                    >
                        <NumberOption label="Spacing" title="Distance between vertices while dragging ([ and ] change it)"
                            value=editor.spacing range=(MIN_SPACING, MAX_SPACING) unit="px" keys=true />
                        <Toggle label="Snap to edges" key="Shift" value=editor.edge_snap disabled=Signal::derive(|| false) />
                        <Show when=move || editor.edge_snap.get()>
                            <NumberOption label="Radius" title="How far from the pointer an edge is looked for, in screen pixels"
                                value=editor.snap_radius range=(1.0, 100.0) unit="px" keys=false />
                            <Filtered editor=editor value=editor.snap_filtered />
                        </Show>
                    </Show>
                }.into_any(),
                Tool::Points => view! {
                    <Segmented choices=vec![("Multiple", false), ("Single point", true)] value=editor.single_point />
                }.into_any(),
                Tool::Ellipse => view! {
                    <Segmented choices=vec![("From center", true), ("Across the box", false)] value=editor.ellipse_from_centre />
                    <span class="muted hint">"Shift: circle · Alt: the other way"</span>
                }.into_any(),
                Tool::Brush => view! {
                    <Segmented choices=vec![("Paint", false), ("Fuzzy fill", true)] value=editor.fill />
                    <Segmented choices=vec![("Brush", false), ("Eraser", true)] value=editor.eraser />
                    <Show
                        when=move || editor.fill.get()
                        fallback=move || view! {
                            <NumberOption label="Size" title="Brush diameter in image pixels ([ and ] or F change it)"
                                value=editor.brush_size range=(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE) unit="px" keys=true />
                            <span class="muted hint">"F resizes · Ctrl inverts"</span>
                            <Toggle label="Stop at edges" key="Shift" value=editor.stop_at_edges disabled=Signal::derive(|| false) />
                            <Show when=move || editor.stop_at_edges.get()>
                                <NumberOption label="Threshold" title="How similar to where the stroke starts a pixel must be to be painted (0–255)"
                                    value=editor.threshold range=(0.0, MAX_THRESHOLD) unit="" keys=false />
                                <NumberOption label="Offset" title="Grows (positive) or shrinks (negative) the region the brush may paint, in image pixels"
                                    value=editor.edge_offset range=(-MAX_EDGE_OFFSET, MAX_EDGE_OFFSET) unit="px" keys=false />
                            </Show>
                            // Shift limits the brush even with "Stop at edges" off.
                            <Filtered editor=editor value=editor.edges_filtered />
                        }
                    >
                        <NumberOption label="Threshold" title="Largest colour difference from the clicked pixel (0–255); [ and ] or dragging after the click change it"
                            value=editor.threshold range=(0.0, MAX_THRESHOLD) unit="" keys=true />
                        <Segmented choices=vec![("Contiguous", true), ("Global", false)] value=editor.contiguous />
                        <NumberOption label="Clean up" title="Patches and holes smaller than this many image pixels are dropped or filled"
                            value=editor.clean_up range=(0.0, 1_000_000.0) unit="px²" keys=false />
                        <NumberOption label="Max" title="Largest fill, as a share of the image"
                            value=editor.max_fill range=(1.0, 100.0) unit="%" keys=false />
                        <Filtered editor=editor value=editor.fill_filtered />
                    </Show>
                }.into_any(),
            }}
        </div>
    }
}

/// A row of buttons of which one is on.
#[component]
fn Segmented(choices: Vec<(&'static str, bool)>, value: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="segmented" role="radiogroup">
            {choices.into_iter().map(|(label, choice)| view! {
                <button role="radio" aria-checked=move || (value.get() == choice).to_string()
                    class:active=move || value.get() == choice on:click=move |_| value.set(choice)>{label}</button>
            }).collect_view()}
        </div>
    }
}

/// An on/off option, with the key that inverts it while drawing.
#[component]
fn Toggle(label: &'static str, key: &'static str, value: RwSignal<bool>, disabled: Signal<bool>) -> impl IntoView {
    view! {
        <button class="toggle" aria-pressed=move || value.get().to_string() class:active=move || value.get()
            disabled=move || disabled.get() title=format!("{key} inverts it while drawing")
            on:click=move |_| value.update(|on| *on = !*on)>
            {label}<kbd>{key}</kbd>
        </button>
    }
}

/// Whether the tool looks at the filtered view's pixels instead of the original's; only with a
/// view filter on.
#[component]
fn Filtered(editor: Editor, value: RwSignal<bool>) -> impl IntoView {
    let no_filter = move || editor.view_filter.get().is_none();
    view! {
        <button class="toggle filtered" aria-pressed=move || value.get().to_string() class:active=move || value.get() && !no_filter()
            disabled=no_filter
            title=move || if no_filter() { "Choose a view filter to analyse its pixels instead of the original's" } else { "Analyse the filtered view's pixels instead of the original's" }
            on:click=move |_| value.update(|on| *on = !*on)>
            "Filtered view"
        </button>
    }
}

/// A number option, typed, or changed with `[` and `]` when `keys` is set.
#[component]
fn NumberOption(label: &'static str, title: &'static str, value: RwSignal<f64>, range: (f64, f64), unit: &'static str, keys: bool) -> impl IntoView {
    view! {
        <label class="option-number" title=title>
            {label}
            <input type="number" min=range.0 max=range.1 step="any"
                prop:value=move || format!("{:.0}", value.get())
                on:change:target={move |ev| {
                    if let Ok(typed) = ev.target().value().trim().parse::<f64>() {
                        value.set(typed.clamp(range.0, range.1));
                    }
                    // Back to the canvas, so tool keys work again.
                    ev.target().blur().ok();
                }} />
            {unit}
            {keys.then(|| view! { <kbd>"["</kbd><kbd>"]"</kbd> })}
        </label>
    }
}
