mod annotate;
mod api;
mod draw;
mod pages;
mod pie;
mod tool_options;
mod viewer;

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use pages::{ProjectPage, ProjectsPage, Tab, WorkspacePage};

#[component]
fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| view! { <p class="page">"Page not found."</p> }>
                <Route path=path!("/") view=ProjectsPage />
                <Route path=path!("/projects/:id") view=|| view! { <ProjectPage tab=Tab::Images /> } />
                <Route path=path!("/projects/:id/labels") view=|| view! { <ProjectPage tab=Tab::Labels /> } />
                <Route path=path!("/projects/:id/images/:image") view=WorkspacePage />
            </Routes>
        </Router>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
