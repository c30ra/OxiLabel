//! Who is signed in: the sign-in and first-account screens shown until someone is, and the
//! account menu in the top bar.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use rtag_core::user::{Credentials, MIN_PASSWORD_LENGTH, Role, User};

use crate::api;

/// The signed-in user, provided to every page; `refresh` reloads the session from the server.
#[derive(Clone)]
pub struct Account {
    pub user: User,
    pub refresh: Callback<()>,
}

/// Whether the signed-in user may do admin-only things; pages hide those for annotators.
pub fn is_admin() -> bool {
    use_context::<Account>().is_some_and(|account| account.user.role == Role::Admin)
}

/// The user's name, the admin screen for admins, and logging out.
#[component]
pub fn AccountMenu() -> impl IntoView {
    let account = use_context::<Account>().expect("pages are shown only when signed in");
    let refresh = account.refresh;
    let log_out = move |_| {
        spawn_local(async move {
            // Either way the session is gone or unusable; the page shows what the server says.
            let _ = api::post_action("/api/logout").await;
            refresh.run(());
        });
    };
    view! {
        <div class="account">
            {is_admin().then(|| view! {
                <A href="/models">"Models"</A>
                <A href="/users">"Users"</A>
                <A href="/settings">"Settings"</A>
            })}
            <span class="username" title=format!("Signed in as {}", account.user.username)>{account.user.username.clone()}</span>
            <button class="log-out" on:click=log_out>"Log out"</button>
        </div>
    }
}

/// The sign-in form, or, while the server has no accounts, the form creating the first one,
/// which becomes admin. `on_signed_in` runs once the server has set the session cookie.
#[component]
pub fn SignIn(setup: bool, on_signed_in: Callback<()>) -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let repeated = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if setup && password.get_untracked() != repeated.get_untracked() {
            error.set(Some("The passwords differ.".to_owned()));
            return;
        }
        let credentials = Credentials { username: username.get_untracked(), password: password.get_untracked() };
        busy.set(true);
        spawn_local(async move {
            let url = if setup { "/api/setup" } else { "/api/login" };
            match api::post::<_, User>(url, &credentials).await {
                Ok(_) => on_signed_in.run(()),
                Err(message) => {
                    error.set(Some(message));
                    busy.set(false);
                }
            }
        });
    };
    view! {
        <main class="sign-in">
            <form class="sign-in-card" on:submit=submit aria-label=if setup { "Create the admin account" } else { "Sign in" }>
                <span class="brand">"rtag"</span>
                {if setup {
                    view! {
                        <h1>"Create the admin account"</h1>
                        <p class="muted">"This server has no accounts yet. The first one is its admin, who can add annotators."</p>
                    }.into_any()
                } else {
                    view! { <h1>"Sign in"</h1> }.into_any()
                }}
                <label>
                    "Username"
                    <input class="username" autocomplete="username" required prop:value=username
                        on:input:target=move |ev| username.set(ev.target().value()) />
                </label>
                <label>
                    "Password"
                    <input class="password" type="password" required
                        autocomplete=if setup { "new-password" } else { "current-password" }
                        minlength=setup.then(|| MIN_PASSWORD_LENGTH.to_string())
                        prop:value=password on:input:target=move |ev| password.set(ev.target().value()) />
                </label>
                {setup.then(|| view! {
                    <label>
                        "Repeat password"
                        <input class="repeat" type="password" autocomplete="new-password" required
                            prop:value=repeated on:input:target=move |ev| repeated.set(ev.target().value()) />
                    </label>
                })}
                <Show when=move || error.get().is_some()>
                    <p class="error" role="alert">{move || error.get()}</p>
                </Show>
                <button class="primary" type="submit" disabled=move || busy.get()>
                    {if setup { "Create account" } else { "Sign in" }}
                </button>
            </form>
        </main>
    }
}
