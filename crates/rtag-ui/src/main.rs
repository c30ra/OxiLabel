mod account;
mod analysis;
mod annotate;
mod api;
mod draw;
mod filmstrip;
mod masks;
mod pages;
mod pie;
mod prefs;
mod pre_annotate;
mod side_panel;
mod tool_options;
mod view_filter;
mod viewer;

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use account::{Account, SignIn};
use pages::{ModelsPage, ProjectPage, ProjectsPage, SettingsPage, Tab, UsersPage, WorkspacePage};
use rtag_core::{settings::UserSettings, user::Session};

/// The pages, once someone is signed in; before that the sign-in (or first account) form.
#[component]
fn App() -> impl IntoView {
    // The session, with the signed-in user's own settings (the defaults if they cannot be read).
    let session = LocalResource::new(|| async {
        let session = api::get::<Session>("/api/session").await?;
        let settings = match session.user {
            Some(_) => api::get::<UserSettings>("/api/me/settings").await.unwrap_or_default(),
            None => UserSettings::default(),
        };
        Ok::<_, String>((session, settings))
    });
    let refresh = Callback::new(move |()| session.refetch());
    view! {
        <Router>
            {move || match session.get() {
                None => view! { <p class="page muted">"Loading…"</p> }.into_any(),
                Some(Err(message)) => view! { <p class="page error" role="alert">{message}</p> }.into_any(),
                Some(Ok((Session { setup, user: None }, _))) => {
                    api::on_signed_out(None);
                    view! { <SignIn setup=setup on_signed_in=refresh /> }.into_any()
                }
                Some(Ok((Session { user: Some(user), .. }, settings))) => {
                    api::on_signed_out(Some(refresh));
                    provide_context(Account { user, refresh });
                    prefs::Prefs::provide(settings);
                    view! {
                        <Routes fallback=|| view! { <p class="page">"Page not found."</p> }>
                            <Route path=path!("/") view=ProjectsPage />
                            <Route path=path!("/users") view=UsersPage />
                            <Route path=path!("/models") view=ModelsPage />
                            <Route path=path!("/settings") view=SettingsPage />
                            <Route path=path!("/projects/:id") view=|| view! { <ProjectPage tab=Tab::Images /> } />
                            <Route path=path!("/projects/:id/jobs") view=|| view! { <ProjectPage tab=Tab::Jobs /> } />
                            <Route path=path!("/projects/:id/labels") view=|| view! { <ProjectPage tab=Tab::Labels /> } />
                            <Route path=path!("/projects/:id/statistics") view=|| view! { <ProjectPage tab=Tab::Statistics /> } />
                            <Route path=path!("/projects/:id/images/:image") view=WorkspacePage />
                        </Routes>
                    }.into_any()
                }
            }}
        </Router>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
