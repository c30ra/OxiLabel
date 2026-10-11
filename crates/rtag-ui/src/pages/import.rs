use leptos::{prelude::*, task::spawn_local};
use rtag_core::{BrowseEntry, BrowseListing, EntryKind, ImportReport, ImportRequest};

use crate::api;

/// One ticked entry: its path relative to the image root and whether it is a folder.
#[derive(Clone, PartialEq)]
struct Picked {
    path: String,
    kind: EntryKind,
}

/// Modal that browses the server's image root, lets the user tick folders and images,
/// and imports the selection into `project`. `on_close` is called when it should disappear.
#[component]
pub fn ImportDialog(
    project: i64,
    project_name: String,
    on_imported: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let folder = RwSignal::new(String::new());
    let filter = RwSignal::new(String::new());
    let picked = RwSignal::new(Vec::<Picked>::new());
    let busy = RwSignal::new(false);
    let outcome = RwSignal::new(None::<Result<ImportReport, String>>);

    let project_param = project.to_string();
    let listing = LocalResource::new(move || {
        let path = folder.get();
        let project = project_param.clone();
        async move {
            api::get_with_query::<BrowseListing>(
                "/api/browse",
                &[("path", &path), ("project", &project)],
            )
            .await
        }
    });

    let open_folder = move |path: String| {
        filter.set(String::new());
        folder.set(path);
    };
    let toggle = move |entry: &BrowseEntry| {
        let item = Picked {
            path: entry.path.clone(),
            kind: entry.kind,
        };
        picked.update(|list| match list.iter().position(|p| p.path == item.path) {
            Some(index) => {
                list.remove(index);
            }
            None => list.push(item),
        });
    };
    let visible = move || -> Vec<BrowseEntry> {
        let needle = filter.get().to_lowercase();
        match listing.get() {
            Some(Ok(listing)) => listing
                .entries
                .into_iter()
                .filter(|entry| needle.is_empty() || entry.name.to_lowercase().contains(&needle))
                .collect(),
            _ => Vec::new(),
        }
    };
    let all_visible_picked = move || {
        let entries = visible();
        !entries.is_empty()
            && entries
                .iter()
                .all(|e| picked.read().iter().any(|p| p.path == e.path))
    };
    let toggle_all = move |checked: bool| {
        let entries = visible();
        picked.update(|list| {
            list.retain(|p| !entries.iter().any(|e| e.path == p.path));
            if checked {
                list.extend(entries.iter().map(|e| Picked {
                    path: e.path.clone(),
                    kind: e.kind,
                }));
            }
        });
    };

    let import_label = move || {
        let list = picked.read();
        let folders = list.iter().filter(|p| p.kind == EntryKind::Folder).count();
        let images = list.len() - folders;
        let part = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        match (folders, images) {
            (0, 0) => "Import".to_owned(),
            (0, i) => format!("Import {}", part(i, "image")),
            (f, 0) => format!("Import {}", part(f, "folder")),
            (f, i) => format!("Import {}, {}", part(f, "folder"), part(i, "image")),
        }
    };

    let start_import = move |_| {
        let request = ImportRequest {
            paths: picked.read().iter().map(|p| p.path.clone()).collect(),
        };
        busy.set(true);
        spawn_local(async move {
            let result =
                api::post::<_, ImportReport>(&format!("/api/projects/{project}/imports"), &request)
                    .await;
            if result.is_ok() {
                picked.set(Vec::new());
                on_imported();
                listing.refetch();
            }
            outcome.set(Some(result));
            busy.set(false);
        });
    };

    let breadcrumbs = move || {
        let path = folder.get();
        let mut crumbs = vec![("images".to_owned(), String::new())];
        let mut so_far = String::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            if !so_far.is_empty() {
                so_far.push('/');
            }
            so_far.push_str(part);
            crumbs.push((part.to_owned(), so_far.clone()));
        }
        crumbs
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="import-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="import-title">{format!("Import images into “{project_name}”")}</h2>
                        <p class="muted">"Browse the image folder on the server and tick folders or single images. Images stay where they are; nothing is copied."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>

                <div class="dialog-bar">
                    <nav class="crumbs" aria-label="Folder">
                        {move || {
                            let crumbs = breadcrumbs();
                            let last = crumbs.len() - 1;
                            crumbs.into_iter().enumerate().map(|(index, (name, path))| view! {
                                {(index > 0).then(|| view! { <span class="sep" aria-hidden="true">"/"</span> })}
                                <button class:current=index == last on:click=move |_| open_folder(path.clone())>{name}</button>
                            }).collect_view()
                        }}
                    </nav>
                    <input class="search" placeholder="Filter this folder" prop:value=filter
                        on:input:target=move |ev| filter.set(ev.target().value()) />
                </div>

                <Show when=move || outcome.get().is_none() fallback=move || view! { <ImportResult outcome=outcome /> }>
                    <div class="browser">
                        <label class="browser-head">
                            <input type="checkbox" aria-label="Select everything in this folder"
                                prop:checked=all_visible_picked on:change:target=move |ev| toggle_all(ev.target().checked()) />
                            <span class="grow">"Name"</span>
                            <span class="status">"In this project"</span>
                        </label>
                        {move || match listing.get() {
                            None => view! { <p class="browser-note">"Loading…"</p> }.into_any(),
                            Some(Err(message)) => view! { <p class="browser-note error" role="alert">{message}</p> }.into_any(),
                            Some(Ok(_)) if visible().is_empty() => view! { <p class="browser-note muted">"No folders or images here."</p> }.into_any(),
                            Some(Ok(_)) => visible().into_iter().map(|entry| {
                                let checked_path = entry.path.clone();
                                let row_path = checked_path.clone();
                                let row_picked = move || picked.read().iter().any(|p| p.path == row_path);
                                let is_checked = move || picked.read().iter().any(|p| p.path == checked_path);
                                let row_entry = entry.clone();
                                let open_path = entry.path.clone();
                                let (status, status_class) = match entry.imported {
                                    Some(true) => ("already imported", "status"),
                                    Some(false) => ("new", "status new"),
                                    None => ("", "status"),
                                };
                                view! {
                                    <div class="browser-row" class:picked=row_picked>
                                        <input type="checkbox" aria-label=format!("Select {}", entry.name)
                                            prop:checked=is_checked on:change=move |_| toggle(&row_entry) />
                                        {match entry.kind {
                                            EntryKind::Folder => view! {
                                                <button class="folder grow" on:click=move |_| open_folder(open_path.clone())>
                                                    <span class="folder-icon" aria-hidden="true"></span>
                                                    {entry.name.clone()}
                                                    <span class="chevron" aria-hidden="true">"›"</span>
                                                </button>
                                            }.into_any(),
                                            EntryKind::Image => view! {
                                                <span class="file grow">{entry.name.clone()}</span>
                                            }.into_any(),
                                        }}
                                        <span class=status_class>{status}</span>
                                    </div>
                                }
                            }).collect_view().into_any(),
                        }}
                    </div>
                </Show>

                <div class="dialog-foot">
                    <div class="chips">
                        <Show when=move || !picked.read().is_empty()>
                            <span class="muted">"Selected:"</span>
                        </Show>
                        <For each=move || picked.get() key=|p| p.path.clone() let:item>
                            <span class="chip">
                                {if item.kind == EntryKind::Folder { format!("{}/", item.path) } else { item.path.clone() }}
                                <button class="icon small" aria-label=format!("Remove {} from selection", item.path)
                                    on:click={
                                        let path = item.path.clone();
                                        move |_| picked.update(|list| list.retain(|p| p.path != path))
                                    }>"✕"</button>
                            </span>
                        </For>
                    </div>
                    <button on:click=move |_| on_close()>{move || if outcome.get().is_some() { "Close" } else { "Cancel" }}</button>
                    <Show when=move || outcome.get().is_none()>
                        <button class="primary" disabled=move || busy.get() || picked.read().is_empty() on:click=start_import>
                            {move || if busy.get() { "Importing…".to_owned() } else { import_label() }}
                        </button>
                    </Show>
                    <Show when=move || outcome.get().is_some()>
                        <button class="primary" on:click=move |_| outcome.set(None)>"Import more"</button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[component]
fn ImportResult(outcome: RwSignal<Option<Result<ImportReport, String>>>) -> impl IntoView {
    move || match outcome.get() {
        Some(Err(message)) => {
            view! { <p class="browser-note error" role="alert">{message}</p> }.into_any()
        }
        Some(Ok(report)) => {
            let failed = report.failed.len();
            view! {
                <div class="report" role="status">
                    <p>{format!(
                        "Added {}, skipped {} already in the project, ignored {} non-image files{}.",
                        report.added,
                        report.skipped,
                        report.unsupported,
                        if failed > 0 { format!(", {failed} unreadable") } else { String::new() },
                    )}</p>
                    {(failed > 0).then(|| view! {
                        <ul class="failures">
                            {report.failed.into_iter().map(|failure| view! {
                                <li><code>{failure.path}</code>": "{failure.reason}</li>
                            }).collect_view()}
                        </ul>
                    })}
                </div>
            }.into_any()
        }
        None => ().into_any(),
    }
}
