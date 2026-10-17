#![allow(dead_code)]

use std::ops::Deref;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

/// A server on an in-memory database with its own temporary image root and cache folder.
/// Requests through it (or `remote`) come from another machine; through `local` from the
/// server's own. Its host folder dialog "picks" `picked`.
pub struct TestApp {
    router: Router,
    pub local: Router,
    pub images: TempDir,
    pub cache: TempDir,
    pub exports: TempDir,
    pub uploads: TempDir,
    pub picked: TempDir,
}

impl Deref for TestApp {
    type Target = Router;
    fn deref(&self) -> &Router {
        &self.router
    }
}

pub async fn test_app() -> TestApp {
    let images = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let exports = TempDir::new().unwrap();
    let uploads = TempDir::new().unwrap();
    let picked = TempDir::new().unwrap();
    let choice = picked.path().to_owned();
    let folder_dialog: rtag_server::FolderDialog = Arc::new(move || {
        let choice = choice.clone();
        Box::pin(async move { Ok(Some(choice)) })
    });
    let router = rtag_server::app(rtag_server::Config {
        database_url: "sqlite::memory:".into(),
        images_root: images.path().to_owned(),
        cache_dir: cache.path().to_owned(),
        export_dir: exports.path().to_owned(),
        upload_dir: uploads.path().to_owned(),
        folder_dialog: Some(folder_dialog),
    })
    .await
    .unwrap();
    let local = router.clone().layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    let router = router.layer(MockConnectInfo(SocketAddr::from(([192, 0, 2, 7], 50000))));
    TestApp { router, local, images, cache, exports, uploads, picked }
}

pub async fn call(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, json)
}

/// Sends a GET and returns the status, content type and raw body.
pub async fn get_bytes(app: &Router, uri: &str) -> (StatusCode, String, Vec<u8>) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let content_type = res
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, content_type, bytes)
}

/// A multipart/form-data POST with one file part per (name, bytes), the name being the file's
/// path relative to the picked folder.
pub async fn upload(app: &Router, uri: &str, files: &[(&str, Vec<u8>)]) -> (StatusCode, Value) {
    let boundary = "rtag-test-boundary";
    let mut body = Vec::new();
    for (name, bytes) in files {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
