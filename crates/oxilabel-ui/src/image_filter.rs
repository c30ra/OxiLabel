//! The filter that picks which of a project's images are listed: shared by the image grid and
//! the editor (whose next and previous images, filmstrip and position follow it), and kept in
//! session storage per project, so it survives reloading the page.

use leptos::prelude::*;
use oxilabel_core::{ImageFilter, Label};

const BYTES_PER_MB: f64 = 1_000_000.0;

fn storage_key(project: i64) -> String {
    format!("oxilabel.image-filter.{project}")
}

fn storage() -> Option<web_sys::Storage> {
    window().session_storage().ok().flatten()
}

/// The project's filter as last left in this browser tab; every change is kept.
pub fn shared(project: i64) -> RwSignal<ImageFilter> {
    let filter = RwSignal::new(
        storage()
            .and_then(|storage| storage.get_item(&storage_key(project)).ok().flatten())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default(),
    );
    Effect::new(move |_| {
        if let (Some(storage), Ok(text)) = (storage(), serde_json::to_string(&filter.get())) {
            storage.set_item(&storage_key(project), &text).ok();
        }
    });
    filter
}

/// How many filters of the panel are set.
pub fn panel_count(filter: &ImageFilter) -> usize {
    [
        filter.min_width.is_some() || filter.max_width.is_some(),
        filter.min_height.is_some() || filter.max_height.is_some(),
        filter.min_size.is_some() || filter.max_size.is_some(),
        filter.modified_from.is_some() || filter.modified_to.is_some(),
        filter.same_name,
        !filter.labels.is_empty(),
    ]
    .into_iter()
    .filter(|active| *active)
    .count()
}

/// Width, height, file size, date, shared file name and label filters. Each field applies when it is changed.
#[component]
pub fn FilterPanel(
    filter: RwSignal<ImageFilter>,
    set_filter: impl Fn(&dyn Fn(&mut ImageFilter)) + Copy + Send + Sync + 'static,
    labels: Signal<Vec<Label>>,
) -> impl IntoView {
    let number = |text: &str| text.trim().parse::<u32>().ok();
    let megabytes = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|mb| *mb >= 0.0)
            .map(|mb| (mb * BYTES_PER_MB).round() as u64)
    };
    let show_mb = |bytes: Option<u64>| {
        bytes
            .map(|b| format!("{}", b as f64 / BYTES_PER_MB))
            .unwrap_or_default()
    };
    let show = |value: Option<u32>| value.map(|v| v.to_string()).unwrap_or_default();
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let date = |value: String| Some(value).filter(|v| !v.is_empty());

    view! {
        <div class="filter-panel" role="group" aria-label="Filters">
            <fieldset>
                <legend>"Width (px)"</legend>
                <input type="number" min="0" placeholder="min" aria-label="Minimum width" prop:value=move || show(filter.read().min_width)
                    on:change:target=move |ev| { let v = number(&ev.target().value()); set_filter(&|f| f.min_width = v) } />
                <span class="muted">"–"</span>
                <input type="number" min="0" placeholder="max" aria-label="Maximum width" prop:value=move || show(filter.read().max_width)
                    on:change:target=move |ev| { let v = number(&ev.target().value()); set_filter(&|f| f.max_width = v) } />
            </fieldset>
            <fieldset>
                <legend>"Height (px)"</legend>
                <input type="number" min="0" placeholder="min" aria-label="Minimum height" prop:value=move || show(filter.read().min_height)
                    on:change:target=move |ev| { let v = number(&ev.target().value()); set_filter(&|f| f.min_height = v) } />
                <span class="muted">"–"</span>
                <input type="number" min="0" placeholder="max" aria-label="Maximum height" prop:value=move || show(filter.read().max_height)
                    on:change:target=move |ev| { let v = number(&ev.target().value()); set_filter(&|f| f.max_height = v) } />
            </fieldset>
            <fieldset>
                <legend>"File size (MB)"</legend>
                <input type="number" min="0" step="0.1" placeholder="min" aria-label="Minimum file size in MB" prop:value=move || show_mb(filter.read().min_size)
                    on:change:target=move |ev| { let v = megabytes(&ev.target().value()); set_filter(&|f| f.min_size = v) } />
                <span class="muted">"–"</span>
                <input type="number" min="0" step="0.1" placeholder="max" aria-label="Maximum file size in MB" prop:value=move || show_mb(filter.read().max_size)
                    on:change:target=move |ev| { let v = megabytes(&ev.target().value()); set_filter(&|f| f.max_size = v) } />
            </fieldset>
            <fieldset>
                <legend>"Modified"</legend>
                <input type="date" aria-label="Modified from" prop:value=move || text(&filter.read().modified_from)
                    on:change:target=move |ev| { let v = date(ev.target().value()); set_filter(&|f| f.modified_from = v.clone()) } />
                <span class="muted">"–"</span>
                <input type="date" aria-label="Modified until" prop:value=move || text(&filter.read().modified_to)
                    on:change:target=move |ev| { let v = date(ev.target().value()); set_filter(&|f| f.modified_to = v.clone()) } />
            </fieldset>
            <fieldset>
                <legend>"File name"</legend>
                <label class="check" title="Only images whose file name another image of the project has too, grouped by name">
                    <input type="checkbox" prop:checked=move || filter.read().same_name
                        on:change:target=move |ev| { let on = ev.target().checked(); set_filter(&|f| f.same_name = on) } />
                    "Shared with another image"
                </label>
            </fieldset>
            {move || (!labels.read().is_empty()).then(|| view! {
                <fieldset class="label-filter">
                    <legend>"Has label"</legend>
                    {labels.get().into_iter().map(|label| {
                        let id = label.id;
                        view! {
                            <label class="check">
                                <input type="checkbox" prop:checked=move || filter.read().labels.contains(&id)
                                    on:change:target=move |ev| {
                                        let on = ev.target().checked();
                                        set_filter(&|f| {
                                            f.labels.retain(|label| *label != id);
                                            if on {
                                                f.labels.push(id);
                                            }
                                        })
                                    } />
                                <span class="swatch" style:background=label.color.clone()></span>
                                {label.name.clone()}
                            </label>
                        }
                    }).collect_view()}
                </fieldset>
            })}
            <button on:click=move |_| set_filter(&|f| *f = ImageFilter { name: f.name.clone(), job: f.job, without_job: f.without_job, ..ImageFilter::default() })>"Clear filters"</button>
        </div>
    }
}

