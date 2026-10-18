//! The Settings screen: everyone's own settings (theme, pie sensitivity, how labels are switched
//! while drawing, shortcuts), saved to the server as they change, and for admins where the
//! pre-annotation worker is (the command the server starts it with, and the URL it answers at).

use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use rtag_core::{
    inference::{DEFAULT_WORKER_URL, WorkerHealth, WorkerSettings},
    keymap::{Action, Binding},
    settings::{FLICK_RANGE, LabelStyle, Theme},
};

use crate::{
    account::{AccountMenu, is_admin},
    api,
    prefs::{prefs, press},
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
            <PersonalSettings />
            {is_admin().then(|| view! { <WorkerSettingsForm /> })}
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

/// The signed-in user's own settings; every change applies at once and is saved.
#[component]
fn PersonalSettings() -> impl IntoView {
    let prefs = prefs();
    let error = RwSignal::new(None::<String>);
    let change = move |update: Box<dyn FnOnce(&mut rtag_core::settings::UserSettings)>| {
        error.set(None);
        prefs.change(update, move |message| error.set(Some(message)));
    };
    let theme = move || prefs.settings.with(|s| s.theme);
    let style = move || prefs.settings.with(|s| s.label_style);
    view! {
        <section class="label-form personal-settings">
            <h3>"Your settings"</h3>
            <p class="muted hint">"They follow your account to every computer."</p>
            <fieldset class="choices theme">
                <legend>"Theme"</legend>
                {[(Theme::Dark, "Dark"), (Theme::Light, "Light"), (Theme::System, "As the system")].map(|(value, name)| view! {
                    <label class="choice">
                        <input type="radio" name="theme" value=name prop:checked=move || theme() == value
                            on:change=move |_| change(Box::new(move |s| s.theme = value)) />
                        <span>{name}</span>
                    </label>
                })}
            </fieldset>
            <fieldset class="flick">
                <legend>"Pie menu: flick distance"</legend>
                <input type="range" class="flick-distance" min=FLICK_RANGE.0 max=FLICK_RANGE.1 step="10"
                    prop:value=move || prefs.settings.with(|s| s.flick_distance).to_string()
                    on:change:target={move |ev| if let Ok(value) = ev.target().value().parse::<f64>() { change(Box::new(move |s| s.flick_distance = value)) }} />
                <span class="mono">{move || format!("{:.0} px", prefs.settings.with(|s| s.flick_distance))}</span>
                <span class="muted hint">"How far the pointer goes from the pie's centre to pick by flicking; nearer picks need a click."</span>
            </fieldset>
            <fieldset class="choices label-style">
                <legend>"Switching labels while drawing"</legend>
                <label class="choice">
                    <input type="radio" name="label-style" value="hotkeys" prop:checked=move || style() == LabelStyle::Hotkeys
                        on:change=move |_| change(Box::new(|s| s.label_style = LabelStyle::Hotkeys)) />
                    <span>"Number keys and the label pie key"<small class="muted">{move || format!("{} opens the label pie", prefs.keymap.with(|k| k.label(Action::LabelPie)))}</small></span>
                </label>
                <label class="choice">
                    <input type="radio" name="label-style" value="options_ring" prop:checked=move || style() == LabelStyle::OptionsRing
                        on:change=move |_| change(Box::new(|s| s.label_style = LabelStyle::OptionsRing)) />
                    <span>"Number keys and a Label slice in the tool's options pie"</span>
                </label>
            </fieldset>
            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
            <Shortcuts />
        </section>
    }
}

/// Every action's keys, with a way to set them by pressing them; keys two actions share are
/// flagged.
#[component]
fn Shortcuts() -> impl IntoView {
    let prefs = prefs();
    // The action waiting for a key press, and whether the press replaces its keys or adds one.
    let capturing = RwSignal::new(None::<(Action, bool)>);
    let error = RwSignal::new(None::<String>);
    let set = move |action: Action, bindings: Option<Vec<Binding>>| {
        prefs.change(
            move |s| match bindings {
                Some(bindings) => {
                    s.shortcuts.insert(action.id().to_owned(), bindings);
                }
                None => {
                    s.shortcuts.remove(action.id());
                }
            },
            move |message| error.set(Some(message)),
        );
    };
    let listener = window_event_listener(leptos::ev::keydown, move |ev| {
        let Some((action, add)) = capturing.get_untracked() else { return };
        // A modifier alone is not a key yet.
        if matches!(ev.key().as_str(), "Control" | "Shift" | "Alt" | "Meta" | "AltGraph" | "CapsLock") {
            return;
        }
        ev.prevent_default();
        ev.stop_propagation();
        capturing.set(None);
        let binding = Binding::of(&press(&ev));
        let mut bindings = if add { prefs.keymap.with_untracked(|k| k.bindings(action).to_vec()) } else { Vec::new() };
        if !bindings.contains(&binding) {
            bindings.push(binding);
        }
        set(action, Some(bindings));
    });
    on_cleanup(move || listener.remove());
    let conflicts = Memo::new(move |_| prefs.keymap.with(|k| k.conflicts()));
    let mut groups: Vec<&'static str> = Vec::new();
    for action in Action::ALL {
        if !groups.contains(&action.group()) {
            groups.push(action.group());
        }
    }
    view! {
        <div class="shortcuts">
            <div class="shortcuts-head">
                <h4>"Shortcuts"</h4>
                <Show when=move || !conflicts.with(|c| c.is_empty())>
                    <span class="error conflict-count" role="status">{move || {
                        let n = conflicts.with(|c| c.len());
                        format!("{n} {} used twice", if n == 1 { "key is" } else { "keys are" })
                    }}</span>
                </Show>
                <button type="button" class="small reset-all" on:click=move |_| prefs.change(|s| s.shortcuts.clear(), move |message| error.set(Some(message)))>"Reset all"</button>
            </div>
            {move || error.get().map(|message| view! { <p class="error" role="alert">{message}</p> })}
            {groups.into_iter().map(|group| view! {
                <table class="labels shortcuts-table">
                    <thead><tr><th>{group}</th><th></th><th></th></tr></thead>
                    <tbody>
                        {Action::ALL.iter().copied().filter(|action| action.group() == group).map(|action| {
                            let own = move || prefs.settings.with(|s| s.shortcuts.contains_key(action.id()));
                            view! {
                                <tr data-action=action.id() class:conflict=move || conflicts.with(|c| prefs.keymap.with(|k| k.bindings(action).iter().any(|b| c.contains_key(b))))>
                                    <td>{action.name()}</td>
                                    <td class="keys"><span class="key-list">
                                        {move || if capturing.get().is_some_and(|(a, _)| a == action) {
                                            // Any key is taken, Escape too: backing out is a button.
                                            view! {
                                                <span class="capturing" role="status">"Press the keys…"</span>
                                                <button type="button" class="small cancel-capture" on:click=move |_| capturing.set(None)>"Cancel"</button>
                                            }.into_any()
                                        } else {
                                            prefs.keymap.with(|k| k.bindings(action).iter().map(|binding| {
                                                let others: Vec<&str> = conflicts.with(|c| c.get(binding).map(|actions| actions.iter().filter(|a| **a != action).map(|a| a.name()).collect()).unwrap_or_default());
                                                let title = if others.is_empty() { String::new() } else { format!("Also: {}", others.join(", ")) };
                                                view! { <kbd class:clash=!others.is_empty() title=title>{binding.label()}</kbd> }
                                            }).collect_view()).into_any()
                                        }}
                                    </span></td>
                                    <td class="actions">
                                        <button type="button" class="small change" on:click=move |_| capturing.set(Some((action, false)))>"Change"</button>
                                        <button type="button" class="small add" title="Another key for it" on:click=move |_| capturing.set(Some((action, true)))>"+"</button>
                                        <Show when=own>
                                            <button type="button" class="small reset" title="Back to the default" on:click=move |_| set(action, None)>"Default"</button>
                                        </Show>
                                    </td>
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            }).collect_view()}
        </div>
    }
}
