use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use rtag_core::Project;

use std::collections::BTreeSet;

use super::{export::ExportDialog, images::ImageSet, import::ImportDialog, labels::LabelSet};
use crate::api;

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Images,
    Labels,
}

#[component]
pub fn ProjectPage(tab: Tab) -> impl IntoView {
    let params = use_params_map();
    let project_id = Memo::new(move |_| {
        params
            .read()
            .get("id")
            .and_then(|id| id.parse::<i64>().ok())
    });
    let project = LocalResource::new(move || {
        let id = project_id.get();
        async move {
            match id {
                Some(id) => api::get::<Project>(&format!("/api/projects/{id}")).await,
                None => Err("Invalid project link".to_owned()),
            }
        }
    });
    let importing = RwSignal::new(false);
    let exporting = RwSignal::new(false);
    let images_version = RwSignal::new(0_u32);
    // The grid's selection; the export dialog can export just these images.
    let selected = RwSignal::new(BTreeSet::<i64>::new());

    move || match project.get() {
        None => view! { <p class="page muted">"Loading…"</p> }.into_any(),
        Some(Err(message)) => view! { <p class="page error" role="alert">{message}</p> }.into_any(),
        Some(Ok(project)) => {
            let id = project.id;
            let name = project.name.clone();
            let export_name = project.name.clone();
            view! {
                <header class="topbar">
                    <A href="/" attr:class="brand">"rtag"</A>
                    <nav class="crumbs" aria-label="Breadcrumb">
                        <A href="/">"Projects"</A>
                        <span class="sep" aria-hidden="true">"›"</span>
                        <span class="current">{project.name.clone()}</span>
                    </nav>
                    <div class="actions">
                        <button on:click=move |_| importing.set(true)>"Import images…"</button>
                        <button on:click=move |_| exporting.set(true)>"Export…"</button>
                    </div>
                </header>
                <nav class="tabs" aria-label="Project">
                    <A href=format!("/projects/{id}") attr:class=if tab == Tab::Images { "tab active" } else { "tab" }>"Images"</A>
                    <A href=format!("/projects/{id}/labels") attr:class=if tab == Tab::Labels { "tab active" } else { "tab" }>"Labels"</A>
                </nav>
                <main class="page">
                    {match tab {
                        Tab::Images => view! { <ImageSet project=id version=images_version selected=selected /> }.into_any(),
                        Tab::Labels => view! { <LabelSet project=id /> }.into_any(),
                    }}
                </main>
                <Show when=move || exporting.get()>
                    <ExportDialog project=id project_name=export_name.clone() selected=selected on_close=move || exporting.set(false) />
                </Show>
                <Show when=move || importing.get()>
                    <ImportDialog
                        project=id
                        project_name=name.clone()
                        on_imported=move || images_version.update(|v| *v += 1)
                        on_close=move || importing.set(false)
                    />
                </Show>
            }
            .into_any()
        }
    }
}
