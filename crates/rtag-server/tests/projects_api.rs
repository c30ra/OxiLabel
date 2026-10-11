mod common;

use axum::http::{Method, StatusCode};
use common::{call, test_app};
use serde_json::json;

#[tokio::test]
async fn created_project_appears_in_the_list() {
    let app = test_app().await;

    let (status, created) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Cracks"}))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "Cracks");

    let (status, list) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([{"id": created["id"], "name": "Cracks"}]));
}

#[tokio::test]
async fn deleted_project_disappears_from_the_list() {
    let app = test_app().await;
    let (_, keep) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Keep"}))).await;
    let (_, gone) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Gone"}))).await;

    let (status, _) = call(&app, Method::DELETE, &format!("/api/projects/{}", gone["id"]), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(list, json!([{"id": keep["id"], "name": "Keep"}]));
}

#[tokio::test]
async fn deleting_an_unknown_project_is_not_found() {
    let app = test_app().await;
    let (status, _) = call(&app, Method::DELETE, "/api/projects/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn project_name_must_not_be_blank() {
    let app = test_app().await;
    let (status, body) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "   "}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "project name must not be blank");

    let (_, list) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn project_name_is_trimmed() {
    let app = test_app().await;
    let (_, created) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "  Cracks "}))).await;
    assert_eq!(created["name"], "Cracks");
}

#[tokio::test]
async fn malformed_project_body_is_rejected() {
    let app = test_app().await;
    let (status, _) = call(&app, Method::POST, "/api/projects", Some(json!({"title": "x"}))).await;
    assert!(status.is_client_error(), "got {status}");
}

#[tokio::test]
async fn a_single_project_can_be_fetched() {
    let app = test_app().await;
    let (_, created) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Cracks"}))).await;

    let (status, project) = call(&app, Method::GET, &format!("/api/projects/{}", created["id"]), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(project, created);

    let (status, _) = call(&app, Method::GET, "/api/projects/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
