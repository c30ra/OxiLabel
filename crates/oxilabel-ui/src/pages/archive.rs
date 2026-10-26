//! Project archives in the UI: downloading a project's archive (with or without its images), and
//! importing an archive as a new project.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::hooks::use_navigate;
use oxilabel_core::{
    api::{ArchiveExportQuery, with_query},
    import::{ArchiveImportPlan, ArchiveImportReport},
};

use crate::api;

/// "Archive…" on the project page: the archive downloads straight from the server.
#[component]
pub fn ArchiveDialog(project: i64, project_name: String, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let images = RwSignal::new(false);
    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog archive-dialog" role="dialog" aria-modal="true" aria-labelledby="archive-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="archive-title">{format!("Archive “{project_name}”")}</h2>
                        <p class="muted">"The whole project in one file (labels, jobs, annotations, pre-annotations), to import into another OxiLabel without loss."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <div class="dialog-body">
                    <fieldset class="choices">
                        <legend>"Images"</legend>
                        <label class="choice">
                            <input type="radio" name="archive-images" prop:checked=move || !images.get() on:change=move |_| images.set(false) />
                            <span>"Refer to them"<small class="muted">"small; the other OxiLabel finds them by path or in a folder you choose"</small></span>
                        </label>
                        <label class="choice">
                            <input type="radio" name="archive-images" class="with-images" prop:checked=images on:change=move |_| images.set(true) />
                            <span>"Include them"<small class="muted">"a self-contained file, as large as the images"</small></span>
                        </label>
                    </fieldset>
                </div>
                <div class="dialog-foot">
                    <span class="grow"></span>
                    <button on:click=move |_| on_close()>"Cancel"</button>
                    <a class="button primary download-archive" download
                        href=move || with_query(&format!("/api/projects/{project}/archive"), &ArchiveExportQuery { images: images.get() }.to_query())>"Download"</a>
                </div>
            </div>
        </div>
    }
}

/// "Import archive…" on the projects page: makes a new project from an archive, then opens it.
#[component]
pub fn ImportArchiveDialog(on_imported: impl Fn() + Copy + Send + Sync + 'static, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let file = RwSignal::new_local(None::<web_sys::File>);
    let name = RwSignal::new(String::new());
    let folder = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let report = RwSignal::new(None::<ArchiveImportReport>);
    let local = LocalResource::new(api::on_server_machine);
    let navigate = StoredValue::new(use_navigate());

    let browse = move |_| {
        spawn_local(async move {
            match api::host_folder_dialog().await {
                Ok(Some(picked)) => folder.set(picked),
                Ok(None) => {}
                Err(message) => error.set(Some(message)),
            }
        });
    };
    let import = move |_| {
        let Some(file) = file.get_untracked() else { return };
        let given = |text: String| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        let plan = ArchiveImportPlan { name: given(name.get_untracked()), images_folder: given(folder.get_untracked()) };
        busy.set(true);
        spawn_local(async move {
            let result = async {
                let form = web_sys::FormData::new().map_err(|_| "cannot build the upload".to_owned())?;
                form.append_with_str("plan", &serde_json::to_string(&plan).expect("plans serialise")).map_err(|_| "cannot build the upload".to_owned())?;
                form.append_with_blob_and_filename("file", &file, &file.name()).map_err(|_| "cannot read the file".to_owned())?;
                api::post_form::<ArchiveImportReport>("/api/project-archives", form).await
            }
            .await;
            busy.set(false);
            match result {
                // Everything found: straight into the new project.
                Ok(done) if done.missing.is_empty() && done.skipped.is_empty() && done.unassigned_jobs.is_empty() => {
                    navigate.with_value(|navigate| navigate(&format!("/projects/{}", done.project.id), Default::default()));
                }
                Ok(done) => {
                    on_imported();
                    report.set(Some(done));
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog import-archive-dialog" role="dialog" aria-modal="true" aria-labelledby="import-archive-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="import-archive-title">"Import a project archive"</h2>
                        <p class="muted">"Makes a new project from an OxiLabel project archive, with its labels, jobs and annotations."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                {move || match report.get() {
                    Some(done) => {
                        let id = done.project.id;
                        view! {
                            <div class="dialog-body archive-report" role="status">
                                <p>{format!("Made “{}” with {} images and {} annotations.", done.project.name, done.images, done.annotations)}</p>
                                {(!done.missing.is_empty()).then(|| view! {
                                    <details class="archive-missing" open>
                                        <summary>{format!("{} images not found, left out with their annotations", done.missing.len())}</summary>
                                        <p class="muted mono">{done.missing.join(", ")}</p>
                                    </details>
                                })}
                                {(!done.unassigned_jobs.is_empty()).then(|| view! {
                                    <p class="muted">{format!("Unassigned (no such user here): job {}.", done.unassigned_jobs.iter().map(i64::to_string).collect::<Vec<_>>().join(", "))}</p>
                                })}
                                {done.skipped.into_iter().map(|skip| view! { <p class="muted">{format!("{} × {}", skip.count, skip.reason)}</p> }).collect_view()}
                            </div>
                            <div class="dialog-foot">
                                <span class="grow"></span>
                                <button class="primary open-project"
                                    on:click=move |_| navigate.with_value(|navigate| navigate(&format!("/projects/{id}"), Default::default()))>"Open the project"</button>
                            </div>
                        }.into_any()
                    }
                    None => view! {
                        <div class="dialog-body archive-form">
                            <input type="file" class="archive-file" accept=".zip" aria-label="Project archive"
                                on:change:target=move |ev| file.set(ev.target().files().and_then(|files| files.get(0))) />
                            <label>
                                "Name"
                                <input class="archive-name" placeholder="The archive's" prop:value=name on:input:target=move |ev| name.set(ev.target().value()) />
                            </label>
                            <div class="images-folder">
                                <label>
                                    "Images folder"
                                    <input class="archive-folder" type="text"
                                        placeholder={move || if local.get().unwrap_or(false) { "Any folder of this computer" } else { "A folder in the server's image folder" }}
                                        prop:value=folder on:input:target=move |ev| folder.set(ev.target().value()) />
                                </label>
                                <Show when=move || local.get().unwrap_or(false)>
                                    <button type="button" on:click=browse>"Browse…"</button>
                                </Show>
                                <p class="muted hint">"For an archive without images: those not at their old path are looked for here by file name, in every subfolder, and checked by content."</p>
                            </div>
                            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
                        </div>
                        <div class="dialog-foot">
                            <span class="grow muted">{move || busy.get().then_some("Importing…")}</span>
                            <button on:click=move |_| on_close()>"Cancel"</button>
                            <button class="primary import-archive" disabled=move || busy.get() || file.with(Option::is_none) on:click=import>"Import"</button>
                        </div>
                    }.into_any(),
                }}
            </div>
        </div>
    }
}
