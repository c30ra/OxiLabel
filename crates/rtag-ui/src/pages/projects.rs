use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use rtag_core::{NewProject, Project};

use crate::api;

#[component]
pub fn ProjectsPage() -> impl IntoView {
    let projects = LocalResource::new(|| api::get::<Vec<Project>>("/api/projects"));
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    let on_create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewProject { name: name.get().trim().to_owned() };
        if new.name.is_empty() {
            return;
        }
        spawn_local(async move {
            match api::post::<_, Project>("/api/projects", &new).await {
                Ok(_) => {
                    name.set(String::new());
                    error.set(None);
                }
                Err(message) => error.set(Some(message)),
            }
            projects.refetch();
        });
    };

    let on_delete = move |project: Project| {
        let confirmed = window()
            .confirm_with_message(&format!("Delete project \"{}\"?", project.name))
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            error.set(api::delete(&format!("/api/projects/{}", project.id)).await.err());
            projects.refetch();
        });
    };

    view! {
        <h1>"Projects"</h1>
        <form class="row" on:submit=on_create>
            <input placeholder="New project name" prop:value=name on:input:target=move |ev| name.set(ev.target().value()) />
            <button type="submit">"Create"</button>
        </form>
        <Show when=move || error.get().is_some()>
            <p class="error" role="alert">{move || error.get()}</p>
        </Show>
        {move || match projects.get() {
            None => view! { <p>"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(list)) if list.is_empty() => view! { <p>"No projects yet."</p> }.into_any(),
            Some(Ok(list)) => view! {
                <ul class="list">
                    {list.into_iter().map(|project| {
                        let target = project.clone();
                        view! {
                            <li>
                                <A href=format!("/projects/{}", project.id)>{project.name}</A>
                                <button on:click=move |_| on_delete(target.clone())>"Delete"</button>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any(),
        }}
    }
}
