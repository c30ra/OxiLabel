//! The "Import annotations" dialog: a CVAT for images 1.1 export (XML or zip) is previewed first
//! (which images match, what each CVAT label becomes, what is left out), the choices change the
//! preview, and nothing is written until Import.

use leptos::{prelude::*, task::spawn_local};
use rtag_core::{
    Label,
    import::{CvatImportPlan, CvatImportReport, LabelChoice},
};

use crate::api;

/// A label menu's option value: "create", "skip" or "map:<label id>".
fn choice_value(choice: LabelChoice) -> String {
    match choice {
        LabelChoice::Create => "create".to_owned(),
        LabelChoice::Skip => "skip".to_owned(),
        LabelChoice::Map { label } => format!("map:{label}"),
    }
}

/// "1 image", "3 images".
fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

fn choice_from(value: &str) -> LabelChoice {
    match value.strip_prefix("map:").and_then(|id| id.parse().ok()) {
        Some(label) => LabelChoice::Map { label },
        None if value == "skip" => LabelChoice::Skip,
        None => LabelChoice::Create,
    }
}

#[component]
pub fn CvatImportDialog(
    project: i64,
    on_imported: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let labels = LocalResource::new(move || async move { api::get::<Vec<Label>>(&format!("/api/projects/{project}/labels")).await });
    let file = RwSignal::new_local(None::<web_sys::File>);
    let plan = RwSignal::new(CvatImportPlan::default());
    let report = RwSignal::new(None::<CvatImportReport>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // Numbers the requests: only the answer to the latest counts, so a preview that arrives
    // after a newer one (or after the import) is dropped.
    let latest = StoredValue::new(0_u32);

    // Sends the file with the plan: a preview, or (with `commit`) the import.
    let send = move |commit: bool| {
        let Some(file) = file.get_untracked() else { return };
        let plan = CvatImportPlan { commit, ..plan.get_untracked() };
        latest.update_value(|n| *n += 1);
        let request = latest.get_value();
        busy.set(true);
        spawn_local(async move {
            let result = async {
                let form = web_sys::FormData::new().map_err(|_| "cannot build the upload".to_owned())?;
                form.append_with_blob_and_filename("file", &file, &file.name()).map_err(|_| "cannot read the file".to_owned())?;
                form.append_with_str("plan", &serde_json::to_string(&plan).expect("plans serialise")).map_err(|_| "cannot build the upload".to_owned())?;
                api::post_form::<CvatImportReport>(&format!("/api/projects/{project}/cvat-imports"), form).await
            }
            .await;
            if latest.get_value() != request {
                return;
            }
            busy.set(false);
            match result {
                Ok(result) => {
                    error.set(None);
                    if result.committed {
                        on_imported();
                    }
                    report.set(Some(result));
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };
    let choose = move |name: String, choice: LabelChoice| {
        plan.update(|plan| {
            plan.labels.insert(name, choice);
        });
        send(false);
    };
    let committed = move || report.with(|report| report.as_ref().is_some_and(|report| report.committed));
    // Where missing images are looked for: typed, or (on the server's machine) picked in its dialog.
    let local = LocalResource::new(api::on_server_machine);
    let set_folder = move |folder: String| {
        plan.update(|plan| plan.images_folder = Some(folder.trim().to_owned()).filter(|folder| !folder.is_empty()));
        send(false);
    };
    let browse = move |_| {
        spawn_local(async move {
            match api::host_folder_dialog().await {
                Ok(Some(folder)) => set_folder(folder),
                Ok(None) => {}
                Err(message) => error.set(Some(message)),
            }
        });
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog cvat-dialog" role="dialog" aria-modal="true" aria-labelledby="cvat-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="cvat-title">"Import annotations"</h2>
                        <p class="muted">"A CVAT “for images 1.1” export, as XML or the zip CVAT downloads. Images are matched by file name; nothing is written until you import."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <div class="dialog-body">
                    <input type="file" class="cvat-file" accept=".xml,.zip" aria-label="CVAT export"
                        on:change:target=move |ev| {
                            let picked = ev.target().files().and_then(|files| files.get(0));
                            file.set(picked);
                            plan.set(CvatImportPlan::default());
                            report.set(None);
                            send(false);
                        } />
                    <div class="images-folder">
                        <label>
                            "Images folder"
                            <input class="images-folder-path" type="text"
                                placeholder={move || if local.get().unwrap_or(false) { "Any folder of this computer" } else { "A folder in the server's image folder" }}
                                prop:value=move || plan.read().images_folder.clone().unwrap_or_default()
                                on:change:target=move |ev| set_folder(ev.target().value()) />
                        </label>
                        <Show when=move || local.get().unwrap_or(false)>
                            <button type="button" on:click=browse>"Browse…"</button>
                        </Show>
                        <p class="muted hint">"Optional. Export images the project lacks are looked for here by file name, in every subfolder, and added to the project."</p>
                    </div>
                    {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
                    {move || report.get().map(|report| {
                        let existing = labels.get().and_then(Result::ok).unwrap_or_default();
                        view! { <Preview report=report existing=existing plan=plan choose=choose resend=move || send(false) /> }
                    })}
                </div>
                <div class="dialog-foot">
                    <span class="grow muted">{move || busy.get().then_some("Reading the export…")}</span>
                    <button on:click=move |_| on_close()>{move || if committed() { "Close" } else { "Cancel" }}</button>
                    <Show when=move || !committed()>
                        <button class="primary import"
                            disabled={move || busy.get() || report.with(|r| r.as_ref().is_none_or(|r| r.annotations == 0 || r.labels.iter().any(|label| label.problem.is_some())))}
                            on:click=move |_| send(true)>
                            {move || match report.with(|r| r.as_ref().map(|r| r.annotations)) {
                                Some(n) => format!("Import {}", count(n, "annotation")),
                                None => "Import".to_owned(),
                            }}
                        </button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

/// What the import finds (or did): matches, label choices, existing annotations and skips.
#[component]
fn Preview(
    report: CvatImportReport,
    existing: Vec<Label>,
    plan: RwSignal<CvatImportPlan>,
    choose: impl Fn(String, LabelChoice) + Copy + Send + Sync + 'static,
    resend: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let names = |list: &[String]| list.join(", ");
    let problems = [
        ("with no project image of that file name", report.unmatched.clone()),
        ("whose file name several project images have", report.ambiguous.clone()),
        ("of another size than the project's image (a resized or cropped copy?)", report.size_mismatch.clone()),
    ];
    if report.committed {
        return view! {
            <p class="report cvat-result" role="status">
                {format!("Imported {} into {}", count(report.annotations, "annotation"), count(report.matched, "image"))}
                {match report.added {
                    0 => ".".to_owned(),
                    added => format!(", adding {} to the project.", count(added, "image")),
                }}
            </p>
        }
        .into_any();
    }
    view! {
        <p class="cvat-summary">
            {format!("{} of {} match", report.matched, count(report.images, "image"))}
            {match report.added {
                0 => ".".to_owned(),
                added => format!(", {added} of them found in the images folder and to be added."),
            }}
        </p>
        {problems.into_iter().filter(|(_, list)| !list.is_empty()).map(|(what, list)| view! {
            <details class="cvat-unmatched">
                <summary>{format!("{} {what}", count(list.len(), "image"))}</summary>
                <p class="muted mono">{names(&list)}</p>
            </details>
        }).collect_view()}
        {(report.annotated > 0).then(|| view! {
            <fieldset class="choices cvat-existing">
                <legend>{format!("{} matched images already have annotations", report.annotated)}</legend>
                <label class="choice">
                    <input type="radio" name="existing" prop:checked=move || !plan.read().replace
                        on:change=move |_| { plan.update(|p| p.replace = false); resend() } />
                    <span>"Add the CVAT ones next to them"</span>
                </label>
                <label class="choice">
                    <input type="radio" name="existing" class="replace" prop:checked=move || plan.read().replace
                        on:change=move |_| { plan.update(|p| p.replace = true); resend() } />
                    <span>"Replace them with the CVAT ones"</span>
                </label>
            </fieldset>
        })}
        <table class="labels cvat-labels">
            <thead><tr><th>"CVAT label"</th><th>"Shapes"</th><th>"Becomes"</th></tr></thead>
            <tbody>
                {report.labels.into_iter().map(|label| {
                    let name = label.name.clone();
                    let current = choice_value(label.choice);
                    view! {
                        <tr data-label=label.name.clone()>
                            <td>
                                <span class="swatch" style:background=label.color.clone().unwrap_or_default()></span>
                                {label.name.clone()}
                            </td>
                            <td class="mono">{label.annotations}</td>
                            <td>
                                <select class="choice" aria-label=format!("What “{}” becomes", label.name)
                                    on:change:target=move |ev| choose(name.clone(), choice_from(&ev.target().value()))>
                                    <option value="create" selected=current == "create">"New label"</option>
                                    {existing.iter().map(|target| {
                                        let value = choice_value(LabelChoice::Map { label: target.id });
                                        view! { <option value=value.clone() selected=current == value>{format!("→ {}", target.name)}</option> }
                                    }).collect_view()}
                                    <option value="skip" selected=current == "skip">"Leave out"</option>
                                </select>
                                {label.problem.map(|problem| view! { <span class="error hint">{problem}</span> })}
                            </td>
                        </tr>
                    }
                }).collect_view()}
            </tbody>
        </table>
        {(!report.skipped.is_empty()).then(|| view! {
            <details class="cvat-skipped" open>
                <summary>"Left out"</summary>
                <ul>
                    {report.skipped.into_iter().map(|skip| view! { <li>{format!("{} × {}", skip.count, skip.reason)}</li> }).collect_view()}
                </ul>
            </details>
        })}
    }
    .into_any()
}
