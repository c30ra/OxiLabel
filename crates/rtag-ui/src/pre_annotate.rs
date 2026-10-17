//! Running a model on images: the model menu and Run button, then the run's progress with Cancel,
//! then what it made. Used by the workspace (the image) and the project's Pre-annotate dialog
//! (the selection or a job).

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use leptos::{prelude::*, task::spawn_local};
use rtag_core::{
    inference::{Model, RunRequest, RunState, RunStatus},
    job::Job,
};

use crate::api;

/// How often a running run's progress is asked for.
const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// What a run covers: images, or a job's images.
#[derive(Debug, Clone, PartialEq)]
pub enum RunScope {
    Images(Vec<i64>),
    Job(i64),
}

/// "4 pre-annotations; 2 left out: …".
fn summary(status: &RunStatus) -> String {
    let made = format!("{} pre-annotation{}", status.created, if status.created == 1 { "" } else { "s" });
    let done = match status.state {
        RunState::Finished => format!("Done: {made}."),
        RunState::Cancelled => format!("Cancelled after {} of {} images: {made}.", status.done, status.total),
        RunState::Failed => format!("Failed after {} of {} images ({made}): {}", status.done, status.total, status.error.clone().unwrap_or_default()),
        RunState::Running => format!("{} of {} images", status.done, status.total),
    };
    let skipped: Vec<String> = status.skipped.iter().map(|skip| format!("{} × {}", skip.count, skip.reason)).collect();
    if skipped.is_empty() { done } else { format!("{done} Left out: {}.", skipped.join(", ")) }
}

/// Runs a model of the user's choice on `scope` (when there is one and `ready`), and calls
/// `on_finished` with the run's last status once it has ended, whatever the outcome.
#[component]
pub fn RunPanel(project: i64, scope: Signal<Option<RunScope>>, ready: Signal<bool>, on_finished: Callback<RunStatus>) -> impl IntoView {
    let models = LocalResource::new(|| api::get::<Vec<Model>>("/api/models"));
    let model = RwSignal::new(None::<i64>);
    let status = RwSignal::new(None::<RunStatus>);
    let error = RwSignal::new(None::<String>);
    let running = move || status.with(|s| s.as_ref().is_some_and(|s| s.state == RunState::Running));
    // Polling stops when the panel goes away (another image, a closed dialog).
    let alive = Arc::new(AtomicBool::new(true));
    on_cleanup({
        let alive = alive.clone();
        move || alive.store(false, Ordering::SeqCst)
    });
    let alive = StoredValue::new(alive);

    // Asks for the run's progress until it ends.
    fn poll(id: u64, alive: Arc<AtomicBool>, status: RwSignal<Option<RunStatus>>, error: RwSignal<Option<String>>, on_finished: Callback<RunStatus>) {
        spawn_local(async move {
            let latest = api::get::<RunStatus>(&format!("/api/runs/{id}")).await;
            if !alive.load(Ordering::SeqCst) {
                return;
            }
            match latest {
                Ok(latest) if latest.state == RunState::Running => {
                    status.set(Some(latest));
                    set_timeout(move || poll(id, alive, status, error, on_finished), POLL_INTERVAL);
                }
                Ok(latest) => {
                    status.set(Some(latest.clone()));
                    on_finished.run(latest);
                }
                // E.g. the server restarted and forgot the run.
                Err(message) => {
                    status.set(None);
                    error.set(Some(format!("Lost track of the run: {message}")));
                }
            }
        });
    }
    let start = move |_| {
        // Read now: once the panel is gone the stored flag is too.
        let alive = alive.get_value();
        let (Some(model), Some(scope)) = (model.get_untracked().or_else(|| models.get().and_then(Result::ok)?.first().map(|m| m.id)), scope.get_untracked()) else { return };
        let request = match scope {
            RunScope::Images(image_ids) => RunRequest { model, image_ids, job: None },
            RunScope::Job(job) => RunRequest { model, image_ids: Vec::new(), job: Some(job) },
        };
        error.set(None);
        spawn_local(async move {
            match api::post::<_, RunStatus>(&format!("/api/projects/{project}/pre-annotations"), &request).await {
                Ok(started) => {
                    let id = started.id;
                    status.set(Some(started));
                    poll(id, alive, status, error, on_finished);
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };
    let cancel = move |_| {
        let Some(id) = status.with_untracked(|s| s.as_ref().map(|s| s.id)) else { return };
        spawn_local(async move {
            if let Err(message) = api::delete(&format!("/api/runs/{id}")).await {
                error.set(Some(message));
            }
        });
    };

    view! {
        <div class="run-panel">
            {move || match models.get() {
                None => view! { <p class="muted">"Loading models…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! { <p class="muted">"No models yet: an admin registers them on the Models screen."</p> }.into_any(),
                Some(Ok(list)) => view! {
                    <div class="run-controls">
                        <select class="run-model" aria-label="Model" disabled=running
                            on:change:target=move |ev| model.set(ev.target().value().parse().ok())>
                            {list.into_iter().map(|m| view! { <option value=m.id.to_string() selected=move || model.get() == Some(m.id)>{m.definition.name}</option> }).collect_view()}
                        </select>
                        <Show when=running fallback=move || view! {
                            <button class="primary run" disabled=move || !ready.get() || scope.with(Option::is_none)
                                title=move || if ready.get() { "" } else { "Waiting for your changes to be saved" }
                                on:click=start>"Run"</button>
                        }>
                            <button class="cancel-run" on:click=cancel>"Cancel"</button>
                        </Show>
                    </div>
                }.into_any(),
            }}
            {move || status.get().map(|s| {
                let percent = (s.done * 100).checked_div(s.total).unwrap_or(0);
                view! {
                    <div class="run-status" role="status">
                        {(s.state == RunState::Running).then(|| view! { <span class="bar"><span style:width=format!("{percent}%")></span></span> })}
                        <span class="run-summary">{summary(&s)}</span>
                    </div>
                }
            })}
            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
        </div>
    }
}

/// The project's "Pre-annotate" dialog: a model run on the grid's selection or on a job.
#[component]
pub fn PreAnnotateDialog(
    project: i64,
    selected: RwSignal<BTreeSet<i64>>,
    on_finished: impl Fn() + Copy + Send + Sync + 'static,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let jobs = LocalResource::new(move || async move { api::get::<Vec<Job>>(&format!("/api/projects/{project}/jobs")).await });
    // The selection when there is one, else the first job.
    let use_job = RwSignal::new(selected.with_untracked(BTreeSet::is_empty));
    let job = RwSignal::new(None::<i64>);
    let scope = Signal::derive(move || {
        if use_job.get() {
            job.get().or_else(|| jobs.get().and_then(Result::ok)?.first().map(|j| j.id)).map(RunScope::Job)
        } else {
            Some(RunScope::Images(selected.get().into_iter().collect())).filter(|scope| scope != &RunScope::Images(Vec::new()))
        }
    });
    view! {
        <div class="backdrop" on:click=move |_| on_close()>
            <div class="dialog run-dialog" role="dialog" aria-modal="true" aria-labelledby="run-title"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| if ev.key() == "Escape" { on_close() }>
                <div class="dialog-head">
                    <div>
                        <h2 id="run-title">"Pre-annotate"</h2>
                        <p class="muted">"A model's findings become pre-annotations to review; running it again replaces those not reviewed yet."</p>
                    </div>
                    <button class="icon" aria-label="Close" on:click=move |_| on_close()>"✕"</button>
                </div>
                <div class="dialog-body">
                    <fieldset class="choices">
                        <legend>"Images"</legend>
                        <label class="choice">
                            <input type="radio" name="scope" class="scope-selection" prop:checked=move || !use_job.get()
                                disabled=move || selected.with(BTreeSet::is_empty) on:change=move |_| use_job.set(false) />
                            <span>{move || format!("Selected images ({})", selected.with(BTreeSet::len))}</span>
                        </label>
                        <label class="choice">
                            <input type="radio" name="scope" class="scope-job" prop:checked=move || use_job.get() on:change=move |_| use_job.set(true) />
                            <span>"A job"</span>
                            <select class="run-job" aria-label="Job" on:change:target=move |ev| { job.set(ev.target().value().parse().ok()); use_job.set(true) }>
                                {move || jobs.get().and_then(Result::ok).unwrap_or_default().into_iter().map(|j| {
                                    view! { <option value=j.id.to_string() selected=move || job.get() == Some(j.id)>{crate::pages::job_name(&j)}</option> }
                                }).collect_view()}
                            </select>
                        </label>
                    </fieldset>
                    <RunPanel project=project scope=scope ready=Signal::derive(|| true) on_finished=Callback::new(move |_| on_finished()) />
                </div>
                <div class="dialog-foot">
                    <span class="grow"></span>
                    <button on:click=move |_| on_close()>"Close"</button>
                </div>
            </div>
        </div>
    }
}
