//! A project's jobs: who each is assigned to and how far along it is, and the dialog that puts
//! the grid's selection into new jobs.

use std::collections::BTreeSet;

use leptos::{prelude::*, task::spawn_local};
use leptos_router::hooks::use_navigate;
use oxilabel_core::{
    ImageFilter, ImagePage,
    api::ImagePageQuery,
    job::{Job, JobChange, NewJobs},
    user::User,
};

use super::statistics::progress;
use crate::{account::is_admin, api};

/// "Job 3", or "Job 3 · anna".
pub fn job_name(job: &Job) -> String {
    match &job.assignee {
        Some(user) => format!("Job {} · {}", job.number, user.username),
        None => format!("Job {}", job.number),
    }
}

/// The value of a user menu's option: the user's id, or empty for nobody.
fn assignee_from(value: &str) -> Option<i64> {
    value.parse().ok()
}

#[component]
pub fn JobsTab(project: i64) -> impl IntoView {
    let admin = is_admin();
    let jobs = LocalResource::new(move || async move { api::get::<Vec<Job>>(&format!("/api/projects/{project}/jobs")).await });
    let users = LocalResource::new(move || async move {
        if admin { api::get::<Vec<User>>("/api/users").await.unwrap_or_default() } else { Vec::new() }
    });
    let error = RwSignal::new(None::<String>);
    let navigate = StoredValue::new(use_navigate());

    let assign = move |job: i64, assignee: Option<i64>| {
        spawn_local(async move {
            error.set(api::put::<_, Job>(&format!("/api/projects/{project}/jobs/{job}"), &JobChange { assignee }).await.err());
            jobs.refetch();
        });
    };
    let delete = move |job: Job| {
        let question = format!("Delete {}? Its images stay in the project, in no job.", job_name(&job));
        if !window().confirm_with_message(&question).unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            error.set(api::delete(&format!("/api/projects/{project}/jobs/{}", job.id)).await.err());
            jobs.refetch();
        });
    };
    // Opens the job's first image not yet marked annotated, or its first image if all are.
    let annotate = move |job: i64| {
        spawn_local(async move {
            let url = format!("/api/projects/{project}/images");
            let first = async |annotated: Option<bool>| {
                let filter = ImageFilter { job: Some(job), annotated, ..Default::default() };
                let mut query = filter.to_query();
                query.extend(ImagePageQuery { limit: 1, ..Default::default() }.to_query());
                api::get_with_query::<ImagePage>(&url, &query).await.map(|page| page.images.into_iter().next())
            };
            let image = match first(Some(false)).await {
                Ok(None) => first(None).await,
                found => found,
            };
            match image {
                Ok(Some(image)) => navigate.with_value(|navigate| navigate(&format!("/projects/{project}/images/{}", image.id), Default::default())),
                Ok(None) => {}
                Err(message) => error.set(Some(message)),
            }
        });
    };

    view! {
        <section class="jobs">
            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
            {move || match jobs.get() {
                None => view! { <p class="muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <p class="muted">
                        {if admin {
                            "No jobs yet. Select images on the Images tab and choose “Create jobs…”."
                        } else {
                            "No jobs are assigned to you."
                        }}
                    </p>
                }.into_any(),
                Some(Ok(list)) => view! {
                    <table class="labels jobs-table">
                        <thead><tr><th>"Job"</th><th>"Assigned to"</th><th>"Annotated"</th><th>"Annotations"</th><th></th></tr></thead>
                        <tbody>
                            {list.into_iter().map(|job| {
                                let id = job.id;
                                let assignee = job.assignee.as_ref().map(|user| user.id);
                                let to_delete = job.clone();
                                view! {
                                    <tr data-job=job.number>
                                        <td>{format!("Job {}", job.number)}</td>
                                        <td>
                                            {if admin {
                                                view! {
                                                    <select class="assignee" aria-label=format!("Assignee of job {}", job.number)
                                                        on:change:target=move |ev| assign(id, assignee_from(&ev.target().value()))>
                                                        <option value="" selected=assignee.is_none()>"Nobody"</option>
                                                        {move || users.get().unwrap_or_default().into_iter().map(|user| view! {
                                                            <option value=user.id.to_string() selected=assignee == Some(user.id)>{user.username}</option>
                                                        }).collect_view()}
                                                    </select>
                                                }.into_any()
                                            } else {
                                                view! { <span>{job.assignee.map(|user| user.username).unwrap_or_default()}</span> }.into_any()
                                            }}
                                        </td>
                                        {progress(job.annotated, job.images)}
                                        <td class="mono">{job.annotations}</td>
                                        <td class="actions">
                                            <button class="annotate" disabled=job.images == 0 on:click=move |_| annotate(id)>"Annotate"</button>
                                            {admin.then(|| view! {
                                                <button class="delete" on:click=move |_| delete(to_delete.clone())>"Delete"</button>
                                            })}
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }.into_any(),
            }}
        </section>
    }
}

/// Puts the selected images into one new job, or into jobs of a chosen size, optionally
/// assigned; then shows the Jobs tab.
#[component]
pub fn CreateJobsDialog(
    project: i64,
    selected: RwSignal<BTreeSet<i64>>,
    on_created: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let users = LocalResource::new(|| api::get::<Vec<User>>("/api/users"));
    let split = RwSignal::new(false);
    let per_job = RwSignal::new(50_u32);
    let assignee = RwSignal::new(None::<i64>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let navigate = StoredValue::new(use_navigate());
    let count = move || selected.read().len();
    let jobs = move || if split.get() { count().div_ceil(per_job.get().max(1) as usize) } else { 1 };

    let create = move |_| {
        let new = NewJobs {
            image_ids: selected.get_untracked().into_iter().collect(),
            per_job: split.get_untracked().then(|| per_job.get_untracked()),
            assignee: assignee.get_untracked(),
        };
        busy.set(true);
        spawn_local(async move {
            match api::post::<_, Vec<Job>>(&format!("/api/projects/{project}/jobs"), &new).await {
                Ok(_) => {
                    selected.set(BTreeSet::new());
                    on_created();
                    navigate.with_value(|navigate| navigate(&format!("/projects/{project}/jobs"), Default::default()));
                    on_close();
                }
                Err(message) => {
                    error.set(Some(message));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog jobs-dialog" role="dialog" aria-modal="true" aria-labelledby="jobs-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="jobs-title">"Create jobs"</h2>
                        <p class="muted">{move || match count() {
                            1 => "1 selected image. An image already in a job moves to the new one.".to_owned(),
                            n => format!("{n} selected images. Images already in a job move to the new one."),
                        }}</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <div class="dialog-body">
                    <fieldset class="split">
                        <legend>"Jobs"</legend>
                        <label class="check">
                            <input class="split" type="checkbox" prop:checked=split on:change:target=move |ev| split.set(ev.target().checked()) />
                            "Split into jobs of"
                        </label>
                        <input class="per-job" type="number" min="1" aria-label="Images per job" disabled=move || !split.get()
                            prop:value=move || per_job.get().to_string()
                            on:input:target={move |ev| if let Ok(n) = ev.target().value().parse::<u32>() { per_job.set(n.max(1)) }} />
                        "images"
                        <p class="muted hint">{move || match jobs() {
                            1 => "Makes 1 job.".to_owned(),
                            n => format!("Makes {n} jobs, in path order."),
                        }}</p>
                    </fieldset>
                    <fieldset>
                        <legend>"Assign to"</legend>
                        <select class="assignee" aria-label="Assign to" on:change:target=move |ev| assignee.set(assignee_from(&ev.target().value()))>
                            <option value="">"Nobody yet"</option>
                            {move || users.get().and_then(Result::ok).unwrap_or_default().into_iter().map(|user| view! {
                                <option value=user.id.to_string()>{user.username}</option>
                            }).collect_view()}
                        </select>
                    </fieldset>
                    {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
                </div>
                <div class="dialog-foot">
                    <span class="grow"></span>
                    <button on:click=move |_| on_close()>"Cancel"</button>
                    <button class="primary" disabled=move || busy.get() || count() == 0 on:click=create>
                        {move || match jobs() {
                            1 => "Create job".to_owned(),
                            n => format!("Create {n} jobs"),
                        }}
                    </button>
                </div>
            </div>
        </div>
    }
}
