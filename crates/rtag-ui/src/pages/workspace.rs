use std::time::Duration;

use leptos::{ev, prelude::*, task::spawn_local};
use leptos_router::{components::A, hooks::use_params_map};
use rtag_core::{
    Image, Label, Project,
    annotation::{Annotation, ImageAnnotations, SaveAnnotations},
    geometry::Point,
    view::View,
};

use crate::{
    annotate::{AnnotationLayer, Editor, Tool},
    api,
    side_panel::SidePanel,
    tool_options::ToolOptions,
    viewer::TileViewer,
};

/// Unsaved changes are saved this long after the last edit.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(1000);
const NOTICE_DURATION: Duration = Duration::from_secs(4);

#[derive(Clone)]
struct Loaded {
    project: Project,
    image: Image,
    labels: Vec<Label>,
    annotations: ImageAnnotations,
}

/// The annotation workspace for one image.
#[component]
pub fn WorkspacePage() -> impl IntoView {
    let params = use_params_map();
    let ids = Memo::new(move |_| {
        let params = params.read();
        let parse = |key| params.get(key).and_then(|id| id.parse::<i64>().ok());
        parse("id").zip(parse("image"))
    });
    let data = LocalResource::new(move || {
        let ids = ids.get();
        async move {
            let (project, image) = ids.ok_or_else(|| "Invalid image link".to_owned())?;
            let base = format!("/api/projects/{project}");
            Ok::<_, String>(Loaded {
                project: api::get::<Project>(&base).await?,
                image: api::get::<Image>(&format!("{base}/images/{image}")).await?,
                labels: api::get::<Vec<Label>>(&format!("{base}/labels")).await?,
                annotations: api::get::<ImageAnnotations>(&format!(
                    "{base}/images/{image}/annotations"
                ))
                .await?,
            })
        }
    });

    move || match data.get() {
        None => view! { <p class="page muted">"Loading…"</p> }.into_any(),
        Some(Err(message)) => view! { <p class="page error" role="alert">{message}</p> }.into_any(),
        Some(Ok(loaded)) => {
            let Loaded {
                project,
                image,
                labels,
                annotations,
            } = loaded;
            view! { <Workspace project=project image=image labels=labels annotations=annotations /> }.into_any()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SaveState {
    Saved,
    Unsaved,
    Saving,
    Failed(String),
    /// The image changed elsewhere; saving stops until the user reloads.
    Conflict(String),
}

#[component]
fn Workspace(
    project: Project,
    image: Image,
    labels: Vec<Label>,
    annotations: ImageAnnotations,
) -> impl IntoView {
    let view = RwSignal::new(None::<View>);
    let size = RwSignal::new((0.0, 0.0));
    let space_held = RwSignal::new(false);
    // Start with the first label; switching to a tool picks one that allows its shape.
    let editor = Editor::new(conformed(&labels, annotations.annotations), labels.iter().map(|label| label.id).min());
    let labels_for_reload = StoredValue::new(labels.clone());
    let picker_labels = labels.clone();
    let layer_labels = labels.clone();
    let url = StoredValue::new(format!(
        "/api/projects/{}/images/{}/annotations",
        project.id, image.id
    ));

    // Autosave: the version the editor is based on and the last revision the server has.
    let version = RwSignal::new(annotations.version);
    let saved_revision = RwSignal::new(0_u64);
    let state = RwSignal::new(SaveState::Saved);
    let retry = RwSignal::new(0_u32);
    let timer = StoredValue::new(0_u64);

    let save = move || {
        if matches!(
            state.get_untracked(),
            SaveState::Saving | SaveState::Conflict(_)
        ) {
            return;
        }
        let revision = editor.revision.get_untracked();
        if revision == saved_revision.get_untracked() {
            state.set(SaveState::Saved);
            return;
        }
        let request = SaveAnnotations {
            base_version: version.get_untracked(),
            annotations: editor.annotations.get_untracked(),
        };
        state.set(SaveState::Saving);
        spawn_local(async move {
            match api::put_detailed::<_, ImageAnnotations>(&url.get_value(), &request).await {
                Ok(saved) => {
                    version.set(saved.version);
                    saved_revision.set(revision);
                    state.set(SaveState::Saved);
                    // Edits made while saving are saved next.
                    if editor.revision.get_untracked() != revision {
                        retry.update(|n| *n += 1);
                    }
                }
                Err(failure) if failure.status == 409 => {
                    state.set(SaveState::Conflict(failure.message))
                }
                Err(failure) => state.set(SaveState::Failed(failure.message)),
            }
        });
    };
    Effect::new(move |_| {
        let revision = editor.revision.get();
        retry.track();
        if revision == saved_revision.get_untracked()
            || matches!(state.get_untracked(), SaveState::Conflict(_))
        {
            return;
        }
        if state.get_untracked() != SaveState::Saving {
            state.set(SaveState::Unsaved);
        }
        let token = timer.get_value() + 1;
        timer.set_value(token);
        set_timeout(
            move || {
                if timer.get_value() == token {
                    save();
                }
            },
            AUTOSAVE_DELAY,
        );
    });
    // Leaving the image saves at once; closing the tab with unsaved work asks first.
    {
        let url = url.get_value();
        on_cleanup(move || {
            if editor.revision.get_untracked() != saved_revision.get_untracked()
                && !matches!(state.get_untracked(), SaveState::Conflict(_))
            {
                let request = SaveAnnotations {
                    base_version: version.get_untracked(),
                    annotations: editor.annotations.get_untracked(),
                };
                spawn_local(async move {
                    let _ = api::put_detailed::<_, ImageAnnotations>(&url, &request).await;
                });
            }
        });
    }
    let before_unload = window_event_listener(ev::beforeunload, move |ev| {
        if editor.revision.get_untracked() != saved_revision.get_untracked() {
            ev.prevent_default();
            ev.set_return_value("");
        }
    });
    on_cleanup(move || before_unload.remove());

    let reload = move |_| {
        spawn_local(async move {
            match api::get::<ImageAnnotations>(&url.get_value()).await {
                Ok(latest) => {
                    editor.reset(conformed(&labels_for_reload.get_value(), latest.annotations));
                    version.set(latest.version);
                    saved_revision.set(editor.revision.get_untracked());
                    state.set(SaveState::Saved);
                }
                Err(message) => editor.notice.set(Some(message)),
            }
        });
    };

    // Hints fade out on their own.
    Effect::new(move |_| {
        if editor.notice.get().is_some() {
            set_timeout(move || editor.notice.set(None), NOTICE_DURATION);
        }
    });

    let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
    let zoom_label = move || {
        view.get()
            .map(|v| format!("{:.0}%", v.scale * 100.0))
            .unwrap_or_default()
    };
    // Toggles between 100% around the centre and fitting the whole image.
    let toggle_zoom = move |_| {
        let (width, height) = size.get_untracked();
        match view.get_untracked() {
            Some(v) if (v.scale - 1.0).abs() > 1e-6 => {
                view.set(Some(v.zoom_to(Point::new(width / 2.0, height / 2.0), 1.0)))
            }
            _ => view.set(None),
        }
    };
    let (image_id, image_size) = (image.id, (image.width, image.height));

    view! {
        <div class="workspace">
            <header class="topbar">
                <A href="/" attr:class="brand">"rtag"</A>
                <nav class="crumbs" aria-label="Breadcrumb">
                    <A href="/">"Projects"</A>
                    <span class="sep" aria-hidden="true">"›"</span>
                    <A href=format!("/projects/{}", project.id)>{project.name.clone()}</A>
                    <span class="sep" aria-hidden="true">"›"</span>
                    <span class="current mono" title=image.path.clone()>{name}</span>
                </nav>
                <div class="actions">
                    <SaveIndicator state=state on_retry=save on_reload=reload />
                    <LabelPicker labels=picker_labels editor=editor />
                    <button class="zoom mono" title="Zoom: click for 100% / fit (Home fits, + and − zoom)"
                        on:click=toggle_zoom>{zoom_label}</button>
                </div>
            </header>
            <ToolOptions editor=editor />
            <div class="workspace-body">
                <ToolRail editor=editor />
                <TileViewer image=image view=view size=size space_held=space_held>
                    <AnnotationLayer editor=editor labels=layer_labels image_id=image_id image_size=image_size view=view size=size space_held=space_held />
                    {move || editor.notice.get().map(|text| view! { <p class="notice" role="status">{text}</p> })}
                </TileViewer>
                <SidePanel editor=editor labels=labels />
            </div>
        </div>
    }
}

#[component]
fn SaveIndicator(
    state: RwSignal<SaveState>,
    on_retry: impl Fn() + Copy + Send + Sync + 'static,
    on_reload: impl Fn(ev::MouseEvent) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    move || {
        match state.get() {
        SaveState::Saved => view! { <span class="save-state saved" role="status">"Saved"</span> }.into_any(),
        SaveState::Unsaved => view! { <span class="save-state unsaved" role="status">"Unsaved changes"</span> }.into_any(),
        SaveState::Saving => view! { <span class="save-state" role="status">"Saving…"</span> }.into_any(),
        SaveState::Failed(message) => view! {
            <span class="save-state failed" role="alert" title=message.clone()>"Save failed"</span>
            <button class="small" on:click=move |_| on_retry()>"Retry"</button>
        }
        .into_any(),
        SaveState::Conflict(message) => view! {
            <span class="save-state failed" role="alert" title=message>"Changed elsewhere"</span>
            <button class="small" title="Load the latest annotations; your unsaved edits are discarded" on:click=on_reload>"Reload"</button>
        }
        .into_any(),
    }
    }
}

/// The label new shapes get; number keys (the labels' hotkeys) switch it too.
#[component]
fn LabelPicker(labels: Vec<Label>, editor: Editor) -> impl IntoView {
    let open = RwSignal::new(false);
    let current = {
        let labels = labels.clone();
        move || {
            let id = editor.label.get();
            labels.iter().find(|label| Some(label.id) == id).cloned()
        }
    };
    let close = window_event_listener(ev::pointerdown, move |ev| {
        let inside = ev
            .target()
            .and_then(|target| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(target).ok())
            .and_then(|element| element.closest(".label-picker").ok().flatten())
            .is_some();
        if !inside {
            open.set(false);
        }
    });
    on_cleanup(move || close.remove());
    view! {
        <div class="label-picker">
            <button class="label-current" aria-haspopup="listbox" aria-expanded=move || open.get().to_string()
                title="Label for new shapes (number keys switch)" on:click=move |_| open.update(|o| *o = !*o)>
                {move || match current() {
                    Some(label) => view! {
                        <span class="swatch" style:background=label.color.clone()></span>
                        <span>{label.name.clone()}</span>
                        {label.hotkey.clone().map(|key| view! { <kbd>{key}</kbd> })}
                    }.into_any(),
                    None => view! { <span class="muted">"No label"</span> }.into_any(),
                }}
                <span class="chevron" aria-hidden="true">"▾"</span>
            </button>
            <Show when=move || open.get()>
                <ul class="label-menu" role="listbox">
                    {labels.iter().map(|label| {
                        let id = label.id;
                        let shapes = StoredValue::new(label.shape_types.clone());
                        // Labels that don't allow the active tool's shape cannot be picked.
                        let usable = move || {
                            editor.tool.get().shape_type().is_none_or(|shape| shapes.with_value(|allowed| allowed.contains(&shape)))
                        };
                        view! {
                            <li role="option" aria-selected=move || (editor.label.get() == Some(id)).to_string()
                                aria-disabled=move || (!usable()).to_string()
                                class:active=move || editor.label.get() == Some(id)
                                class:disabled=move || !usable()
                                on:click=move |_| if usable() { editor.label.set(Some(id)); open.set(false); }>
                                <span class="swatch" style:background=label.color.clone()></span>
                                <span class="grow">{label.name.clone()}</span>
                                {label.hotkey.clone().map(|key| view! { <kbd>{key}</kbd> })}
                            </li>
                        }
                    }).collect_view()}
                </ul>
            </Show>
        </div>
    }
}

#[component]
fn ToolRail(editor: Editor) -> impl IntoView {
    let tool_button = move |tool: Tool,
                            name: &'static str,
                            key: &'static str,
                            icon: &'static str| {
        view! {
            <button class="tool" class:active=move || editor.tool.get() == tool aria-pressed=move || (editor.tool.get() == tool).to_string()
                title=format!("{name} ({key})") aria-label=name on:click=move |_| editor.tool.set(tool)>
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d=icon /></svg>
                <kbd>{key}</kbd>
            </button>
        }
    };
    view! {
        <nav class="tool-rail" aria-label="Tools">
            {tool_button(Tool::Select, "Select", "V", "M5 3l13 8-6 1.5L9.5 19z")}
            {tool_button(Tool::Rectangle, "Rectangle", "R", "M4 6h16v12H4z")}
            {tool_button(Tool::Polygon, "Polygon", "P", "M12 3l8 6-3 11H7L4 9z")}
            {tool_button(Tool::Polyline, "Polyline", "L", "M3 18l5-9 5 6 8-11")}
            {tool_button(Tool::Circle, "Circle", "C", "M12 4a8 8 0 1 0 .01 0")}
            {tool_button(Tool::Brush, "Brush", "B", "M4 20c3 0 5-1.5 5-4a3 3 0 0 0-3-3c-2 0-3 1.5-3 3.5 0 1.5-.5 2.5-1 3.5z M9 13l10-10 2 2-10 10")}
            {tool_button(Tool::Points, "Points", ".", "M4.4 7a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0 M15.4 6a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0 M9.9 12.5a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0 M5.4 18a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0 M16.4 17a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0")}
        </nav>
    }
}

/// Annotations with their attribute values made to fit their labels' current definitions, which
/// may have changed since they were saved.
fn conformed(labels: &[Label], annotations: Vec<Annotation>) -> Vec<Annotation> {
    annotations
        .into_iter()
        .map(|annotation| match labels.iter().find(|label| label.id == annotation.label) {
            Some(label) => Annotation { attributes: label.conform_attributes(&annotation.attributes), ..annotation },
            None => annotation,
        })
        .collect()
}
