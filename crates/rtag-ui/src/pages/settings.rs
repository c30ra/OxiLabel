//! The admin's Settings screen: where the pre-annotation worker is (the command the server starts
//! it with, and the URL it answers at), applied as soon as it is saved.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use rtag_core::inference::{DEFAULT_WORKER_URL, WorkerHealth, WorkerSettings};

use crate::{
    account::{AccountMenu, is_admin},
    api,
};

#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <header class="topbar">
            <A href="/" attr:class="brand">"rtag"</A>
            <nav class="crumbs" aria-label="Breadcrumb">
                <A href="/">"Projects"</A>
                <span class="sep" aria-hidden="true">"›"</span>
                <span class="current">"Settings"</span>
            </nav>
            <AccountMenu />
        </header>
        <main class="page narrow settings">
            <div class="page-head"><h1>"Settings"</h1></div>
            {if is_admin() {
                view! { <WorkerSettingsForm /> }.into_any()
            } else {
                view! { <p class="muted">"Only admins change the server's settings."</p> }.into_any()
            }}
        </main>
    }
}

/// The worker's health line, asked for again with "Check again".
#[component]
pub fn WorkerHealthLine(health: LocalResource<Result<WorkerHealth, String>>) -> impl IntoView {
    view! {
        <p class="worker-health" role="status">
            {move || match health.get() {
                None => view! { <span class="muted">"Asking the worker…"</span> }.into_any(),
                Some(Err(message)) => view! { <span class="error">{message}</span> }.into_any(),
                Some(Ok(WorkerHealth { reachable: true, device, .. })) => view! {
                    <span class="status-dot done"></span>{format!("Worker ready on {}", device.unwrap_or_else(|| "an unknown device".to_owned()))}
                }.into_any(),
                Some(Ok(WorkerHealth { error, .. })) => view! {
                    <span class="status-dot"></span><span class="error">{format!("Worker not reachable: {}", error.unwrap_or_default())}</span>
                }.into_any(),
            }}
            <button class="small" on:click=move |_| health.refetch()>"Check again"</button>
        </p>
    }
}

#[component]
fn WorkerSettingsForm() -> impl IntoView {
    let health = LocalResource::new(|| async { api::get::<WorkerHealth>("/api/worker").await });
    let command = RwSignal::new(String::new());
    let url = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let saved = RwSignal::new(false);
    spawn_local(async move {
        match api::get::<WorkerSettings>("/api/settings/worker").await {
            Ok(settings) => {
                command.set(settings.command.unwrap_or_default());
                url.set(settings.url.unwrap_or_default());
            }
            Err(message) => error.set(Some(message)),
        }
    });
    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let settings = WorkerSettings { command: Some(command.get_untracked()), url: Some(url.get_untracked()) };
        spawn_local(async move {
            match api::put::<_, WorkerSettings>("/api/settings/worker", &settings).await {
                Ok(settings) => {
                    command.set(settings.command.unwrap_or_default());
                    url.set(settings.url.unwrap_or_default());
                    error.set(None);
                    saved.set(true);
                    // A worker just started takes a moment to answer.
                    set_timeout(move || health.refetch(), std::time::Duration::from_secs(2));
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };
    view! {
        <form class="label-form worker-settings" on:submit=save>
            <h3>"Pre-annotation worker"</h3>
            <WorkerHealthLine health=health />
            <label>
                "Start command"
                <input class="worker-command" placeholder="e.g. PYTHONPATH=/path/to/rtag/worker exec /path/to/venv/bin/python -m rtag_worker"
                    prop:value=command on:input:target=move |ev| { command.set(ev.target().value()); saved.set(false) } />
                <span class="muted hint">"The server runs it through the shell and starts it again if it stops. Leave it empty when the worker runs elsewhere (e.g. a server in a container, the worker on the host)."</span>
            </label>
            <label>
                "URL"
                <input class="worker-url" placeholder=DEFAULT_WORKER_URL
                    prop:value=url on:input:target=move |ev| { url.set(ev.target().value()); saved.set(false) } />
                <span class="muted hint">"Where the worker answers. A started worker listens here."</span>
            </label>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            <div class="row">
                <button class="primary" type="submit">"Save"</button>
                <Show when=move || saved.get()>
                    <span class="muted saved" role="status">"Saved and applied."</span>
                </Show>
            </div>
        </form>
    }
}
