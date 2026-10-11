//! Thin JSON client for the rtag server API. Errors are messages ready to show the user.

use gloo_net::http::{Request, RequestBuilder, Response};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub async fn get<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    read_json(check(Request::get(url).send().await).await?).await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, String> {
    send_json(Request::post(url), body).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, String> {
    send_json(Request::put(url), body).await
}

pub async fn delete(url: &str) -> Result<(), String> {
    check(Request::delete(url).send().await).await.map(|_| ())
}

async fn send_json<B: Serialize, T: DeserializeOwned>(builder: RequestBuilder, body: &B) -> Result<T, String> {
    let request = builder.json(body).map_err(|err| err.to_string())?;
    read_json(check(request.send().await).await?).await
}

async fn read_json<T: DeserializeOwned>(response: Response) -> Result<T, String> {
    response.json().await.map_err(|err| format!("Unexpected response: {err}"))
}

/// Turns a non-2xx response or a network failure into a message for the user.
async fn check(result: Result<Response, gloo_net::Error>) -> Result<Response, String> {
    let response = result.map_err(|err| format!("Server unreachable: {err}"))?;
    if response.ok() {
        return Ok(response);
    }
    Err(match response.json::<ErrorBody>().await {
        Ok(body) => body.error,
        Err(_) => format!("Request failed ({})", response.status()),
    })
}
