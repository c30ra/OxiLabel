use std::collections::BTreeSet;

use leptos::{prelude::*, task::spawn_local};
use rtag_core::export::{ExportFormat, ExportRequest, ExportResult, ImageMode, ImageSelection, Split, Subset};

use crate::api;

#[derive(Clone, Copy, PartialEq)]
enum Which {
    All,
    WithAnnotations,
    Selected,
}

/// Modal that exports the project in a chosen format into a folder on the server, offering the
/// result as a zip download as well.
#[component]
pub fn ExportDialog(
    project: i64,
    project_name: String,
    selected: RwSignal<BTreeSet<i64>>,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let formats = LocalResource::new(|| api::get::<Vec<ExportFormat>>("/api/exporters"));
    let format = RwSignal::new(String::from("yolo-detect"));
    let which = RwSignal::new(if selected.read_untracked().is_empty() { Which::All } else { Which::Selected });
    let mode = RwSignal::new(ImageMode::Symlink);
    // The image options the chosen format supports (its default first).
    let supported = move || {
        let id = format.get();
        formats.get().and_then(Result::ok).and_then(|list| list.into_iter().find(|f| f.id == id)).map(|f| f.image_modes).unwrap_or_default()
    };
    // Keep the choice valid when the format (or the list of formats) changes.
    Effect::new(move |_| {
        let modes = supported();
        if !modes.is_empty() && !modes.contains(&mode.get_untracked()) {
            mode.set(modes[0]);
        }
    });
    let split = RwSignal::new(Split::default());
    let busy = RwSignal::new(false);
    let outcome = RwSignal::new(None::<Result<ExportResult, String>>);

    let split_error = move || split.get().validate().err();
    let start = move |_| {
        let images = match which.get_untracked() {
            Which::All => ImageSelection::All,
            Which::WithAnnotations => ImageSelection::WithAnnotations,
            Which::Selected => ImageSelection::Ids { ids: selected.get_untracked().into_iter().collect() },
        };
        let request = ExportRequest {
            format: format.get_untracked(),
            images,
            include_images: Some(mode.get_untracked()),
            split: split.get_untracked(),
        };
        busy.set(true);
        spawn_local(async move {
            outcome.set(Some(api::post::<_, ExportResult>(&format!("/api/projects/{project}/exports"), &request).await));
            busy.set(false);
        });
    };

    let percent = move |label: &'static str, get: fn(&Split) -> u8, set: fn(&mut Split, u8)| {
        view! {
            <label class="percent">
                {label}
                <input type="number" min="0" max="100" prop:value=move || get(&split.get()).to_string()
                    on:change:target=move |ev| {
                        let value = ev.target().value().trim().parse::<u8>().unwrap_or(0).min(100);
                        split.update(|split| set(split, value));
                    } />
                "%"
            </label>
        }
    };
    let choice = move |name: &'static str, label: &'static str, hint: &'static str, value: Which, disabled: Signal<bool>| {
        view! {
            <label class="choice" class:disabled=disabled>
                <input type="radio" name=name prop:checked=move || which.get() == value disabled=disabled
                    on:change=move |_| which.set(value) />
                <span>{label}<small class="muted">{hint}</small></span>
            </label>
        }
    };
    let image_choice = move |label: &'static str, hint: &'static str, value: ImageMode| {
        view! {
            <label class="choice" class:hidden=move || !supported().contains(&value)>
                <input type="radio" name="include" prop:checked=move || mode.get() == value on:change=move |_| mode.set(value) />
                <span>{label}<small class="muted">{hint}</small></span>
            </label>
        }
    };
    let selected_label = move || {
        let count = selected.read().len();
        if count == 1 { "1 image selected in the grid".to_owned() } else { format!("{count} images selected in the grid") }
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog export-dialog" role="dialog" aria-modal="true" aria-labelledby="export-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="export-title">{format!("Export “{project_name}”")}</h2>
                        <p class="muted">"Writes a dataset folder on the server; you can also download it as a zip."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <Show when=move || outcome.get().is_none() fallback=move || view! { <ExportOutcome outcome=outcome /> }>
                    <div class="dialog-body">
                        <fieldset>
                            <legend>"Format"</legend>
                            <select aria-label="Format" on:change:target=move |ev| format.set(ev.target().value())>
                                {move || formats.get().and_then(Result::ok).unwrap_or_default().into_iter().map(|f| {
                                    let id = f.id.clone();
                                    view! { <option value=f.id selected=move || format.get() == id>{f.name}</option> }
                                }).collect_view()}
                            </select>
                        </fieldset>
                        <fieldset class="choices">
                            <legend>"Images"</legend>
                            {choice("which", "All images", "", Which::All, Signal::derive(|| false))}
                            {choice("which", "Images with annotations", "", Which::WithAnnotations, Signal::derive(|| false))}
                            <label class="choice" class:disabled=move || selected.read().is_empty()>
                                <input type="radio" name="which" prop:checked=move || which.get() == Which::Selected
                                    disabled=move || selected.read().is_empty() on:change=move |_| which.set(Which::Selected) />
                                <span>"Selected images"<small class="muted">{selected_label}</small></span>
                            </label>
                        </fieldset>
                        <fieldset class="split">
                            <legend>"Split"</legend>
                            {percent("Train", |s| s.train, |s, v| s.train = v)}
                            {percent("Val", |s| s.val, |s, v| s.val = v)}
                            {percent("Test", |s| s.test, |s, v| s.test = v)}
                            <p class="muted hint">"Each image's subset follows from its content, so it stays the same in every export."</p>
                            {move || split_error().map(|message| view! { <p class="error" role="alert">{message}</p> })}
                        </fieldset>
                        <fieldset class="choices">
                            <legend>"Include images"</legend>
                            {image_choice("No, labels only", "point your training at the images yourself", ImageMode::None)}
                            {image_choice("Links to the originals", "instant, no extra space; for training on this machine", ImageMode::Symlink)}
                            {image_choice("Copies", "a self-contained dataset; takes time and disk space", ImageMode::Copy)}
                        </fieldset>
                    </div>
                </Show>
                <div class="dialog-foot">
                    <span class="grow"></span>
                    <button on:click=move |_| on_close()>{move || if outcome.get().is_some() { "Close" } else { "Cancel" }}</button>
                    <Show when=move || outcome.get().is_none()>
                        <button class="primary" disabled=move || busy.get() || split_error().is_some() on:click=start>
                            {move || if busy.get() { "Exporting…" } else { "Export" }}
                        </button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[component]
fn ExportOutcome(outcome: RwSignal<Option<Result<ExportResult, String>>>) -> impl IntoView {
    move || match outcome.get() {
        Some(Ok(result)) => {
            let subsets = Subset::ALL
                .iter()
                .filter_map(|subset| result.subsets.get(subset).filter(|n| **n > 0).map(|n| format!("{} {n}", subset.name())))
                .collect::<Vec<_>>()
                .join(", ");
            view! {
                <div class="report" role="status">
                    <p>{format!("Exported {} images ({subsets}) with {} annotations to", result.image_count, result.annotation_count)}</p>
                    <p><code class="export-path">{result.path.clone()}</code></p>
                    <p><a class="button primary" href=result.download.clone() download>"Download zip"</a></p>
                </div>
            }
            .into_any()
        }
        Some(Err(message)) => view! { <p class="report error" role="alert">{message}</p> }.into_any(),
        None => ().into_any(),
    }
}
