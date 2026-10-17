//! The start page: every project as a card (progress, images, labels), and the "New project"
//! dialog, which can copy another project's labels and import a folder of images at once.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{components::A, hooks::use_navigate};
use rtag_core::{FolderCount, ImportReport, ImportRequest, Label, LabelDefinition, NewProject, Project, ProjectSummary, ShapeType};

use crate::{
    account::{AccountMenu, is_admin},
    api,
};

#[component]
pub fn ProjectsPage() -> impl IntoView {
    let projects = LocalResource::new(|| api::get::<Vec<ProjectSummary>>("/api/projects"));
    let creating = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let admin = is_admin();

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
            <AccountMenu />
        </header>
        <main class="page projects">
            <div class="page-head">
                <h1>"Projects"</h1>
                {admin.then(|| view! { <button class="primary new-project" on:click=move |_| creating.set(true)>"New project"</button> })}
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

/// The folder of images a new project starts with (ADR 0003): one on the server's machine,
/// imported where it is, or one picked in this browser, whose images are uploaded.
#[derive(Clone)]
enum Pick {
    Host(String),
    Browser { name: String, files: Vec<web_sys::File> },
}

/// Images uploaded per request, so a big folder goes in pieces with progress.
const UPLOAD_BATCH: usize = 16;
const IMAGE_EXTENSIONS: [&str; 7] = ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];

/// Name, where the labels come from, and optionally a folder to import; creating opens the
/// new project.
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
    let pick = RwSignal::new_local(None::<Pick>);
    let busy = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let browser_input = NodeRef::<leptos::html::Input>::new();
    let count = LocalResource::new(move || {
        let path = pick.with(|pick| match pick {
            Some(Pick::Host(path)) => Some(path.clone()),
            _ => None,
        });
        async move {
            match path {
                Some(path) => api::get_with_query::<FolderCount>("/api/browse/count", &[("path", &path)]).await.map(Some),
                None => Ok(None),
            }
        }
    });
    let navigate = StoredValue::new(use_navigate());

    // A folder dialog rather than a file one (there is no view! syntax for this attribute).
    Effect::new(move |_| {
        if let Some(input) = browser_input.get() {
            input.set_attribute("webkitdirectory", "").ok();
        }
    });
    let pick_in_browser = move || {
        if let Some(input) = browser_input.get_untracked() {
            input.click();
        }
    };
    let browse = move |_| {
        spawn_local(async move {
            // On the server's machine the server shows the desktop's dialog.
            let local = api::get::<Host>("/api/host").await.is_ok_and(|host| host.local);
            if !local {
                return pick_in_browser();
            }
            match api::post::<_, Picked>("/api/host/folder-dialog", &()).await {
                Ok(Picked { path: Some(path) }) => pick.set(Some(Pick::Host(path))),
                Ok(Picked { path: None }) => {}
                // No dialog on this server (e.g. headless): the browser's own, with an upload.
                Err(_) => pick_in_browser(),
            }
        });
    };
    let on_browser_pick = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = event_target(&ev);
        let Some(list) = input.files() else { return };
        let files: Vec<web_sys::File> = (0..list.length()).filter_map(|i| list.get(i)).filter(|file| is_image(&file.name())).collect();
        // The picked folder's name, from the files' paths inside it (none if only files came).
        let name = files
            .first()
            .map(relative_path)
            .and_then(|path| path.split_once('/').map(|(folder, _)| folder.to_owned()))
            .unwrap_or_else(|| "Selected files".to_owned());
        pick.set(Some(Pick::Browser { name, files }));
    };

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
            match picked {
                Some(Pick::Host(path)) => {
                    busy.set(Some("Importing…".into()));
                    let request = ImportRequest { paths: vec![path] };
                    // The project exists either way; an import problem can be retried on its page.
                    api::post::<_, ImportReport>(&format!("/api/projects/{}/imports", project.id), &request).await.ok();
                }
                Some(Pick::Browser { files, .. }) => {
                    let url = format!("/api/projects/{}/uploads", project.id);
                    for (done, batch) in files.chunks(UPLOAD_BATCH).enumerate() {
                        busy.set(Some(format!("Uploading {} / {}…", done * UPLOAD_BATCH, files.len())));
                        let Ok(form) = web_sys::FormData::new() else { break };
                        for file in batch {
                            form.append_with_blob_and_filename("file", file, &relative_path(file)).ok();
                        }
                        if let Err(message) = api::post_form::<ImportReport>(&url, form).await {
                            // The project exists; say what happened rather than leave silently.
                            busy.set(None);
                            return error.set(Some(format!("“{}” was created, but the upload stopped: {message}", project.name)));
                        }
                    }
                }
                None => {}
            }
            navigate.with_value(|navigate| navigate(&format!("/projects/{}", project.id), Default::default()));
        });
    };
    let count_text = move || match (pick.get(), count.get()) {
        (Some(Pick::Browser { files, .. }), _) => {
            format!("{} image{} to upload from this browser", files.len(), if files.len() == 1 { "" } else { "s" })
        }
        (Some(Pick::Host(_)), Some(Ok(Some(found)))) => format!("{} image{} found", found.images, if found.images == 1 { "" } else { "s" }),
        (Some(Pick::Host(_)), Some(Err(message))) => message,
        _ => String::new(),
    };
    let submit_text = move || {
        if let Some(step) = busy.get() {
            return step;
        }
        match (pick.get(), count.get()) {
            (Some(Pick::Browser { files, .. }), _) => format!("Create and upload {}", files.len()),
            (Some(Pick::Host(_)), Some(Ok(Some(found)))) => format!("Create and import {}", found.images),
            (Some(_), _) => "Create and import".to_owned(),
            (None, _) => "Create".to_owned(),
        }
    };
    let shown_folder = move || match pick.get() {
        Some(Pick::Host(path)) => format!("{}/", path.trim_end_matches('/')),
        Some(Pick::Browser { name, .. }) => format!("{name}/"),
        None => String::new(),
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
                        <span class="folder-row">
                            <input class="mono" readonly placeholder="No folder: import later" prop:value=shown_folder />
                            <Show when=move || pick.with(Option::is_some)>
                                <button type="button" on:click=move |_| pick.set(None)>"Clear"</button>
                            </Show>
                            <button type="button" on:click=browse>"Browse…"</button>
                        </span>
                        <span class="folder-count">{count_text}</span>
                        // The browser's own folder dialog, for when the server cannot show one.
                        <input type="file" class="pick-folder" node_ref=browser_input multiple hidden
                            on:change=on_browser_pick />
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

#[derive(serde::Deserialize)]
struct Host {
    local: bool,
}

#[derive(serde::Deserialize)]
struct Picked {
    path: Option<String>,
}

fn is_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// A file's path inside the folder picked in the browser ("folder/sub/name.png").
fn relative_path(file: &web_sys::File) -> String {
    let path = web_sys::js_sys::Reflect::get(file, &"webkitRelativePath".into()).ok().and_then(|path| path.as_string());
    // Empty for files given without their folder.
    path.filter(|path| !path.is_empty()).unwrap_or_else(|| file.name())
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
        line_width: rtag_core::DEFAULT_LINE_WIDTH,
        point_radius: rtag_core::DEFAULT_POINT_RADIUS,
    }
}
