use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use rtag_core::Project;

use super::labels::LabelSet;
use crate::api;

#[component]
pub fn ProjectPage() -> impl IntoView {
    let params = use_params_map();
    let project_id = Memo::new(move |_| params.read().get("id").and_then(|id| id.parse::<i64>().ok()));
    let project = LocalResource::new(move || {
        let id = project_id.get();
        async move {
            match id {
                Some(id) => api::get::<Project>(&format!("/api/projects/{id}")).await,
                None => Err("Invalid project link".to_owned()),
            }
        }
    });

    view! {
        <nav><A href="/">"← Projects"</A></nav>
        {move || match project.get() {
            None => view! { <p>"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(project)) => view! {
                <h1>{project.name}</h1>
                <LabelSet project=project.id />
            }.into_any(),
        }}
    }
}
