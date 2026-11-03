//! The start page: every project as a card (progress, images, labels), and the "New project"
//! dialog, which can copy another project's labels and import a folder or images at once.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{components::A, hooks::use_navigate};
use oxilabel_core::{Label, LabelDefinition, NewProject, Project, ProjectDeletionPreview, ProjectDeletionReport, ProjectSummary, ShapeType};

use super::{
    archive::ImportArchiveDialog,
    image_pick::{ImagePick, ImagePicker, import_pick, pick_count},
};
use crate::{
    account::{AccountMenu, is_admin},
    api,
};

#[component]
pub fn ProjectsPage() -> impl IntoView {
    let projects = LocalResource::new(|| api::get::<Vec<ProjectSummary>>("/api/projects"));
    let creating = RwSignal::new(false);
    let importing_archive = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let deleting = RwSignal::new(None::<ProjectSummary>);
    let admin = is_admin();

    let on_delete = move |project: ProjectSummary| deleting.set(Some(project));

    view! {
        <header class="topbar">
            <A href="/" attr:class="brand">"OxiLabel"</A>
            <AccountMenu />
        </header>
        <main class="page projects">
            <div class="page-head">
                <h1>"Projects"</h1>
                {admin.then(|| view! {
                    <div class="actions">
                        <button class="import-archive" on:click=move |_| importing_archive.set(true)>"Import archive…"</button>
                        <button class="primary new-project" on:click=move |_| creating.set(true)>"New project"</button>
                    </div>
                })}
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            {move || match projects.get() {
                None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(list)) => view! {
                    <div class="project-grid">
                        {list.into_iter().map(|project| view! { <ProjectCard project=project admin=admin on_delete=on_delete /> }).collect_view()}
                        {admin.then(|| view! {
                            <button class="new-card" on:click=move |_| creating.set(true)>
                                <span class="plus" aria-hidden="true">"+"</span>
                                "New project"
                            </button>
                        })}
                    </div>
                }.into_any(),
            }}
        </main>
        <Show when=move || importing_archive.get()>
            <ImportArchiveDialog on_imported=move || projects.refetch() on_close=move || importing_archive.set(false) />
        </Show>
        {move || deleting.get().map(|project| view! {
            <DeleteProjectDialog project=project on_deleted=move || projects.refetch() on_close=move || deleting.set(None) />
        })}
        <Show when=move || creating.get()>
            {move || {
                let sources = projects.get().and_then(Result::ok).unwrap_or_default();
                view! { <NewProjectDialog sources=sources on_close=move || creating.set(false) /> }
            }}
        </Show>
    }
}

#[component]
fn ProjectCard(project: ProjectSummary, admin: bool, on_delete: impl Fn(ProjectSummary) + Copy + Send + Sync + 'static) -> impl IntoView {
    let percent = if project.images > 0 { project.annotated * 100 / project.images } else { 0 };
    let plural = |n: i64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let target = project.clone();
    view! {
        <a class="project-card" href=if admin { format!("/projects/{}", project.id) } else { format!("/projects/{}/jobs", project.id) }>
            <span class="title">
                <span class="name">{project.name.clone()}</span>
                <span class="percent">{format!("{percent}%")}</span>
            </span>
            <span class="bar"><span style:width=format!("{percent}%")></span></span>
            <span class="stats">
                <span>{plural(project.images, "image")}</span>
                <span>{format!("{} annotated", project.annotated)}</span>
                <span>{plural(project.label_colors.len() as i64, "label")}</span>
                {(project.jobs > 0).then(|| view! { <span class="jobs">{plural(project.jobs, "job")}</span> })}
            </span>
            <span class="swatches">
                {project.label_colors.iter().map(|color| view! { <span class="swatch" style:background=color.clone()></span> }).collect_view()}
            </span>
            {admin.then(|| view! {
                <button class="delete" title="Delete this project" aria-label=format!("Delete {}", project.name)
                    on:click=move |ev| {
                        // The card is a link: deleting must not open it.
                        ev.prevent_default();
                        on_delete(target.clone());
                    }>"×"</button>
            })}
        </a>
    }
}

/// Confirms deleting a project, which can take its stored copies along; originals always stay.
/// Kept or undeletable copies are listed afterwards.
#[component]
fn DeleteProjectDialog(
    project: ProjectSummary,
    on_deleted: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let id = project.id;
    let preview = LocalResource::new(move || {
        let url = format!("/api/projects/{id}/deletion");
        async move { api::get::<ProjectDeletionPreview>(&url).await }
    });
    let with_copies = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let report = RwSignal::new(None::<ProjectDeletionReport>);
    let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });

    let confirm = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if report.get_untracked().is_some() {
            return on_close();
        }
        busy.set(true);
        let url = format!("/api/projects/{id}?delete_stored_copies={}", with_copies.get_untracked());
        spawn_local(async move {
            let result = api::delete_json::<ProjectDeletionReport>(&url).await;
            busy.set(false);
            match result {
                Ok(done) => {
                    on_deleted();
                    if done.kept.is_empty() && done.failed.is_empty() { on_close() } else { report.set(Some(done)) }
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };

    view! {
        <div class="backdrop" on:click=move |ev| {
            if ev.target() == ev.current_target() { on_close() }
        }>
            <form class="dialog delete-project-dialog" role="dialog" aria-label="Delete project" on:submit=confirm>
                <div class="dialog-head">
                    <h2>{format!("Delete “{}”", project.name)}</h2>
                    <button type="button" class="icon" aria-label="Close" on:click=move |_| on_close()>"×"</button>
                </div>
                <div class="dialog-body">
                    {move || match (report.get(), preview.get()) {
                        (Some(done), _) => view! {
                            <p class="deletion-report">{format!("The project is deleted, and {}.", plural(done.deleted.len() as u64, "stored copy", "stored copies"))}</p>
                            <ul class="deletion-files">
                                {done.kept.into_iter().map(|kept| view! { <li>{format!("Kept {}: used by “{}”", kept.path, kept.project)}</li> }).collect_view()}
                                {done.failed.into_iter().map(|failed| view! { <li class="error">{format!("Not deleted {}: {}", failed.path, failed.reason)}</li> }).collect_view()}
                            </ul>
                        }.into_any(),
                        (None, None) => view! { <p class="muted">"Loading…"</p> }.into_any(),
                        (None, Some(Err(message))) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                        (None, Some(Ok(files))) => view! {
                            <p>"Its images, annotations, labels and jobs are deleted."</p>
                            <label class="choice" class:disabled={files.stored_copies == 0}>
                                <input type="checkbox" class="delete-stored-copies" disabled={files.stored_copies == 0}
                                    prop:checked=move || with_copies.get() on:change:target=move |ev| with_copies.set(ev.target().checked()) />
                                <span>{format!("Also delete {} the server keeps for it", plural(files.stored_copies, "stored copy", "stored copies"))}</span>
                            </label>
                            {(files.shared_copies > 0).then(|| view! {
                                <p class="muted">{format!("{}: another project uses them.", plural(files.shared_copies, "stored copy stays", "stored copies stay"))}</p>
                            })}
                            {(files.originals > 0).then(|| view! {
                                <p class="warning originals-stay">{format!("{} at their locations on disk.", plural(files.originals, "original stays", "originals stay"))}</p>
                            })}
                        }.into_any(),
                    }}
                    <Show when=move || error.get().is_some()>
                        <p class="error" role="alert">{move || error.get()}</p>
                    </Show>
                </div>
                <div class="dialog-foot">
                    {move || report.get().is_none().then(|| view! { <button type="button" on:click=move |_| on_close()>"Cancel"</button> })}
                    <button type="submit" class="primary confirm-delete" disabled=move || busy.get() || preview.get().is_none()>
                        {move || if report.get().is_some() { "Close" } else { "Delete project" }}
                    </button>
                </div>
            </form>
        </div>
    }
}

/// Name, where the labels come from, and optionally images to import; creating opens the new
/// project.
#[component]
fn NewProjectDialog(sources: Vec<ProjectSummary>, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let name = RwSignal::new(String::new());
    // The project to copy labels from (the latest one with labels by default), or none.
    let with_labels: Vec<ProjectSummary> = sources.into_iter().filter(|p| !p.label_colors.is_empty()).collect();
    let copy_from = RwSignal::new(with_labels.last().map(|p| p.id));
    let copy = RwSignal::new(copy_from.get_untracked().is_some());
    // Otherwise the labels typed here (names; colors and keys are given in order).
    let new_labels = RwSignal::new(Vec::<String>::new());
    let label_draft = RwSignal::new(String::new());
    let add_label = move || {
        let name = label_draft.get_untracked().trim().to_owned();
        if !name.is_empty() && !new_labels.with_untracked(|list| list.contains(&name)) {
            new_labels.update(|list| list.push(name));
        }
        label_draft.set(String::new());
    };
    let pick = RwSignal::new_local(None::<ImagePick>);
    let count = pick_count(pick);
    let busy = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let navigate = StoredValue::new(use_navigate());

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewProject { name: name.get().trim().to_owned(), copy_labels_from: copy.get().then(|| copy_from.get()).flatten() };
        if new.name.is_empty() {
            error.set(Some("Give the project a name.".into()));
            return;
        }
        let picked = pick.get();
        let labels: Vec<LabelDefinition> = if copy.get() {
            Vec::new()
        } else {
            new_labels.get().into_iter().enumerate().map(|(index, name)| new_label(index, name)).collect()
        };
        busy.set(Some("Creating…".into()));
        spawn_local(async move {
            let project = match api::post::<_, Project>("/api/projects", &new).await {
                Ok(project) => project,
                Err(message) => {
                    busy.set(None);
                    return error.set(Some(message));
                }
            };
            for definition in &labels {
                if let Err(message) = api::post::<_, Label>(&format!("/api/projects/{}/labels", project.id), definition).await {
                    busy.set(None);
                    return error.set(Some(format!("“{}” was created, but not its label “{}”: {message}", project.name, definition.name)));
                }
            }
            if let Some(chosen) = picked {
                if let Err(message) = import_pick(project.id, chosen, move |step| busy.set(Some(step))).await {
                    // The project exists; say what happened rather than leave silently.
                    busy.set(None);
                    return error.set(Some(format!("“{}” was created, but the import stopped: {message}", project.name)));
                }
            }
            navigate.with_value(|navigate| navigate(&format!("/projects/{}", project.id), Default::default()));
        });
    };
    let submit_text = move || {
        if let Some(step) = busy.get() {
            return step;
        }
        let uploads = pick.with(|pick| pick.as_ref().map(ImagePick::uploads));
        match (uploads, count.get()) {
            (Some(true), Some(Ok(Some(n)))) => format!("Create and upload {n}"),
            (Some(false), Some(Ok(Some(n)))) => format!("Create and import {n}"),
            (Some(_), _) => "Create and import".to_owned(),
            (None, _) => "Create".to_owned(),
        }
    };

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
                            <input type="radio" name="labels" value="copy" prop:checked=move || copy.get() disabled=with_labels.is_empty()
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
                        <div class="choice new-labels" class:chosen=move || !copy.get()>
                            <label class="choice-head">
                                <input type="radio" name="labels" value="new" prop:checked=move || !copy.get() on:change=move |_| copy.set(false) />
                                <span>"New labels"</span>
                                <span class="muted">"— or add them later"</span>
                            </label>
                            <Show when=move || !copy.get()>
                                <ul>
                                    {move || new_labels.get().into_iter().enumerate().map(|(index, name)| {
                                        let (color, key) = (LABEL_COLORS[index % LABEL_COLORS.len()], hotkey(index));
                                        let gone = name.clone();
                                        view! {
                                            <li>
                                                <span class="swatch" style:background=color></span>
                                                <span class="name">{name}</span>
                                                {key.map(|key| view! { <kbd>{key}</kbd> })}
                                                <button type="button" aria-label=format!("Remove {gone}")
                                                    on:click=move |_| new_labels.update(|list| list.retain(|n| *n != gone))>"×"</button>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                                <input class="new-label" placeholder="Label name, Enter adds" prop:value=label_draft
                                    on:input:target=move |ev| label_draft.set(ev.target().value())
                                    on:keydown=move |ev| if ev.key() == "Enter" {
                                        // Enter adds the label rather than submitting the form.
                                        ev.prevent_default();
                                        add_label();
                                    } />
                                <span class="muted hint">"Every shape is allowed; shapes, attributes and the ignore label are set on the Labels tab."</span>
                            </Show>
                        </div>
                    </fieldset>

                    <div class="field">
                        <span>"Images " <span class="muted">"(optional)"</span></span>
                        <ImagePicker pick=pick count=count placeholder="Nothing chosen: import later" />
                    </div>

                    <Show when=move || error.get().is_some()>
                        <p class="error" role="alert">{move || error.get()}</p>
                    </Show>
                </div>
                <div class="dialog-foot">
                    <button type="button" on:click=move |_| on_close()>"Cancel"</button>
                    <button type="submit" class="primary" disabled=move || busy.get().is_some()>{submit_text}</button>
                </div>
            </form>
        </div>
    }
}

/// Colors new labels get in turn.
const LABEL_COLORS: [&str; 10] = ["#e74c3c", "#3498db", "#f1c40f", "#9b59b6", "#1abc9c", "#e67e22", "#4fb286", "#ff77aa", "#34495e", "#7f8c8d"];

/// The number key of the `index`th new label: 1–9, then 0, then none.
fn hotkey(index: usize) -> Option<String> {
    match index {
        0..=8 => Some((index + 1).to_string()),
        9 => Some("0".to_owned()),
        _ => None,
    }
}

fn new_label(index: usize, name: String) -> LabelDefinition {
    LabelDefinition {
        name,
        color: LABEL_COLORS[index % LABEL_COLORS.len()].to_owned(),
        hotkey: hotkey(index),
        shape_types: ShapeType::ALL.to_vec(),
        ignore: false,
        attributes: Vec::new(),
        footprint: oxilabel_core::Footprint::default(),
        show_footprint: true,
        constraints: Default::default(),
    }
}
