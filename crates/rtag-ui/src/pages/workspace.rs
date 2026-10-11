use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use rtag_core::{Image, Project, geometry::Point, view::View};

use crate::{api, viewer::TileViewer};

/// The annotation workspace for one image. For now it only views the image.
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
            let project = api::get::<Project>(&format!("/api/projects/{project}")).await?;
            let image = api::get::<Image>(&format!("/api/projects/{}/images/{image}", project.id)).await?;
            Ok::<_, String>((project, image))
        }
    });

    move || match data.get() {
        None => view! { <p class="page muted">"Loading…"</p> }.into_any(),
        Some(Err(message)) => view! { <p class="page error" role="alert">{message}</p> }.into_any(),
        Some(Ok((project, image))) => view! { <Workspace project=project image=image /> }.into_any(),
    }
}

#[component]
fn Workspace(project: Project, image: Image) -> impl IntoView {
    let view = RwSignal::new(None::<View>);
    let size = RwSignal::new((0.0, 0.0));
    let name = image.path.rsplit('/').next().unwrap_or_default().to_owned();
    let zoom_label = move || view.get().map(|v| format!("{:.0}%", v.scale * 100.0)).unwrap_or_default();
    // Toggles between 100% around the centre and fitting the whole image.
    let toggle_zoom = move |_| {
        let (width, height) = size.get_untracked();
        match view.get_untracked() {
            Some(v) if (v.scale - 1.0).abs() > 1e-6 => view.set(Some(v.zoom_to(Point::new(width / 2.0, height / 2.0), 1.0))),
            _ => view.set(None),
        }
    };

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
                    <span class="muted mono">{format!("{} × {}", image.width, image.height)}</span>
                    <button class="zoom mono" title="Zoom: click for 100% / fit (Home fits, + and − zoom)"
                        on:click=toggle_zoom>{zoom_label}</button>
                </div>
            </header>
            <TileViewer image=image view=view size=size />
        </div>
    }
}
