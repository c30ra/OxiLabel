use std::collections::BTreeSet;

use leptos::{ev::MouseEvent, prelude::*, task::spawn_local};
use oxilabel_core::{Image, ImageFilter, ImagePage, Label, RemovalReport, RemovalRequest, job::Job, view::View};
use serde::{Deserialize, Serialize};

use super::jobs::{CreateJobsDialog, job_name};
use crate::{
    account::is_admin,
    api,
    prefs::{prefs, press},
    image_filter::{self, FilterPanel},
    viewer::TileViewer,
};
use oxilabel_core::keymap::Action;

const PAGE_SIZES: [i64; 4] = [30, 60, 120, 240];
/// The grid's page, kept in session storage so it survives reloading the page (the filter is
/// kept with the editor's, see `image_filter`).
#[derive(Serialize, Deserialize)]
struct Saved {
    offset: i64,
    page_size: i64,
    filters_open: bool,
}

fn storage_key(project: i64) -> String {
    format!("oxilabel.image-grid.{project}")
}

fn storage() -> Option<web_sys::Storage> {
    window().session_storage().ok().flatten()
}

/// Paged, filterable grid of the project's image thumbnails. `version` changes whenever images were added.
/// Images can be selected (checkbox, or click once something is selected; shift-click for a range;
/// A selects all matching images, Alt+A clears) and removed from the project.
#[component]
pub fn ImageSet(project: i64, version: RwSignal<u32>, selected: RwSignal<BTreeSet<i64>>) -> impl IntoView {
    let saved = storage()
        .and_then(|storage| storage.get_item(&storage_key(project)).ok().flatten())
        .and_then(|text| serde_json::from_str::<Saved>(&text).ok());
    let offset = RwSignal::new(saved.as_ref().map_or(0, |saved| saved.offset.max(0)));
    let page_size = RwSignal::new(saved.as_ref().map_or(60, |saved| saved.page_size).clamp(1, 500));
    let filters_open = RwSignal::new(saved.as_ref().is_some_and(|saved| saved.filters_open));
    let filter = image_filter::shared(project);
    Effect::new(move |_| {
        let saved = Saved { offset: offset.get(), page_size: page_size.get(), filters_open: filters_open.get() };
        if let (Some(storage), Ok(text)) = (storage(), serde_json::to_string(&saved)) {
            storage.set_item(&storage_key(project), &text).ok();
        }
    });
    let labels = LocalResource::new(move || async move {
        api::get::<Vec<Label>>(&format!("/api/projects/{project}/labels")).await.unwrap_or_default()
    });
    let preview = RwSignal::new(None::<Image>);
    // Annotations the confirmed removal would discard; `None` while they are counted.
    let doomed = RwSignal::new(None::<u64>);
    let total = RwSignal::new(0_i64);
    let anchor = RwSignal::new(None::<usize>);
    let confirming = RwSignal::new(false);
    let admin = is_admin();
    let creating_jobs = RwSignal::new(false);
    let jobs = LocalResource::new(move || {
        version.track();
        async move { api::get::<Vec<Job>>(&format!("/api/projects/{project}/jobs")).await }
    });
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
            // The filter may have changed in the editor since this page was left.
            let size = page_size.get_untracked();
            if page.total > 0 && offset.get_untracked() >= page.total {
                offset.set(((page.total - 1) / size) * size);
            }
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
    let confirm = move || {
        confirming.set(true);
        doomed.set(None);
        let request = RemovalRequest { ids: selected.get_untracked().into_iter().collect(), dry_run: true };
        spawn_local(async move {
            if let Ok(report) = api::post::<_, RemovalReport>(&format!("/api/projects/{project}/image-removals"), &request).await {
                doomed.set(Some(report.annotations));
            }
        });
    };
    let remove = move |_| {
        let request = RemovalRequest {
            ids: selected.get_untracked().into_iter().collect(),
            dry_run: false,
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
    let keymap = prefs().keymap;
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
        {
            return;
        }
        let has_selection = !selected.read_untracked().is_empty();
        let size = page_size.get_untracked();
        let keys = keymap.get_untracked();
        let press = press(&ev);
        let is = |action| keys.matches(action, &press);
        if is(Action::SelectNone) {
            clear();
        } else if is(Action::SelectAll) {
            select_all();
        } else if is(Action::Cancel) && has_selection {
            clear();
        } else if is(Action::Delete) && has_selection && admin {
            confirm();
        } else if is(Action::NextPage) {
            offset.update(|o| *o = (*o + size).min(((total.get_untracked() - 1).max(0) / size) * size));
        } else if is(Action::PreviousPage) {
            offset.update(|o| *o = (*o - size).max(0));
        } else {
            return;
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let active_filters = move || image_filter::panel_count(&filter.read());

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
                <select class="job-filter" aria-label="Job"
                    on:change:target=move |ev| {
                        let value = ev.target().value();
                        set_filter(&|f| {
                            f.without_job = value == "none";
                            f.job = value.parse().ok();
                        })
                    }>
                    <option value="" selected=move || filter.read().job.is_none() && !filter.read().without_job>"Any job"</option>
                    {admin.then(|| view! { <option value="none" selected=move || filter.read().without_job>"No job"</option> })}
                    {move || jobs.get().and_then(Result::ok).unwrap_or_default().into_iter().map(|job| {
                        let id = job.id;
                        view! { <option value=id.to_string() selected=move || filter.read().job == Some(id)>{job_name(&job)}</option> }
                    }).collect_view()}
                </select>
                <button title="Select every image matching the filter (A)" disabled=move || busy.get() || total.get() == 0
                    on:click=move |_| select_all()>"Select all"</button>
                <Pager offset=offset page_size=page_size total=total anchor=anchor />
            </div>

            <Show when=move || filters_open.get()>
                <FilterPanel filter=filter set_filter=set_filter labels=Signal::derive(move || labels.get().unwrap_or_default()) />
            </Show>

            <Show when=move || !selected.read().is_empty()>
                <div class="selection-bar" role="toolbar" aria-label="Selection">
                    {move || {
                        let count = selected.read().len();
                        let noun = if count == 1 { "image" } else { "images" };
                        if confirming.get() {
                            view! {
                                <span class="grow">
                                    {format!("Remove {count} {noun} from this project? The files stay on disk and can be imported again.")}
                                    {move || match doomed.get() {
                                        Some(0) => None,
                                        Some(1) => Some(view! { " " <strong class="warning">"Its 1 annotation is discarded."</strong> }.into_any()),
                                        Some(n) => Some(view! { " " <strong class="warning">{format!("Their {n} annotations are discarded.")}</strong> }.into_any()),
                                        None => Some(view! { <span class="muted">" Counting annotations…"</span> }.into_any()),
                                    }}
                                </span>
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
                                {admin.then(|| view! {
                                    <button class="create-jobs" on:click=move |_| creating_jobs.set(true)>"Create jobs…"</button>
                                    <button class="danger" title="Delete" on:click=move |_| confirm()>"Remove from project…"</button>
                                })}
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
                Some(Ok(page)) if filter.read_untracked().same_name => {
                    // One grid per file name, with the details to choose which namesake to keep.
                    let mut groups: Vec<(String, Vec<(usize, Image)>)> = Vec::new();
                    for (index, image) in page.images.into_iter().enumerate() {
                        let name = file_name(&image.path).to_owned();
                        match groups.last_mut() {
                            Some((last, members)) if *last == name => members.push((index, image)),
                            _ => groups.push((name, vec![(index, image)])),
                        }
                    }
                    groups.into_iter().map(|(name, members)| view! {
                        <section class="name-group" aria-label=name.clone()>
                            <h4><span class="mono">{name.clone()}</span><span class="muted">{format!(" · {} on this page", members.len())}</span></h4>
                            <ul class="grid detailed" class:selecting=move || !selected.read().is_empty()>
                                {members.into_iter().map(|(index, image)| view! {
                                    <Tile project=project image=image selected=selected preview=Some(preview) on_click=move |shift| click(index, shift) />
                                }).collect_view()}
                            </ul>
                        </section>
                    }).collect_view().into_any()
                }
                Some(Ok(page)) => view! {
                    <ul class="grid" class:selecting=move || !selected.read().is_empty()>
                        {page.images.into_iter().enumerate().map(|(index, image)| {
                            view! { <Tile project=project image=image selected=selected preview=None on_click=move |shift| click(index, shift) /> }
                        }).collect_view()}
                    </ul>
                }
                .into_any(),
            }}
            {move || preview.get().map(|image| view! { <Preview project=project image=image on_close=move || preview.set(None) /> })}
            <Show when=move || creating_jobs.get()>
                <CreateJobsDialog project=project selected=selected
                    on_created=move || version.update(|v| *v += 1)
                    on_close=move || creating_jobs.set(false) />
            </Show>
        </div>
    }
}

/// One image of the grid. With `preview` the tile lists the image's file details, and clicking the
/// thumbnail opens it there (zoomable) instead of in the editor.
#[component]
fn Tile(
    project: i64,
    image: Image,
    selected: RwSignal<BTreeSet<i64>>,
    preview: Option<RwSignal<Option<Image>>>,
    on_click: impl Fn(bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let id = image.id;
    let name = file_name(&image.path).to_owned();
    let details = file_details(&image);
    let is_selected = move || selected.read().contains(&id);
    let shown = image.clone();
    let tooltip = details.iter().map(|(_, value)| value.as_str()).collect::<Vec<_>>().join("\n");
    view! {
        // A click opens the image, unless something is selected: then it adds to the selection.
        <li title=tooltip class:selected=is_selected
            on:click=move |ev: MouseEvent| if !selected.read_untracked().is_empty() {
                ev.prevent_default();
                on_click(ev.shift_key());
            }>
            <input type="checkbox" class="pick" aria-label=format!("Select {name}") prop:checked=is_selected
                on:click=move |ev: MouseEvent| {
                    ev.stop_propagation();
                    on_click(ev.shift_key());
                } />
            <a class="open" href=format!("/projects/{project}/images/{id}")
                on:click=move |ev: MouseEvent| if let Some(preview) = preview.filter(|_| selected.read_untracked().is_empty()) {
                    ev.prevent_default();
                    preview.set(Some(shown.clone()));
                }>
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
            {preview.is_some().then(|| view! { <Details details=details.clone() /> })}
        </li>
    }
}

/// The file details of an image as (term, value) pairs: path, dimensions, size and dates.
fn file_details(image: &Image) -> Vec<(&'static str, String)> {
    let mut details = vec![("Path", image.path.clone()), ("Dimensions", format!("{} × {} px", image.width, image.height))];
    details.extend(image.size.map(|size| ("Size", human_size(size))));
    details.extend(image.created.clone().map(|date| ("Created", date)));
    details.extend(image.modified.clone().map(|date| ("Modified", date)));
    details
}

#[component]
fn Details(details: Vec<(&'static str, String)>) -> impl IntoView {
    view! {
        <dl class="details">
            {details.into_iter().map(|(term, value)| view! { <dt>{term}</dt><dd class="mono" title=value.clone()>{value.clone()}</dd> }).collect_view()}
        </dl>
    }
}

/// The image at full resolution in a dialog: scroll to zoom, drag with the middle button or
/// Space held to pan, Home to fit.
#[component]
fn Preview(project: i64, image: Image, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let id = image.id;
    let details = file_details(&image);
    let title = file_name(&image.path).to_owned();
    let view = RwSignal::new(None::<View>);
    let size = RwSignal::new((0.0, 0.0));
    let space_held = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let keys = window_event_listener(leptos::ev::keydown, move |ev| if ev.key() == "Escape" { on_close() });
    on_cleanup(move || keys.remove());
    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog preview-dialog" role="dialog" aria-modal="true" aria-labelledby="preview-title"
                on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <div>
                        <h2 id="preview-title">{title}</h2>
                        <p class="muted">"Scroll to zoom, middle-drag to pan, Home to fit."</p>
                    </div>
                    <a class="button" href=format!("/projects/{project}/images/{id}")>"Open in editor"</a>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <div class="preview-body">
                    <TileViewer image=image view=view size=size space_held=space_held
                        filter=Signal::stored(None) css_filter=Signal::stored(String::new()) loading=loading>
                        ""
                    </TileViewer>
                    <Details details=details />
                </div>
            </div>
        </div>
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
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

fn human_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    match bytes {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b if b >= 1e3 => format!("{:.0} KB", b / 1e3),
        b => format!("{b} B"),
    }
}
