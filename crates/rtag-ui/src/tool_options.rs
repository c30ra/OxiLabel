//! The tool settings bar above the image (as in Blender's tool header): the active tool's options,
//! always visible and editable. The tool's options pie (Q) changes the same settings.

use leptos::prelude::*;

use crate::annotate::{Editor, MAX_BRUSH_SIZE, MAX_SPACING, MIN_BRUSH_SIZE, MIN_SPACING, Tool};

#[component]
pub fn ToolOptions(editor: Editor) -> impl IntoView {
    view! {
        <div class="tool-options" role="toolbar" aria-label="Tool settings">
            <span class="tool-name">{move || editor.tool.get().name()}</span>
            {move || match editor.tool.get() {
                Tool::Select => view! {
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
                            value=editor.spacing range=(MIN_SPACING, MAX_SPACING) />
                    </Show>
                }.into_any(),
                Tool::Points => view! {
                    <Segmented choices=vec![("Multiple", false), ("Single point", true)] value=editor.single_point />
                }.into_any(),
                Tool::Circle => view! {
                    <Segmented choices=vec![("Centre and radius", false), ("Diameter", true)] value=editor.diameter />
                }.into_any(),
                Tool::Brush => view! {
                    <Segmented choices=vec![("Brush", false), ("Eraser", true)] value=editor.eraser />
                    <NumberOption label="Size" title="Brush diameter in image pixels ([ and ] change it)"
                        value=editor.brush_size range=(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE) />
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

/// A size in image pixels, typed or changed with `[` and `]`.
#[component]
fn NumberOption(label: &'static str, title: &'static str, value: RwSignal<f64>, range: (f64, f64)) -> impl IntoView {
    view! {
        <label class="option-number" title=title>
            {label}
            <input type="number" min=range.0 max=range.1 step="1"
                prop:value=move || format!("{:.0}", value.get())
                on:change:target={move |ev| {
                    if let Ok(typed) = ev.target().value().trim().parse::<f64>() {
                        value.set(typed.clamp(range.0, range.1));
                    }
                    // Back to the canvas, so tool keys work again.
                    ev.target().blur().ok();
                }} />
            "px"
            <kbd>"["</kbd><kbd>"]"</kbd>
        </label>
    }
}
