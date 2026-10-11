#![allow(dead_code)]

use std::ops::Deref;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

/// A server on an in-memory database with its own temporary image root and cache folder.
pub struct TestApp {
    router: Router,
    pub images: TempDir,
    _cache: TempDir,
    pub exports: TempDir,
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
    let router = rtag_server::app(rtag_server::Config {
        database_url: "sqlite::memory:".into(),
        images_root: images.path().to_owned(),
        cache_dir: cache.path().to_owned(),
        export_dir: exports.path().to_owned(),
    })
    .await
    .unwrap();
    TestApp { router, images, _cache: cache, exports }
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
