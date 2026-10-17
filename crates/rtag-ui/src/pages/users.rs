//! The admin screen's accounts: every user with their role, a password reset per user, and a
//! form adding one.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use rtag_core::user::{MIN_PASSWORD_LENGTH, NewUser, Role, User, UserChange};

use crate::{
    account::{Account, AccountMenu, is_admin},
    api,
};

/// The role a role menu's option stands for.
fn role_from(value: &str) -> Role {
    Role::parse(value).unwrap_or(Role::Annotator)
}

#[component]
pub fn UsersPage() -> impl IntoView {
    view! {
        <header class="topbar">
            <A href="/" attr:class="brand">"rtag"</A>
            <nav class="crumbs" aria-label="Breadcrumb">
                <A href="/">"Projects"</A>
                <span class="sep" aria-hidden="true">"›"</span>
                <span class="current">"Users"</span>
            </nav>
            <AccountMenu />
        </header>
        <main class="page narrow users">
            <div class="page-head"><h1>"Users"</h1></div>
            {if is_admin() {
                view! { <UserAdmin /> }.into_any()
            } else {
                view! { <p class="muted">"Only admins manage users."</p> }.into_any()
            }}
        </main>
    }
}

#[component]
fn UserAdmin() -> impl IntoView {
    let users = LocalResource::new(|| api::get::<Vec<User>>("/api/users"));
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);
    let account = use_context::<Account>().expect("pages are shown only when signed in");
    let (me, refresh_account) = (account.user.id, account.refresh);
    let change = move |id: i64, change: UserChange| {
        let reset = change.password.is_some();
        // An admin who steps down loses this screen and the admin buttons.
        let own_role = id == me && change.role.is_some();
        spawn_local(async move {
            match api::put::<_, User>(&format!("/api/users/{id}"), &change).await {
                Ok(user) => {
                    error.set(None);
                    notice.set(reset.then(|| format!("{}'s password was reset.", user.username)));
                    if own_role {
                        refresh_account.run(());
                    }
                }
                Err(message) => {
                    error.set(Some(message));
                    notice.set(None);
                }
            }
            users.refetch();
        });
    };

    view! {
        <Show when=move || error.get().is_some()>
            <p class="error" role="alert">{move || error.get()}</p>
        </Show>
        <Show when=move || notice.get().is_some()>
            <p class="muted notice-line" role="status">{move || notice.get()}</p>
        </Show>
        {move || match users.get() {
            None => view! { <p class="muted">"Loading…"</p> }.into_any(),
            Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
            Some(Ok(list)) => view! {
                <table class="labels users">
                    <thead><tr><th>"Username"</th><th>"Role"</th><th></th></tr></thead>
                    <tbody>
                        {list.into_iter().map(|user| view! { <UserRow user=user on_change=change /> }).collect_view()}
                    </tbody>
                </table>
            }.into_any(),
        }}
        <NewUserForm on_created=move || users.refetch() />
    }
}

#[component]
fn UserRow(user: User, on_change: impl Fn(i64, UserChange) + Copy + Send + Sync + 'static) -> impl IntoView {
    let id = user.id;
    let resetting = RwSignal::new(false);
    let password = RwSignal::new(String::new());
    let reset = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        on_change(id, UserChange { password: Some(password.get_untracked()), ..UserChange::default() });
        resetting.set(false);
    };
    view! {
        <tr data-user=user.username.clone()>
            <td>{user.username.clone()}</td>
            <td>
                <select class="role" aria-label=format!("Role of {}", user.username)
                    on:change:target=move |ev| on_change(id, UserChange { role: Some(role_from(&ev.target().value())), ..UserChange::default() })>
                    {Role::ALL.map(|role| view! { <option value=role.as_str() selected=role == user.role>{role.name()}</option> })}
                </select>
            </td>
            <td class="actions">
                <Show when=move || resetting.get() fallback=move || view! {
                    <button class="reset" on:click=move |_| { password.set(String::new()); resetting.set(true); }>"Reset password…"</button>
                }>
                    <form class="reset-form" on:submit=reset>
                        <input class="new-password" type="password" placeholder="New password" autocomplete="new-password" required
                            minlength=MIN_PASSWORD_LENGTH.to_string() prop:value=password on:input:target=move |ev| password.set(ev.target().value()) />
                        <button class="primary" type="submit">"Set"</button>
                        <button type="button" on:click=move |_| resetting.set(false)>"Cancel"</button>
                    </form>
                </Show>
            </td>
        </tr>
    }
}

#[component]
fn NewUserForm(on_created: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let role = RwSignal::new(Role::Annotator);
    let error = RwSignal::new(None::<String>);
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let new = NewUser { username: username.get_untracked(), password: password.get_untracked(), role: role.get_untracked() };
        spawn_local(async move {
            match api::post::<_, User>("/api/users", &new).await {
                Ok(_) => {
                    error.set(None);
                    username.set(String::new());
                    password.set(String::new());
                    on_created();
                }
                Err(message) => error.set(Some(message)),
            }
        });
    };
    view! {
        <form class="label-form new-user" on:submit=submit>
            <h3>"Add user"</h3>
            <div class="row">
                <input class="username" placeholder="Username" autocomplete="off" required prop:value=username
                    on:input:target=move |ev| username.set(ev.target().value()) />
                <input class="password" type="password" placeholder="Password" autocomplete="new-password" required
                    minlength=MIN_PASSWORD_LENGTH.to_string() prop:value=password on:input:target=move |ev| password.set(ev.target().value()) />
                <select class="role" aria-label="Role" on:change:target=move |ev| role.set(role_from(&ev.target().value()))>
                    {Role::ALL.map(|value| view! { <option value=value.as_str() selected=move || role.get() == value>{value.name()}</option> })}
                </select>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error" role="alert">{move || error.get()}</p>
            </Show>
            <div class="row"><button class="primary" type="submit">"Add user"</button></div>
        </form>
    }
}
