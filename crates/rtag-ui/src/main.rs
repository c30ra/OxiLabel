use gloo_net::http::{Request, Response};
use leptos::{prelude::*, task::spawn_local};
use rtag_core::{NewProject, Project};
use serde::Deserialize;

#[derive(Deserialize)]
struct ApiError {
    error: String,
}

/// Turns a non-2xx response or a network failure into a message for the user.
async fn check(result: Result<Response, gloo_net::Error>) -> Result<Response, String> {
    let response = result.map_err(|err| format!("Server unreachable: {err}"))?;
    if response.ok() {
        return Ok(response);
    }
    Err(match response.json::<ApiError>().await {
        Ok(body) => body.error,
        Err(_) => format!("Request failed ({})", response.status()),
    })
}

async fn fetch_projects() -> Result<Vec<Project>, String> {
    let response = check(Request::get("/api/projects").send().await).await?;
    response.json().await.map_err(|err| err.to_string())
}

async fn create_project(new: NewProject) -> Result<(), String> {
    let request = Request::post("/api/projects").json(&new).map_err(|err| err.to_string())?;
    check(request.send().await).await.map(|_| ())
}

async fn delete_project(id: i64) -> Result<(), String> {
    check(Request::delete(&format!("/api/projects/{id}")).send().await).await.map(|_| ())
}

#[component]
fn App() -> impl IntoView {
    let projects = LocalResource::new(fetch_projects);
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);

    let on_create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewProject { name: name.get().trim().to_owned() };
        if new.name.is_empty() {
            return;
        }
        spawn_local(async move {
            match create_project(new).await {
                Ok(()) => {
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
            error.set(delete_project(project.id).await.err());
            projects.refetch();
        });
    };

    view! {
        <h1>"Projects"</h1>
        <form on:submit=on_create>
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
                <ul>
                    {list.into_iter().map(|project| {
                        let target = project.clone();
                        view! {
                            <li>
                                <span>{project.name}</span>
                                <button on:click=move |_| on_delete(target.clone())>"Delete"</button>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any(),
        }}
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
