//! Thin JSON client for the rtag server API. Errors are messages ready to show the user.

use std::cell::RefCell;

use gloo_net::http::{Request, RequestBuilder, Response};
use leptos::prelude::{Callable, Callback};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

thread_local! {
    static ON_SIGNED_OUT: RefCell<Option<Callback<()>>> = const { RefCell::new(None) };
}

/// What to do when the server says the session has ended (expired, or ended by a password
/// reset): set while someone is signed in, to show the sign-in form again.
pub fn on_signed_out(callback: Option<Callback<()>>) {
    ON_SIGNED_OUT.with(|slot| *slot.borrow_mut() = callback);
}

fn signed_out_if_unauthorized(status: u16) {
    if status == 401
        && let Some(callback) = ON_SIGNED_OUT.with(|slot| *slot.borrow())
    {
        callback.run(());
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub async fn get<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    read_json(check(Request::get(&absolute(url)).send().await).await?).await
}

/// GET with query parameters, which are URL-encoded.
pub async fn get_with_query<T: DeserializeOwned>(
    url: &str,
    query: &[(&str, &str)],
) -> Result<T, String> {
    read_json(
        check(
            Request::get(&absolute(url))
                .query(query.iter().copied())
                .send()
                .await,
        )
        .await?,
    )
    .await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, String> {
    send_json(Request::post(&absolute(url)), body).await
}

/// POST of a multipart form (e.g. files to upload).
pub async fn post_form<T: DeserializeOwned>(url: &str, form: web_sys::FormData) -> Result<T, String> {
    let request = Request::post(&absolute(url)).body(form).map_err(|err| err.to_string())?;
    read_json(check(request.send().await).await?).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, String> {
    send_json(Request::put(&absolute(url)), body).await
}

/// A failed request: the HTTP status (0 when the server was unreachable) and a message for the user.
pub struct Failure {
    pub status: u16,
    pub message: String,
}

/// PUT that keeps the status of a failure, for callers that treat some statuses specially.
pub async fn put_detailed<B: Serialize, T: DeserializeOwned>(
    url: &str,
    body: &B,
) -> Result<T, Failure> {
    let request = Request::put(&absolute(url))
        .json(body)
        .map_err(|err| Failure {
            status: 0,
            message: err.to_string(),
        })?;
    let response = request.send().await.map_err(|err| Failure {
        status: 0,
        message: format!("Server unreachable: {err}"),
    })?;
    let status = response.status();
    if !response.ok() {
        signed_out_if_unauthorized(status);
        let message = match response.json::<ErrorBody>().await {
            Ok(body) => body.error,
            Err(_) => format!("Request failed ({status})"),
        };
        return Err(Failure { status, message });
    }
    read_json(response)
        .await
        .map_err(|message| Failure { status, message })
}

/// POST without a body, for actions that answer with none.
pub async fn post_action(url: &str) -> Result<(), String> {
    check(Request::post(&absolute(url)).send().await)
        .await
        .map(|_| ())
}

pub async fn delete(url: &str) -> Result<(), String> {
    check(Request::delete(&absolute(url)).send().await)
        .await
        .map(|_| ())
}

/// Prefixes a server path with the page's origin. Browsers resolve relative request URLs
/// themselves, but some headless engines (Obscura, used for UI tests) do not.
fn absolute(path: &str) -> String {
    let origin = leptos::prelude::window()
        .location()
        .origin()
        .unwrap_or_default();
    format!("{origin}{path}")
}

async fn send_json<B: Serialize, T: DeserializeOwned>(
    builder: RequestBuilder,
    body: &B,
) -> Result<T, String> {
    let request = builder.json(body).map_err(|err| err.to_string())?;
    read_json(check(request.send().await).await?).await
}

async fn read_json<T: DeserializeOwned>(response: Response) -> Result<T, String> {
    response
        .json()
        .await
        .map_err(|err| format!("Unexpected response: {err}"))
}

/// Turns a non-2xx response or a network failure into a message for the user.
async fn check(result: Result<Response, gloo_net::Error>) -> Result<Response, String> {
    let response = result.map_err(|err| format!("Server unreachable: {err}"))?;
    if response.ok() {
        return Ok(response);
    }
    signed_out_if_unauthorized(response.status());
    Err(match response.json::<ErrorBody>().await {
        Ok(body) => body.error,
        Err(_) => format!("Request failed ({})", response.status()),
    })
}

#[derive(Deserialize)]
struct Host {
    local: bool,
}

/// Whether this browser runs on the server's machine, which may use any folder of it (ADR 0003).
pub async fn on_server_machine() -> bool {
    get::<Host>("/api/host").await.is_ok_and(|host| host.local)
}

#[derive(Deserialize)]
struct Picked {
    path: Option<String>,
}

/// Shows the desktop's folder dialog on the server's machine: the chosen folder's absolute path,
/// none if cancelled, or an error if the server cannot show one.
pub async fn host_folder_dialog() -> Result<Option<String>, String> {
    post::<_, Picked>("/api/host/folder-dialog", &()).await.map(|picked| picked.path)
}
