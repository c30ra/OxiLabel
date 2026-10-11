use gloo_net::http::Request;
use leptos::{prelude::*, task::spawn_local};
use rtag_core::{NewProject, Project};

async fn fetch_projects() -> Vec<Project> {
    match Request::get("/api/projects").send().await {
        Ok(res) => res.json().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[component]
fn App() -> impl IntoView {
    let projects = LocalResource::new(fetch_projects);
    let name = RwSignal::new(String::new());

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewProject { name: name.get().trim().to_owned() };
        if new.name.is_empty() {
            return;
        }
        spawn_local(async move {
            if let Ok(req) = Request::post("/api/projects").json(&new) {
                let _ = req.send().await;
            }
            name.set(String::new());
            projects.refetch();
        });
    };

    let remove = move |project: Project| {
        let confirmed = window()
            .confirm_with_message(&format!("Delete project \"{}\"?", project.name))
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            let _ = Request::delete(&format!("/api/projects/{}", project.id)).send().await;
            projects.refetch();
        });
    };

    view! {
        <h1>"Projects"</h1>
        <form on:submit=create>
            <input placeholder="New project name" prop:value=name on:input:target=move |ev| name.set(ev.target().value()) />
            <button type="submit">"Create"</button>
        </form>
        <ul>
            <For
                each=move || projects.get().unwrap_or_default()
                key=|p| p.id
                children=move |p| {
                    let target = p.clone();
                    view! {
                        <li>
                            <span>{p.name}</span>
                            <button on:click=move |_| remove(target.clone())>"Delete"</button>
                        </li>
                    }
                }
            />
        </ul>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
