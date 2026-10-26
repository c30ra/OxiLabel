use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use oxilabel_core::{ImagePage, Label, Project};

use std::collections::BTreeSet;

use super::{archive::ArchiveDialog, cvat_import::CvatImportDialog, export::ExportDialog, images::ImageSet, import::ImportDialog, jobs::JobsTab, labels::LabelSet, statistics::StatisticsTab};
use crate::{
    account::{AccountMenu, is_admin},
    api,
    pre_annotate::PreAnnotateDialog,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Images,
    Jobs,
    Labels,
    Statistics,
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
    let admin = is_admin();
    let importing = RwSignal::new(false);
    let importing_annotations = RwSignal::new(false);
    let pre_annotating = RwSignal::new(false);
    let exporting = RwSignal::new(false);
    let archiving = RwSignal::new(false);
    let images_version = RwSignal::new(0_u32);
    // The grid's selection; the export dialog can export just these images.
    let selected = RwSignal::new(BTreeSet::<i64>::new());
    // Whether the project has images yet: without, the Images tab is a get-started checklist.
    let has_images = LocalResource::new(move || {
        let id = project_id.get();
        images_version.track();
        async move {
            let Some(id) = id.filter(|_| admin) else { return true };
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
            let archive_name = project.name.clone();
            let class = move |of: Tab| if of == tab { "tab active" } else { "tab" };
            view! {
                <header class="topbar">
                    <A href="/" attr:class="brand">"OxiLabel"</A>
                    <nav class="crumbs" aria-label="Breadcrumb">
                        <A href="/">"Projects"</A>
                        <span class="sep" aria-hidden="true">"›"</span>
                        <span class="current">{project.name.clone()}</span>
                    </nav>
                    {admin.then(|| view! {
                        <div class="actions">
                            <button on:click=move |_| importing.set(true)>"Import images…"</button>
                            <button class="import-annotations" on:click=move |_| importing_annotations.set(true)>"Import annotations…"</button>
                            <button on:click=move |_| exporting.set(true)>"Export…"</button>
                            <button class="archive" on:click=move |_| archiving.set(true)>"Archive…"</button>
                        </div>
                    })}
                    <button class="pre-annotate" on:click=move |_| pre_annotating.set(true)>"Pre-annotate…"</button>
                    <AccountMenu />
                </header>
                <nav class="tabs" aria-label="Project">
                    {admin.then(|| view! { <A href=format!("/projects/{id}") attr:class=class(Tab::Images)>"Images"</A> })}
                    <A href=format!("/projects/{id}/jobs") attr:class=class(Tab::Jobs)>"Jobs"</A>
                    <A href=format!("/projects/{id}/labels") attr:class=class(Tab::Labels)>"Labels"</A>
                    {admin.then(|| view! { <A href=format!("/projects/{id}/statistics") attr:class=class(Tab::Statistics)>"Statistics"</A> })}
                </nav>
                <main class="page">
                    {match tab {
                        Tab::Images if !admin => view! {
                            <p class="muted">"Only admins browse all images. "<A href=format!("/projects/{id}/jobs")>"Open your jobs"</A></p>
                        }.into_any(),
                        Tab::Images => view! {
                            {move || match has_images.get() {
                                Some(false) => view! { <GetStarted project=id on_import=move || importing.set(true) /> }.into_any(),
                                _ => view! { <ImageSet project=id version=images_version selected=selected /> }.into_any(),
                            }}
                        }.into_any(),
                        Tab::Jobs => view! { <JobsTab project=id /> }.into_any(),
                        Tab::Labels => view! { <LabelSet project=id /> }.into_any(),
                        Tab::Statistics => view! { <StatisticsTab project=id /> }.into_any(),
                    }}
                </main>
                <Show when=move || archiving.get()>
                    <ArchiveDialog project=id project_name=archive_name.clone() on_close=move || archiving.set(false) />
                </Show>
                <Show when=move || exporting.get()>
                    <ExportDialog project=id project_name=export_name.clone() selected=selected on_close=move || exporting.set(false) />
                </Show>
                <Show when=move || pre_annotating.get()>
                    <PreAnnotateDialog project=id selected=selected
                        on_finished=move || images_version.update(|v| *v += 1)
                        on_close=move || pre_annotating.set(false) />
                </Show>
                <Show when=move || importing_annotations.get()>
                    <CvatImportDialog project=id
                        on_imported=move || images_version.update(|v| *v += 1)
                        on_close=move || importing_annotations.set(false) />
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
                    {is_admin().then(|| view! { <button class="primary" on:click=move |_| on_import()>"Import images…"</button> })}
                </li>
                <li>
                    <span class="mark">"3"</span>
                    <span class="text">
                        <strong>"Jobs " <span class="muted">"· optional"</span></strong>
                        <span class="muted">"Split the images among annotators. Working alone? Skip it."</span>
                    </span>
                    <button disabled title="Import images first; then select them in the grid">"Create jobs…"</button>
                </li>
            </ol>
        </section>
    }
}
