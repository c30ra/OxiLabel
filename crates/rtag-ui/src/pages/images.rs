use std::collections::BTreeSet;

use leptos::{ev::MouseEvent, prelude::*, task::spawn_local};
use rtag_core::{Image, ImageFilter, ImagePage, RemovalReport, RemovalRequest};

use crate::api;

const PAGE_SIZES: [i64; 4] = [30, 60, 120, 240];
const BYTES_PER_MB: f64 = 1_000_000.0;

/// Paged, filterable grid of the project's image thumbnails. `version` changes whenever images were added.
/// Images can be selected (checkbox, or click once something is selected; shift-click for a range;
/// A selects all matching images, Alt+A clears) and removed from the project.
#[component]
pub fn ImageSet(project: i64, version: RwSignal<u32>) -> impl IntoView {
    let offset = RwSignal::new(0_i64);
    let page_size = RwSignal::new(60_i64);
    let filter = RwSignal::new(ImageFilter::default());
    let filters_open = RwSignal::new(false);
    let total = RwSignal::new(0_i64);
    let selected = RwSignal::new(BTreeSet::<i64>::new());
    let anchor = RwSignal::new(None::<usize>);
    let confirming = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let images = LocalResource::new(move || {
        let mut query = filter.get().to_query();
        query.push(("offset", offset.get().to_string()));
        query.push(("limit", page_size.get().to_string()));
        version.track();
        async move {
            let pairs: Vec<(&str, &str)> = query
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            api::get_with_query::<ImagePage>(&format!("/api/projects/{project}/images"), &pairs)
                .await
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(page)) = images.get() {
            total.set(page.total);
        }
    });
    let page_ids = move || match images.get() {
        Some(Ok(page)) => page.images.iter().map(|image| image.id).collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let clear = move || {
        selected.set(BTreeSet::new());
        anchor.set(None);
        confirming.set(false);
    };
    // A new filter starts from the first page with nothing selected, so a removal never
    // includes images the filter hides.
    let set_filter = move |change: &dyn Fn(&mut ImageFilter)| {
        filter.update(|filter| change(filter));
        offset.set(0);
        clear();
    };
    // Plain click toggles one image; shift-click sets the range from the last clicked image.
    let click = move |index: usize, shift: bool| {
        let ids = page_ids();
        match (shift, anchor.get_untracked()) {
            (true, Some(from)) => {
                let (low, high) = (from.min(index), from.max(index));
                selected.update(|set| set.extend(&ids[low..=high.min(ids.len() - 1)]));
            }
            _ => selected.update(|set| {
                if !set.remove(&ids[index]) {
                    set.insert(ids[index]);
                }
            }),
        }
        anchor.set(Some(index));
        confirming.set(false);
    };
    let select_all = move || {
        busy.set(true);
        let query = filter.get_untracked().to_query();
        spawn_local(async move {
            let pairs: Vec<(&str, &str)> = query
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            match api::get_with_query::<Vec<i64>>(
                &format!("/api/projects/{project}/image-ids"),
                &pairs,
            )
            .await
            {
                Ok(ids) => selected.set(ids.into_iter().collect()),
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };
    let remove = move |_| {
        let request = RemovalRequest {
            ids: selected.get_untracked().into_iter().collect(),
        };
        busy.set(true);
        spawn_local(async move {
            match api::post::<_, RemovalReport>(
                &format!("/api/projects/{project}/image-removals"),
                &request,
            )
            .await
            {
                Ok(report) => {
                    clear();
                    // Stay on the last page that still has images.
                    let left = (total.get_untracked() - report.removed as i64).max(0);
                    let size = page_size.get_untracked();
                    offset.update(|o| {
                        if *o >= left {
                            *o = ((left - 1).max(0) / size) * size;
                        }
                    });
                    version.update(|v| *v += 1);
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    // Blender-style keys: A selects all, Alt+A clears, Esc clears, Delete asks to remove,
    // Page Up/Down turn pages. Ignored while a dialog is open or a field has focus.
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        let document = document();
        let typing = document.active_element().is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                && !el.class_list().contains("pick")
        });
        if typing
            || document
                .query_selector(".backdrop")
                .ok()
                .flatten()
                .is_some()
            || ev.ctrl_key()
            || ev.meta_key()
        {
            return;
        }
        let has_selection = !selected.read_untracked().is_empty();
        let size = page_size.get_untracked();
        match ev.key().as_str() {
            "a" | "A" if ev.alt_key() => clear(),
            "a" | "A" => select_all(),
            "Escape" if has_selection => clear(),
            "Delete" if has_selection => confirming.set(true),
            "PageDown" => offset.update(|o| {
                *o = (*o + size).min(((total.get_untracked() - 1).max(0) / size) * size)
            }),
            "PageUp" => offset.update(|o| *o = (*o - size).max(0)),
            _ => return,
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let active_filters = move || {
        let filter = filter.read();
        [
            filter.min_width.is_some() || filter.max_width.is_some(),
            filter.min_height.is_some() || filter.max_height.is_some(),
            filter.min_size.is_some() || filter.max_size.is_some(),
            filter.modified_from.is_some() || filter.modified_to.is_some(),
        ]
        .into_iter()
        .filter(|active| *active)
        .count()
    };

    view! {
        <div class="image-set">
            <div class="toolbar">
                <input class="search" type="search" placeholder="Filter by path (regex)" aria-label="Filter by path, regular expression"
                    prop:value=move || filter.read().name.clone().unwrap_or_default()
                    on:input:target=move |ev| {
                        let text = ev.target().value();
                        set_filter(&|filter| filter.name = Some(text.clone()).filter(|t| !t.is_empty()));
                    } />
                <button class:active=move || filters_open.get() aria-expanded=move || filters_open.get().to_string()
                    on:click=move |_| filters_open.update(|open| *open = !*open)>
                    {move || match active_filters() {
                        0 => "Filters".to_owned(),
                        n => format!("Filters · {n}"),
                    }}
                </button>
                <button title="Select every image matching the filter (A)" disabled=move || busy.get() || total.get() == 0
                    on:click=move |_| select_all()>"Select all"</button>
                <Pager offset=offset page_size=page_size total=total anchor=anchor />
            </div>

            <Show when=move || filters_open.get()>
                <FilterPanel filter=filter set_filter=set_filter />
            </Show>

            <Show when=move || !selected.read().is_empty()>
                <div class="selection-bar" role="toolbar" aria-label="Selection">
                    {move || {
                        let count = selected.read().len();
                        let noun = if count == 1 { "image" } else { "images" };
                        if confirming.get() {
                            view! {
                                <span class="grow">{format!("Remove {count} {noun} from this project? The files stay on disk and can be imported again.")}</span>
                                <button on:click=move |_| confirming.set(false)>"Cancel"</button>
                                <button class="danger" disabled=move || busy.get() on:click=remove>
                                    {move || if busy.get() { "Removing…" } else { "Remove" }}
                                </button>
                            }.into_any()
                        } else {
                            view! {
                                <span class="grow"><strong>{format!("{count} selected")}</strong></span>
                                <button on:click=move |_| selected.update(|set| set.extend(page_ids()))>"Select page"</button>
                                <button title="Alt+A" on:click=move |_| clear()>"Clear"</button>
                                <button class="danger" title="Delete" on:click=move |_| confirming.set(true)>"Remove from project…"</button>
                            }.into_any()
                        }
                    }}
                </div>
            </Show>

            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
            {move || match images.get() {
                None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(page)) if page.total == 0 && filter.read_untracked().is_empty() => {
                    view! { <p class="muted">"No images yet. Use “Import images…” to add some from the server's image folder."</p> }
                        .into_any()
                }
                Some(Ok(page)) if page.total == 0 => view! { <p class="muted">"No images match the filter."</p> }.into_any(),
                Some(Ok(page)) => view! {
                    <ul class="grid" class:selecting=move || !selected.read().is_empty()>
                        {page.images.into_iter().enumerate().map(|(index, image)| {
                            view! { <Tile project=project image=image selected=selected on_click=move |shift| click(index, shift) /> }
                        }).collect_view()}
                    </ul>
                }
                .into_any(),
            }}
        </div>
    }
}

#[component]
fn Tile(
    project: i64,
    image: Image,
    selected: RwSignal<BTreeSet<i64>>,
    on_click: impl Fn(bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let id = image.id;
    let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
    let mut details = vec![
        image.path.clone(),
        format!("{} × {} px", image.width, image.height),
    ];
    details.extend(image.size.map(human_size));
    details.extend(image.modified.clone());
    let is_selected = move || selected.read().contains(&id);
    view! {
        // A click opens the image, unless something is selected: then it adds to the selection.
        <li title=details.join("\n") class:selected=is_selected
            on:click=move |ev: MouseEvent| if !selected.read_untracked().is_empty() {
                ev.prevent_default();
                on_click(ev.shift_key());
            }>
            <input type="checkbox" class="pick" aria-label=format!("Select {name}") prop:checked=is_selected
                on:click=move |ev: MouseEvent| {
                    ev.stop_propagation();
                    on_click(ev.shift_key());
                } />
            <a class="open" href=format!("/projects/{project}/images/{id}")>
                <img loading="lazy" alt="" src=format!("/api/images/{id}/thumbnail") />
                <span class="caption">
                    <span class=if image.annotated { "status-dot done" } else { "status-dot" }
                        title=if image.annotated { "Annotated" } else { "Not annotated" }></span>
                    <span class="name">{name.clone()}</span>
                    {(image.annotations > 0).then(|| view! {
                        <span class="count mono" title=format!("{} annotations", image.annotations)>{image.annotations}</span>
                    })}
                </span>
            </a>
        </li>
    }
}

/// First/previous/next/last buttons, a page number field to jump to, and the page size.
#[component]
fn Pager(
    offset: RwSignal<i64>,
    page_size: RwSignal<i64>,
    total: RwSignal<i64>,
    anchor: RwSignal<Option<usize>>,
) -> impl IntoView {
    let pages = move || ((total.get() + page_size.get() - 1) / page_size.get()).max(1);
    let current = move || offset.get() / page_size.get() + 1;
    let go_to = move |page: i64| {
        let page = page.clamp(1, pages());
        anchor.set(None);
        offset.set((page - 1) * page_size.get_untracked());
    };
    let range = move || {
        let first = (offset.get() + 1).min(total.get());
        let last = (offset.get() + page_size.get()).min(total.get());
        format!("{first}–{last} of {}", total.get())
    };
    view! {
        <nav class="pager" aria-label="Pages">
            <span class="mono muted">{range}</span>
            <button class="icon" aria-label="First page" title="First page" disabled={move || current() <= 1} on:click=move |_| go_to(1)>"«"</button>
            <button class="icon" aria-label="Previous page" title="Previous page (Page Up)" disabled={move || current() <= 1}
                on:click=move |_| go_to(current() - 1)>"‹"</button>
            <label class="page-jump">
                "Page"
                <input type="number" min="1" max=pages aria-label="Page number" prop:value=current
                    on:change:target={move |ev| match ev.target().value().trim().parse::<i64>() {
                        Ok(page) => go_to(page),
                        Err(_) => ev.target().set_value(&current().to_string()),
                    }} />
                {move || format!("of {}", pages())}
            </label>
            <button class="icon" aria-label="Next page" title="Next page (Page Down)" disabled={move || current() >= pages()}
                on:click=move |_| go_to(current() + 1)>"›"</button>
            <button class="icon" aria-label="Last page" title="Last page" disabled={move || current() >= pages()}
                on:click=move |_| go_to(pages())>"»"</button>
            <select aria-label="Images per page" on:change:target=move |ev| {
                let size = ev.target().value().parse().unwrap_or(60);
                // Keep the first image of the current page in view.
                offset.update(|o| *o = (*o / size) * size);
                page_size.set(size);
                anchor.set(None);
            }>
                {PAGE_SIZES.into_iter().map(|size| view! {
                    <option value=size.to_string() selected=move || page_size.get() == size>{format!("{size} / page")}</option>
                }).collect_view()}
            </select>
        </nav>
    }
}

/// Width, height, file size and date bounds. Each field applies when it is changed.
#[component]
fn FilterPanel(
    filter: RwSignal<ImageFilter>,
    set_filter: impl Fn(&dyn Fn(&mut ImageFilter)) + Copy + Send + Sync + 'static,
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
            <button on:click=move |_| set_filter(&|f| *f = ImageFilter { name: f.name.clone(), ..ImageFilter::default() })>"Clear filters"</button>
        </div>
    }
}

fn human_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    match bytes {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b if b >= 1e3 => format!("{:.0} KB", b / 1e3),
        b => format!("{b} B"),
    }
}
