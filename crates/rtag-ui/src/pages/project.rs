use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use rtag_core::{ImagePage, Label, Project};

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
    // Whether the project has images yet: without, the Images tab is a get-started checklist.
    let has_images = LocalResource::new(move || {
        let id = project_id.get();
        images_version.track();
        async move {
            let Some(id) = id else { return true };
            api::get::<ImagePage>(&format!("/api/projects/{id}/images")).await.map_or(true, |page| page.total > 0)
        }
    });

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
                        Tab::Images => view! {
                            {move || match has_images.get() {
                                Some(false) => view! { <GetStarted project=id on_import=move || importing.set(true) /> }.into_any(),
                                _ => view! { <ImageSet project=id version=images_version selected=selected /> }.into_any(),
                            }}
                        }.into_any(),
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

/// The Images tab of a project without images: what to set up, in order.
#[component]
fn GetStarted(project: i64, on_import: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let labels = LocalResource::new(move || async move { api::get::<Vec<Label>>(&format!("/api/projects/{project}/labels")).await });
    let label_step = move || {
        let labels = labels.get().and_then(Result::ok).unwrap_or_default();
        let done = !labels.is_empty();
        let summary = match labels.len() {
            0 => "None yet: add the classes you will annotate.".to_owned(),
            1 => "1 label".to_owned(),
            n => format!("{n} labels"),
        };
        view! {
            <li class:done=done>
                <span class="mark">{if done { "✓" } else { "1" }}</span>
                <span class="text">
                    <strong>"Labels"</strong>
                    <span class="muted swatches">
                        {labels.iter().map(|l| view! { <span class="swatch" style:background=l.color.clone()></span> }).collect_view()}
                        {summary}
                    </span>
                </span>
                <A href=format!("/projects/{project}/labels")>{if done { "Edit" } else { "Add labels" }}</A>
            </li>
        }
    };
    view! {
        <section class="get-started">
            <h1>"Get started"</h1>
            <p class="muted">"The image grid appears here once images are imported."</p>
            <ol>
                {label_step}
                <li class="current">
                    <span class="mark">"2"</span>
                    <span class="text">
                        <strong>"Images"</strong>
                        <span class="muted">"Import from folders of the server's image folder; nothing is copied."</span>
                    </span>
                    <button class="primary" on:click=move |_| on_import()>"Import images…"</button>
                </li>
                <li>
                    <span class="mark">"3"</span>
                    <span class="text">
                        <strong>"Jobs " <span class="muted">"· optional"</span></strong>
                        <span class="muted">"Split the images among annotators. Working alone? Skip it."</span>
                    </span>
                    <button disabled title="Jobs come in a later version">"Create jobs…"</button>
                </li>
            </ol>
        </section>
    }
}
