//! The admin screen's models: the worker's health, the registered models, and a form to add or
//! change one (adapter, weights, options, which label each class becomes).

use std::collections::BTreeMap;

use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use oxilabel_core::inference::{Model, ModelDefinition, WorkerHealth};
use serde_json::{Map, Value};

use super::settings::WorkerHealthLine;
use crate::{
    account::{AccountMenu, is_admin},
    api,
};

/// The class map as the form shows it: one "class = label" line each.
fn map_text(map: &BTreeMap<String, String>) -> String {
    map.iter().map(|(class, label)| format!("{class} = {label}\n")).collect()
}

fn map_from(text: &str) -> Result<BTreeMap<String, String>, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('=') {
            Some((class, label)) if !class.trim().is_empty() && !label.trim().is_empty() => Ok((class.trim().to_owned(), label.trim().to_owned())),
            _ => Err(format!("“{line}” is not “class = label”")),
        })
        .collect()
}

fn options_from(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    serde_json::from_str(text).map_err(|err| format!("the options are not a JSON object: {err}"))
}

#[component]
pub fn ModelsPage() -> impl IntoView {
    view! {
        <header class="topbar">
            <A href="/" attr:class="brand">"OxiLabel"</A>
            <nav class="crumbs" aria-label="Breadcrumb">
                <A href="/">"Projects"</A>
                <span class="sep" aria-hidden="true">"›"</span>
                <span class="current">"Models"</span>
            </nav>
            <AccountMenu />
        </header>
        <main class="page narrow models">
            <div class="page-head"><h1>"Models"</h1></div>
            {if is_admin() {
                view! { <ModelAdmin /> }.into_any()
            } else {
                view! { <p class="muted">"Only admins manage models."</p> }.into_any()
            }}
        </main>
    }
}

#[component]
fn ModelAdmin() -> impl IntoView {
    let health = LocalResource::new(|| async { api::get::<WorkerHealth>("/api/worker").await });
    let models = LocalResource::new(|| api::get::<Vec<Model>>("/api/models"));
    let editing = RwSignal::new(None::<i64>);
    let name = RwSignal::new(String::new());
    let adapter = RwSignal::new("ultralytics".to_owned());
    let weights = RwSignal::new(String::new());
    let options = RwSignal::new(String::new());
    let class_map = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    let reset = move || {
        editing.set(None);
        name.set(String::new());
        adapter.set("ultralytics".to_owned());
        weights.set(String::new());
        options.set(String::new());
        class_map.set(String::new());
        error.set(None);
    };
    let start_edit = move |model: Model| {
        editing.set(Some(model.id));
        name.set(model.definition.name);
        adapter.set(model.definition.adapter);
        weights.set(model.definition.weights);
        options.set(if model.definition.options.is_empty() { String::new() } else { Value::Object(model.definition.options).to_string() });
        class_map.set(map_text(&model.definition.class_map));
        error.set(None);
    };
    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let definition = match (options_from(&options.get_untracked()), map_from(&class_map.get_untracked())) {
            (Ok(options), Ok(class_map)) => ModelDefinition { name: name.get_untracked(), adapter: adapter.get_untracked(), weights: weights.get_untracked(), options, class_map },
            (Err(message), _) | (_, Err(message)) => return error.set(Some(message)),
        };
        spawn_local(async move {
            let saved = match editing.get_untracked() {
                Some(id) => api::put::<_, Model>(&format!("/api/models/{id}"), &definition).await,
                None => api::post::<_, Model>("/api/models", &definition).await,
            };
            match saved {
                Ok(_) => {
                    reset();
                    models.refetch();
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };
    let delete = move |model: Model| {
        let question = format!("Forget the model “{}”? Its pre-annotations stay.", model.definition.name);
        if !window().confirm_with_message(&question).unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            error.set(api::delete(&format!("/api/models/{}", model.id)).await.err());
            models.refetch();
        });
    };
    // Adds a "class = class" line for each class of the model not mapped yet.
    let read_classes = move |_| {
        let Some(id) = editing.get_untracked() else { return };
        spawn_local(async move {
            match api::get::<Vec<String>>(&format!("/api/models/{id}/classes")).await {
                Ok(classes) => {
                    let mut map = map_from(&class_map.get_untracked()).unwrap_or_default();
                    for class in classes {
                        map.entry(class.clone()).or_insert(class);
                    }
                    class_map.set(map_text(&map));
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };

    view! {
        <WorkerHealthLine health=health />
        <p class="muted hint">"Where the worker is is set on the Settings screen."</p>
        {move || match models.get() {
            None => view! { <p class="muted">"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(list)) if list.is_empty() => view! { <p class="muted">"No models yet."</p> }.into_any(),
            Some(Ok(list)) => view! {
                <table class="labels models-table">
                    <thead><tr><th>"Name"</th><th>"Adapter"</th><th>"Weights"</th><th></th></tr></thead>
                    <tbody>
                        {list.into_iter().map(|model| {
                            let (to_edit, to_delete) = (model.clone(), model.clone());
                            view! {
                                <tr data-model=model.definition.name.clone()>
                                    <td>{model.definition.name.clone()}</td>
                                    <td class="mono">{model.definition.adapter.clone()}</td>
                                    <td class="mono weights" title=model.definition.weights.clone()>{model.definition.weights.clone()}</td>
                                    <td class="actions">
                                        <button on:click=move |_| start_edit(to_edit.clone())>"Edit"</button>
                                        <button on:click=move |_| delete(to_delete.clone())>"Delete"</button>
                                    </td>
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            }.into_any(),
        }}
        <form class="label-form model-form" on:submit=save>
            <h3>{move || if editing.get().is_some() { "Edit model" } else { "Add model" }}</h3>
            <label>"Name" <input class="model-name" required prop:value=name on:input:target=move |ev| name.set(ev.target().value()) /></label>
            <label>
                "Adapter"
                <input class="model-adapter" required placeholder="ultralytics, or the path of an adapter .py file on the worker's machine"
                    prop:value=adapter on:input:target=move |ev| adapter.set(ev.target().value()) />
            </label>
            <label>
                "Weights"
                <input class="model-weights" required placeholder="Path of the weights file on the worker's machine"
                    prop:value=weights on:input:target=move |ev| weights.set(ev.target().value()) />
            </label>
            <label>
                "Options"
                <textarea class="model-options" rows="2" placeholder="JSON, e.g. {\"conf\": 0.25, \"imgsz\": 1024}"
                    prop:value=options on:input:target=move |ev| options.set(ev.target().value())></textarea>
            </label>
            <label>
                "Classes"
                <textarea class="model-classes" rows="4" placeholder="class = label, one per line; a class not listed becomes the label of the same name"
                    prop:value=class_map on:input:target=move |ev| class_map.set(ev.target().value())></textarea>
            </label>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            <div class="row">
                <button class="primary" type="submit">{move || if editing.get().is_some() { "Save" } else { "Add model" }}</button>
                <Show when=move || editing.get().is_some()>
                    <button type="button" class="read-classes" title="Ask the worker for the model's classes" on:click=read_classes>"Read classes"</button>
                    <button type="button" on:click=move |_| reset()>"Cancel"</button>
                </Show>
            </div>
        </form>
    }
}
