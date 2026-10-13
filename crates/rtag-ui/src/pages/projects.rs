//! The start page: every project as a card (progress, images, labels), and the "New project"
//! dialog, which can copy another project's labels and import a folder of images at once.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{components::A, hooks::use_navigate};
use rtag_core::{BrowseListing, EntryKind, FolderCount, ImportReport, ImportRequest, NewProject, Project, ProjectSummary};

use crate::api;

#[component]
pub fn ProjectsPage() -> impl IntoView {
    let projects = LocalResource::new(|| api::get::<Vec<ProjectSummary>>("/api/projects"));
    let creating = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let on_delete = move |project: ProjectSummary| {
        let question = format!("Delete project \"{}\" and its annotations? The image files stay.", project.name);
        if !window().confirm_with_message(&question).unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            error.set(api::delete(&format!("/api/projects/{}", project.id)).await.err());
            projects.refetch();
        });
    };

    view! {
        <header class="topbar">
            <A href="/" attr:class="brand">"rtag"</A>
        </header>
        <main class="page projects">
            <div class="page-head">
                <h1>"Projects"</h1>
                <button class="primary new-project" on:click=move |_| creating.set(true)>"New project"</button>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            {move || match projects.get() {
                None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(list)) => view! {
                    <div class="project-grid">
                        {list.into_iter().map(|project| view! { <ProjectCard project=project on_delete=on_delete /> }).collect_view()}
                        <button class="new-card" on:click=move |_| creating.set(true)>
                            <span class="plus" aria-hidden="true">"+"</span>
                            "New project"
                        </button>
                    </div>
                }.into_any(),
            }}
        </main>
        <Show when=move || creating.get()>
            {move || {
                let sources = projects.get().and_then(Result::ok).unwrap_or_default();
                view! { <NewProjectDialog sources=sources on_close=move || creating.set(false) /> }
            }}
        </Show>
    }
}

#[component]
fn ProjectCard(project: ProjectSummary, on_delete: impl Fn(ProjectSummary) + Copy + Send + Sync + 'static) -> impl IntoView {
    let percent = if project.images > 0 { project.annotated * 100 / project.images } else { 0 };
    let plural = |n: i64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let target = project.clone();
    view! {
        <a class="project-card" href=format!("/projects/{}", project.id)>
            <span class="title">
                <span class="name">{project.name.clone()}</span>
                <span class="percent">{format!("{percent}%")}</span>
            </span>
            <span class="bar"><span style:width=format!("{percent}%")></span></span>
            <span class="stats">
                <span>{plural(project.images, "image")}</span>
                <span>{format!("{} annotated", project.annotated)}</span>
                <span>{plural(project.label_colors.len() as i64, "label")}</span>
            </span>
            <span class="swatches">
                {project.label_colors.iter().map(|color| view! { <span class="swatch" style:background=color.clone()></span> }).collect_view()}
            </span>
            <button class="delete" title="Delete this project" aria-label=format!("Delete {}", project.name)
                on:click=move |ev| {
                    // The card is a link: deleting must not open it.
                    ev.prevent_default();
                    on_delete(target.clone());
                }>"×"</button>
        </a>
    }
}

/// Name, where the labels come from, and optionally a folder to import; creating opens the
/// new project.
#[component]
fn NewProjectDialog(sources: Vec<ProjectSummary>, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let name = RwSignal::new(String::new());
    // The project to copy labels from (the latest one with labels by default), or none.
    let with_labels: Vec<ProjectSummary> = sources.into_iter().filter(|p| !p.label_colors.is_empty()).collect();
    let copy_from = RwSignal::new(with_labels.last().map(|p| p.id));
    let copy = RwSignal::new(copy_from.get_untracked().is_some());
    // The folder to import, relative to the image root ("" is the root itself).
    let folder = RwSignal::new(None::<String>);
    let browsing = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let count = LocalResource::new(move || {
        let folder = folder.get();
        async move {
            match folder {
                Some(path) => api::get_with_query::<FolderCount>("/api/browse/count", &[("path", &path)]).await.map(Some),
                None => Ok(None),
            }
        }
    });
    let navigate = use_navigate();
    let navigate = StoredValue::new(navigate);

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewProject { name: name.get().trim().to_owned(), copy_labels_from: copy.get().then(|| copy_from.get()).flatten() };
        if new.name.is_empty() {
            error.set(Some("Give the project a name.".into()));
            return;
        }
        let folder = folder.get();
        busy.set(true);
        spawn_local(async move {
            let project = match api::post::<_, Project>("/api/projects", &new).await {
                Ok(project) => project,
                Err(message) => {
                    busy.set(false);
                    return error.set(Some(message));
                }
            };
            if let Some(path) = folder {
                let request = ImportRequest { paths: vec![path] };
                // The project exists either way; an import problem shows on its page later.
                api::post::<_, ImportReport>(&format!("/api/projects/{}/imports", project.id), &request).await.ok();
            }
            navigate.with_value(|navigate| navigate(&format!("/projects/{}", project.id), Default::default()));
        });
    };
    let count_text = move || match count.get() {
        Some(Ok(Some(found))) => format!("{} image{} found", found.images, if found.images == 1 { "" } else { "s" }),
        Some(Err(message)) => message,
        _ => String::new(),
    };
    let submit_text = move || match (folder.get(), count.get()) {
        (Some(_), Some(Ok(Some(found)))) => format!("Create and import {}", found.images),
        (Some(_), _) => "Create and import".to_owned(),
        _ => "Create".to_owned(),
    };
    let shown_folder = move || folder.get().map(|path| if path.is_empty() { "/".to_owned() } else { format!("{path}/") }).unwrap_or_default();

    view! {
        <div class="backdrop" on:click=move |ev| {
            // A click on the dark area outside the dialog closes it.
            if ev.target() == ev.current_target() { on_close() }
        }>
            <form class="dialog new-project-dialog" role="dialog" aria-label="New project" on:submit=create>
                <div class="dialog-head">
                    <h2>"New project"</h2>
                    <button type="button" class="icon" aria-label="Close" on:click=move |_| on_close()>"×"</button>
                </div>
                <div class="dialog-body">
                    <label class="field">
                        <span>"Name"</span>
                        <input class="project-name" autofocus prop:value=name on:input:target=move |ev| name.set(ev.target().value()) />
                    </label>

                    <fieldset class="field">
                        <legend>"Labels"</legend>
                        <label class="choice" class:chosen=move || copy.get() class:disabled=with_labels.is_empty()>
                            <input type="radio" name="labels" prop:checked=move || copy.get() disabled=with_labels.is_empty()
                                on:change=move |_| copy.set(true) />
                            <span class="grow">"Copy from a project"</span>
                            <select class="copy-from" disabled=move || !copy.get()
                                on:change:target=move |ev| copy_from.set(ev.target().value().parse().ok())>
                                {with_labels.iter().map(|p| {
                                    let id = p.id;
                                    let text = format!("{} · {} label{}", p.name, p.label_colors.len(), if p.label_colors.len() == 1 { "" } else { "s" });
                                    view! { <option value=id.to_string() selected=move || copy_from.get() == Some(id)>{text}</option> }
                                }).collect_view()}
                            </select>
                        </label>
                        <label class="choice" class:chosen=move || !copy.get()>
                            <input type="radio" name="labels" prop:checked=move || !copy.get() on:change=move |_| copy.set(false) />
                            <span>"Start empty"</span>
                            <span class="muted">"— add labels after"</span>
                        </label>
                    </fieldset>

                    <div class="field">
                        <span>"Images " <span class="muted">"(optional, from the server's image folder)"</span></span>
                        <span class="folder-row">
                            <input class="mono" readonly placeholder="No folder: import later" prop:value=shown_folder />
                            <Show when=move || folder.get().is_some()>
                                <button type="button" on:click=move |_| folder.set(None)>"Clear"</button>
                            </Show>
                            <button type="button" on:click=move |_| browsing.update(|on| *on = !*on)>"Browse…"</button>
                        </span>
                        <span class="folder-count">{count_text}</span>
                        <Show when=move || browsing.get()>
                            <FolderPicker on_pick=move |path| {
                                folder.set(Some(path));
                                browsing.set(false);
                            } />
                        </Show>
                    </div>

                    <Show when=move || error.get().is_some()>
                        <p class="error" role="alert">{move || error.get()}</p>
                    </Show>
                </div>
                <div class="dialog-foot">
                    <button type="button" on:click=move |_| on_close()>"Cancel"</button>
                    <button type="submit" class="primary" disabled=move || busy.get()>{submit_text}</button>
                </div>
            </form>
        </div>
    }
}

/// Folders of the image root, opened by clicking; "Use this folder" picks the one shown.
#[component]
fn FolderPicker(on_pick: impl Fn(String) + Copy + Send + Sync + 'static) -> impl IntoView {
    let path = RwSignal::new(String::new());
    let listing = LocalResource::new(move || {
        let path = path.get();
        async move { api::get_with_query::<BrowseListing>("/api/browse", &[("path", &path)]).await }
    });
    let parent = move || {
        let current = path.get();
        (!current.is_empty()).then(|| current.rsplit_once('/').map(|(up, _)| up.to_owned()).unwrap_or_default())
    };
    view! {
        <div class="folder-picker">
            <div class="picker-head">
                <span class="mono">{move || format!("/{}", path.get())}</span>
                <button type="button" class="primary" on:click=move |_| on_pick(path.get_untracked())>"Use this folder"</button>
            </div>
            <ul>
                {move || parent().map(|up| view! {
                    <li><button type="button" on:click=move |_| path.set(up.clone())>"‹ Up"</button></li>
                })}
                {move || match listing.get() {
                    Some(Ok(listing)) => {
                        let images = listing.entries.iter().filter(|e| e.kind == EntryKind::Image).count();
                        let folders: Vec<_> = listing.entries.into_iter().filter(|e| e.kind == EntryKind::Folder).collect();
                        view! {
                            {folders.into_iter().map(|entry| {
                                let target = entry.path.clone();
                                view! { <li><button type="button" class="folder" on:click=move |_| path.set(target.clone())>{entry.name}</button></li> }
                            }).collect_view()}
                            {(images > 0).then(|| view! { <li class="muted">{format!("{images} image{} here", if images == 1 { "" } else { "s" })}</li> })}
                        }.into_any()
                    }
                    Some(Err(message)) => view! { <li class="error">{message}</li> }.into_any(),
                    None => view! { <li class="muted">"Loading…"</li> }.into_any(),
                }}
            </ul>
        </div>
    }
}
