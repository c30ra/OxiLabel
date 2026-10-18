use std::collections::{BTreeMap, BTreeSet};

use leptos::{prelude::*, task::spawn_local};
use rtag_core::{
    Footprint, Label, PointShape, ShapeType,
    export::{ExportFormat, ExportRequest, ExportResult, ImageMode, ImageSelection, Split, Subset},
};

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
    let labels = LocalResource::new(move || async move { api::get::<Vec<Label>>(&format!("/api/projects/{project}/labels")).await });
    // This export's footprints where they differ from their label's, by label id.
    let footprints = RwSignal::new(BTreeMap::<i64, Footprint>::new());
    let footprint_bounds = RwSignal::new(true);
    // Whether the chosen format writes boxes (footprints only size them).
    let boxes = move || {
        let id = format.get();
        formats.get().and_then(Result::ok).is_some_and(|list| list.iter().any(|f| f.id == id && f.boxes))
    };
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
            footprints: footprints.get_untracked(),
            footprint_bounds: footprint_bounds.get_untracked(),
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
                        <FootprintTable labels footprints footprint_bounds boxes=Signal::derive(boxes) />
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

/// The export footprints of the labels with polylines or points, filled in from each label and
/// changeable for this export. Box formats can leave them out, and take no point shape.
#[component]
fn FootprintTable(
    labels: LocalResource<Result<Vec<Label>, String>>,
    footprints: RwSignal<BTreeMap<i64, Footprint>>,
    footprint_bounds: RwSignal<bool>,
    boxes: Signal<bool>,
) -> impl IntoView {
    let rows = move || {
        labels
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .filter(|label| !label.ignore && label.shape_types.iter().any(|s| matches!(s, ShapeType::Polyline | ShapeType::Points)))
            .collect::<Vec<_>>()
    };
    view! {
        <Show when=move || !rows().is_empty()>
            <fieldset class="footprints">
                <legend>"Export footprint"</legend>
                <Show when=move || boxes.get()>
                    <label class="check">
                        <input type="checkbox" class="footprint-bounds" prop:checked=footprint_bounds
                            on:change:target=move |ev| footprint_bounds.set(ev.target().checked()) />
                        "Box lines and points by their footprint"
                        <small class="muted">"off: a point is a box of no size"</small>
                    </label>
                </Show>
                <table>
                    <thead>
                        <tr>
                            <th>"Label"</th><th>"Line width"</th><th>"Point radius"</th>
                            <Show when=move || !boxes.get()><th>"Point shape"</th></Show>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=rows key=|label| label.id let:label>
                            <FootprintRow label footprints boxes />
                        </For>
                    </tbody>
                </table>
                <span class="muted hint">"Filled in from each label. Changes apply to this export only."</span>
            </fieldset>
        </Show>
    }
}

#[component]
fn FootprintRow(label: Label, footprints: RwSignal<BTreeMap<i64, Footprint>>, boxes: Signal<bool>) -> impl IntoView {
    let (id, own) = (label.id, label.footprint);
    let current = move || footprints.read().get(&id).copied().unwrap_or(own);
    let edit = move |change: &dyn Fn(&mut Footprint)| {
        let mut footprint = current();
        change(&mut footprint);
        footprints.update(|map| {
            if footprint == own {
                map.remove(&id);
            } else {
                map.insert(id, footprint);
            }
        });
    };
    let changed = move || current() != own;
    let lines = label.shape_types.contains(&ShapeType::Polyline);
    let points = label.shape_types.contains(&ShapeType::Points);
    let number = move |get: fn(&Footprint) -> f64, set: fn(&mut Footprint, f64), name: String| {
        view! {
            <input type="number" min="0.5" step="0.5" aria-label=name class:changed=move || get(&current()) != get(&own)
                prop:value=move || get(&current()).to_string()
                on:change:target={move |ev| if let Ok(v) = ev.target().value().parse::<f64>() { edit(&|f| set(f, v)) }} />
            " px"
        }
    };
    let dash = || view! { <span class="muted">"—"</span> };
    view! {
        <tr class:changed=changed>
            <td><span class="swatch" style:background=label.color.clone()></span>{label.name.clone()}</td>
            <td>{if lines { number(|f| f.line_width, |f, v| f.line_width = v, format!("{} line width", label.name)).into_any() } else { dash().into_any() }}</td>
            <td>{if points { number(|f| f.point_radius, |f, v| f.point_radius = v, format!("{} point radius", label.name)).into_any() } else { dash().into_any() }}</td>
            <Show when=move || !boxes.get()>
                <td>
                    {if points {
                        view! {
                            <select aria-label="Point shape" class:changed=move || current().point_shape != own.point_shape
                                on:change:target=move |ev| {
                                    let shape = PointShape::named(&ev.target().value());
                                    edit(&|f| f.point_shape = shape);
                                }>
                                <option value="disc" selected=move || current().point_shape == PointShape::Disc>"Disc"</option>
                                <option value="square" selected=move || current().point_shape == PointShape::Square>"Square"</option>
                            </select>
                        }.into_any()
                    } else {
                        dash().into_any()
                    }}
                </td>
            </Show>
            <td>
                <Show when=changed>
                    <button type="button" class="icon" title="Reset to the label's values" aria-label=format!("Reset {}", label.name)
                        on:click=move |_| footprints.update(|map| { map.remove(&id); })>"↺"</button>
                </Show>
            </td>
        </tr>
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
