use std::collections::BTreeSet;

use leptos::{ev::MouseEvent, prelude::*, task::spawn_local};
use rtag_core::{ImagePage, RemovalReport, RemovalRequest};

use crate::api;

const PAGE_SIZE: i64 = 60;
/// Largest page the server hands out; used when collecting the ids of every image.
const MAX_PAGE_SIZE: i64 = 500;

/// Paged grid of the project's image thumbnails. `version` changes whenever images were added.
/// Images can be selected (checkbox, or click once something is selected; shift-click for a range)
/// and removed from the project.
#[component]
pub fn ImageSet(project: i64, version: RwSignal<u32>) -> impl IntoView {
    let offset = RwSignal::new(0_i64);
    let selected = RwSignal::new(BTreeSet::<i64>::new());
    let anchor = RwSignal::new(None::<usize>);
    let confirming = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let images = LocalResource::new(move || {
        let offset = offset.get();
        version.track();
        async move {
            api::get::<ImagePage>(&format!("/api/projects/{project}/images?offset={offset}&limit={PAGE_SIZE}")).await
        }
    });
    let total = move || match images.get() {
        Some(Ok(page)) => page.total,
        _ => 0,
    };
    let page_ids = move || match images.get() {
        Some(Ok(page)) => page.images.iter().map(|image| image.id).collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let clear = move || {
        selected.set(BTreeSet::new());
        anchor.set(None);
        confirming.set(false);
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
    let select_page = move |_| selected.update(|set| set.extend(page_ids()));
    let select_all = move |_| {
        busy.set(true);
        spawn_local(async move {
            let mut all = BTreeSet::new();
            let mut from = 0;
            loop {
                let url = format!("/api/projects/{project}/images?offset={from}&limit={MAX_PAGE_SIZE}");
                match api::get::<ImagePage>(&url).await {
                    Ok(page) => {
                        all.extend(page.images.iter().map(|image| image.id));
                        from += MAX_PAGE_SIZE;
                        if page.images.is_empty() || from >= page.total {
                            break;
                        }
                    }
                    Err(message) => {
                        error.set(Some(message));
                        break;
                    }
                }
            }
            selected.set(all);
            busy.set(false);
        });
    };
    let remove = move |_| {
        let request = RemovalRequest { ids: selected.get_untracked().into_iter().collect() };
        busy.set(true);
        spawn_local(async move {
            match api::post::<_, RemovalReport>(&format!("/api/projects/{project}/image-removals"), &request).await {
                Ok(report) => {
                    clear();
                    // Stay on the last page that still has images.
                    let left = (total() - report.removed as i64).max(0);
                    offset.update(|o| {
                        if *o >= left {
                            *o = ((left - 1).max(0) / PAGE_SIZE) * PAGE_SIZE;
                        }
                    });
                    version.update(|v| *v += 1);
                }
                Err(message) => error.set(Some(message)),
            }
            busy.set(false);
        });
    };

    let selection_bar = move || {
        let count = selected.read().len();
        let plural = |n: usize| if n == 1 { "image" } else { "images" };
        view! {
            <div class="selection-bar" role="toolbar" aria-label="Selection">
                {move || if confirming.get() {
                    view! {
                        <span class="grow">{format!("Remove {count} {} from this project? The files stay on disk and can be imported again.", plural(count))}</span>
                        <button on:click=move |_| confirming.set(false)>"Cancel"</button>
                        <button class="danger" disabled=move || busy.get() on:click=remove>
                            {move || if busy.get() { "Removing…" } else { "Remove" }}
                        </button>
                    }.into_any()
                } else {
                    view! {
                        <span class="grow"><strong>{format!("{count} selected")}</strong></span>
                        <button on:click=select_page>"Select page"</button>
                        <button disabled=move || busy.get() on:click=select_all>{move || format!("Select all {}", total())}</button>
                        <button on:click=move |_| clear()>"Clear"</button>
                        <button class="danger" on:click=move |_| confirming.set(true)>"Remove from project…"</button>
                    }.into_any()
                }}
            </div>
        }
    };

    let keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" if !selected.read_untracked().is_empty() => clear(),
        "Delete" if !selected.read_untracked().is_empty() => confirming.set(true),
        _ => {}
    };

    view! {
        <div class="image-set" on:keydown=keydown>
            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
            {move || match images.get() {
                None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(page)) if page.total == 0 => {
                    view! { <p class="muted">"No images yet. Use “Import images…” to add some from the server's image folder."</p> }
                        .into_any()
                }
                Some(Ok(page)) => {
                    let total = page.total;
                    let first = offset.get_untracked() + 1;
                    let last = offset.get_untracked() + page.images.len() as i64;
                    view! {
                        <div class="grid-head">
                            <Show when=move || !selected.read().is_empty()>{selection_bar}</Show>
                            <div class="pager">
                                <button class="icon" aria-label="Previous page" disabled=move || offset.get() == 0
                                    on:click=move |_| { anchor.set(None); offset.update(|o| *o = (*o - PAGE_SIZE).max(0)) }>"‹"</button>
                                <span class="mono">{format!("{first}–{last} of {total}")}</span>
                                <button class="icon" aria-label="Next page" disabled=move || offset.get() + PAGE_SIZE >= total
                                    on:click=move |_| { anchor.set(None); offset.update(|o| *o += PAGE_SIZE) }>"›"</button>
                            </div>
                        </div>
                        <ul class="grid" class:selecting=move || !selected.read().is_empty()>
                            {page.images.into_iter().enumerate().map(|(index, image)| {
                                let id = image.id;
                                let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
                                let is_selected = move || selected.read().contains(&id);
                                // Once something is selected, a click anywhere on a tile adds to the selection.
                                let tile_click = move |ev: MouseEvent| {
                                    if !selected.read_untracked().is_empty() {
                                        click(index, ev.shift_key());
                                    }
                                };
                                view! {
                                    <li title=image.path.clone() class:selected=is_selected on:click=tile_click>
                                        <input type="checkbox" class="pick" aria-label=format!("Select {name}")
                                            prop:checked=is_selected
                                            on:click=move |ev: MouseEvent| {
                                                ev.stop_propagation();
                                                click(index, ev.shift_key());
                                            } />
                                        <img loading="lazy" alt="" src=format!("/api/images/{id}/thumbnail") />
                                        <span class="caption">
                                            <span class=if image.annotated { "status-dot done" } else { "status-dot" }
                                                title=if image.annotated { "Annotated" } else { "Not annotated" }></span>
                                            <span class="name">{name.clone()}</span>
                                        </span>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }
                    .into_any()
                }
            }}
        </div>
    }
}
