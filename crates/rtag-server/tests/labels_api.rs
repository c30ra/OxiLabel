mod common;

use axum::{
    Router,
    http::{Method, StatusCode},
};
use common::{call, test_app};
use serde_json::{Value, json};

async fn new_project(app: &Router) -> i64 {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    project["id"].as_i64().unwrap()
}

async fn add_label(app: &Router, project: i64, label: Value) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/labels"), Some(label)).await
}

#[tokio::test]
async fn created_label_appears_in_the_project_label_set() {
    let app = test_app().await;
    let project = new_project(&app).await;

    let (status, label) = add_label(
        &app,
        project,
        json!({"name": "crack", "color": "#e74c3c", "hotkey": "1", "shape_types": ["polyline", "polygon"]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let expected = json!({
        "id": 1,
        "name": "crack",
        "color": "#e74c3c",
        "hotkey": "1",
        "shape_types": ["polyline", "polygon"],
        "ignore": false,
        "attributes": [], "line_width": 7.0, "point_radius": 8.0
    });
    assert_eq!(label, expected);
    let (status, labels) = call(&app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(labels, json!([expected]));
}

fn ids(labels: &Value) -> Vec<i64> {
    labels.as_array().unwrap().iter().map(|l| l["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn deleted_label_ids_are_never_reused() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for name in ["crack", "chip", "shunt"] {
        add_label(&app, project, json!({"name": name, "color": "#ffffff"})).await;
    }

    let (status, _) = call(&app, Method::DELETE, &format!("/api/projects/{project}/labels/2"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, added) = add_label(&app, project, json!({"name": "finger", "color": "#ffffff"})).await;

    assert_eq!(added["id"], 4);
    let (_, labels) = call(&app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    assert_eq!(ids(&labels), vec![1, 3, 4]);
}

#[tokio::test]
async fn editing_a_label_keeps_its_id() {
    let app = test_app().await;
    let project = new_project(&app).await;
    add_label(&app, project, json!({"name": "crack", "color": "#ffffff"})).await;

    let (status, edited) = call(
        &app,
        Method::PUT,
        &format!("/api/projects/{project}/labels/1"),
        Some(json!({"name": "micro_crack", "color": "#00ff00", "hotkey": "c", "shape_types": ["polyline"], "ignore": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let expected = json!({
        "id": 1, "name": "micro_crack", "color": "#00ff00", "hotkey": "c", "shape_types": ["polyline"], "ignore": false,
        "attributes": [], "line_width": 7.0, "point_radius": 8.0
    });
    assert_eq!(edited, expected);
    let (_, labels) = call(&app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    assert_eq!(labels, json!([expected]));
}

#[tokio::test]
async fn omitted_shape_types_allow_every_shape() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (_, label) = add_label(&app, project, json!({"name": "crack", "color": "#ffffff"})).await;
    assert_eq!(
        label["shape_types"],
        json!(["rectangle", "oriented_rectangle", "polygon", "polyline", "points", "circle", "mask", "tag"])
    );
}

#[tokio::test]
async fn each_project_numbers_its_labels_from_one() {
    let app = test_app().await;
    let first = new_project(&app).await;
    let second = new_project(&app).await;
    add_label(&app, first, json!({"name": "crack", "color": "#ffffff"})).await;

    let (_, label) = add_label(&app, second, json!({"name": "crack", "color": "#ffffff"})).await;
    assert_eq!(label["id"], 1);
}

#[tokio::test]
async fn labels_of_an_unknown_project_are_not_found() {
    let app = test_app().await;
    let (status, _) = call(&app, Method::GET, "/api/projects/999/labels", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = add_label(&app, 999, json!({"name": "crack", "color": "#ffffff"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_project_removes_its_labels() {
    let app = test_app().await;
    let project = new_project(&app).await;
    add_label(&app, project, json!({"name": "crack", "color": "#ffffff"})).await;

    call(&app, Method::DELETE, &format!("/api/projects/{project}"), None).await;
    let (status, _) = call(&app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_label_definitions_are_rejected_with_a_reason() {
    let app = test_app().await;
    let project = new_project(&app).await;
    add_label(&app, project, json!({"name": "crack", "color": "#ffffff", "hotkey": "1"})).await;

    let cases = [
        (json!({"name": "  ", "color": "#ffffff"}), "label name must not be blank"),
        (json!({"name": "crack", "color": "#ffffff"}), "a label named \"crack\" already exists"),
        (json!({"name": "chip", "color": "red"}), "color must look like #rrggbb"),
        (json!({"name": "chip", "color": "#ffffff", "hotkey": "1"}), "hotkey \"1\" is already used by \"crack\""),
        (json!({"name": "chip", "color": "#ffffff", "hotkey": "ab"}), "hotkey must be a single key"),
        (json!({"name": "chip", "color": "#ffffff", "shape_types": []}), "a label must allow at least one shape type"),
    ];
    for (definition, reason) in cases {
        let (status, body) = add_label(&app, project, definition.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{definition}");
        assert_eq!(body["error"], reason, "{definition}");
    }
}

#[tokio::test]
async fn a_label_can_keep_its_own_name_and_hotkey_when_edited() {
    let app = test_app().await;
    let project = new_project(&app).await;
    add_label(&app, project, json!({"name": "crack", "color": "#ffffff", "hotkey": "1"})).await;

    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("/api/projects/{project}/labels/1"),
        Some(json!({"name": "crack", "color": "#000000", "hotkey": "1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn labels_keep_their_attribute_definitions_and_reject_bad_ones() {
    let app = test_app().await;
    let (_, project) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Attrs"}))).await;
    let labels = format!("/api/projects/{}/labels", project["id"]);
    let attributes = json!([
        {"name": "severity", "type": "select", "options": ["low", "high"], "default": "low"},
        {"name": "occluded", "type": "checkbox", "default": false},
        {"name": "width", "type": "number", "min": 0.0, "max": null, "default": 1.5},
        {"name": "note", "type": "text", "default": ""}
    ]);
    let (status, label) = call(&app, Method::POST, &labels, Some(json!({"name": "crack", "color": "#e74c3c", "attributes": attributes}))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(label["attributes"], attributes);
    let (_, listed) = call(&app, Method::GET, &labels, None).await;
    assert_eq!(listed[0]["attributes"], attributes);

    let bad = json!([{"name": "severity", "type": "select", "options": ["low"], "default": "high"}]);
    let (status, body) = call(&app, Method::POST, &labels, Some(json!({"name": "chip", "color": "#e67e22", "attributes": bad}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn labels_keep_their_line_width_and_point_radius() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, label) = add_label(&app, project, json!({"name": "crack", "color": "#e74c3c", "line_width": 4, "point_radius": 3})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!((label["line_width"].clone(), label["point_radius"].clone()), (json!(4.0), json!(3.0)));
    let (status, _) = add_label(&app, project, json!({"name": "chip", "color": "#e67e22", "line_width": 0})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
