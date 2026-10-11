mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, call, test_app};
use serde_json::{Value, json};

/// A project with labels "defect" (rectangles allowed, id 1) and "crack" (polylines only, id 2),
/// and one imported 400 × 300 image. Returns (project, image id).
async fn setup(app: &TestApp) -> (i64, i64) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    let project = project["id"].as_i64().unwrap();
    let labels = format!("/api/projects/{project}/labels");
    call(app, Method::POST, &labels, Some(json!({"name": "defect", "color": "#e74c3c", "shape_types": ["rectangle"]}))).await;
    call(app, Method::POST, &labels, Some(json!({"name": "crack", "color": "#3498db", "shape_types": ["polyline"]}))).await;
    let path = app.images.path().join("cell.png");
    image::RgbImage::new(400, 300).save(&path).unwrap();
    call(app, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": ["cell.png"]}))).await;
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    (project, page["images"][0]["id"].as_i64().unwrap())
}

fn rectangle(label: i64, min: (f64, f64), max: (f64, f64)) -> Value {
    json!({"label": label, "shape": {"type": "rectangle", "min": {"x": min.0, "y": min.1}, "max": {"x": max.0, "y": max.1}}})
}

async fn load(app: &TestApp, project: i64, image: i64) -> Value {
    let (status, body) = call(app, Method::GET, &format!("/api/projects/{project}/images/{image}/annotations"), None).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn save(app: &TestApp, project: i64, image: i64, base_version: i64, annotations: Value) -> (StatusCode, Value) {
    let body = json!({"base_version": base_version, "annotations": annotations});
    call(app, Method::PUT, &format!("/api/projects/{project}/images/{image}/annotations"), Some(body)).await
}

#[tokio::test]
async fn a_new_image_has_no_annotations_at_version_zero() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    assert_eq!(load(&app, project, image).await, json!({"version": 0, "annotations": []}));
}

#[tokio::test]
async fn saved_rectangles_come_back_in_image_coordinates_with_a_new_version() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let annotations = json!([rectangle(1, (10.5, 20.25), (110.0, 70.0)), rectangle(1, (0.0, 0.0), (400.0, 300.0))]);

    let (status, saved) = save(&app, project, image, 0, annotations.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["version"], 1);

    let loaded = load(&app, project, image).await;
    assert_eq!(loaded["version"], 1);
    assert_eq!(loaded["annotations"], annotations);
}

#[tokio::test]
async fn each_save_replaces_the_whole_set() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    save(&app, project, image, 0, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0)), rectangle(1, (6.0, 6.0), (9.0, 9.0))])).await;
    let (_, saved) = save(&app, project, image, 1, json!([rectangle(1, (2.0, 2.0), (3.0, 3.0))])).await;
    assert_eq!(saved["version"], 2);
    assert_eq!(load(&app, project, image).await["annotations"], json!([rectangle(1, (2.0, 2.0), (3.0, 3.0))]));

    save(&app, project, image, 2, json!([])).await;
    assert_eq!(load(&app, project, image).await, json!({"version": 3, "annotations": []}));
}

#[tokio::test]
async fn a_save_from_a_stale_version_is_rejected_and_changes_nothing() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    save(&app, project, image, 0, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0))])).await;

    let (status, body) = save(&app, project, image, 0, json!([])).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"],
        "these annotations were changed elsewhere since you opened the image (now version 1); reload to see them"
    );
    let loaded = load(&app, project, image).await;
    assert_eq!(loaded["version"], 1);
    assert_eq!(loaded["annotations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_annotations_are_rejected_and_nothing_is_saved() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let cases = [
        (rectangle(9, (1.0, 1.0), (5.0, 5.0)), "annotation 1: there is no label with id 9"),
        (rectangle(2, (1.0, 1.0), (5.0, 5.0)), "annotation 1: label \"crack\" does not allow rectangle shapes"),
        (rectangle(1, (-1.0, 1.0), (5.0, 5.0)), "annotation 1: the shape must lie inside the 400 × 300 image"),
        (rectangle(1, (1.0, 1.0), (401.0, 5.0)), "annotation 1: the shape must lie inside the 400 × 300 image"),
        (rectangle(1, (5.0, 1.0), (5.0, 5.0)), "annotation 1: a rectangle needs a width and a height"),
        (rectangle(1, (6.0, 1.0), (5.0, 5.0)), "annotation 1: a rectangle needs a width and a height"),
    ];
    for (annotation, message) in cases {
        let (status, body) = save(&app, project, image, 0, json!([annotation])).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{message}");
        assert_eq!(body["error"], message);
    }
    assert_eq!(load(&app, project, image).await, json!({"version": 0, "annotations": []}));
}

#[tokio::test]
async fn annotations_of_an_image_in_another_project_are_not_found() {
    let app = test_app().await;
    let (_, image) = setup(&app).await;
    let (_, other) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Other"}))).await;
    let other = other["id"].as_i64().unwrap();
    let (status, _) = call(&app, Method::GET, &format!("/api/projects/{other}/images/{image}/annotations"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = save(&app, other, image, 0, json!([])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_image_list_counts_annotations() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    save(&app, project, image, 0, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0)), rectangle(1, (6.0, 6.0), (9.0, 9.0))])).await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    assert_eq!(page["images"][0]["annotations"], 2);
}

async fn label_usage(app: &TestApp, project: i64, label: i64) -> Value {
    let (status, body) = call(app, Method::GET, &format!("/api/projects/{project}/labels/{label}/usage"), None).await;
    assert_eq!(status, StatusCode::OK);
    body
}

#[tokio::test]
async fn deleting_a_label_in_use_needs_confirmation_and_removes_its_annotations() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    save(&app, project, image, 0, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0)), rectangle(1, (6.0, 6.0), (9.0, 9.0))])).await;
    assert_eq!(label_usage(&app, project, 1).await, json!({"annotations": 2}));

    let (status, body) = call(&app, Method::DELETE, &format!("/api/projects/{project}/labels/1"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "label \"defect\" is used by 2 annotations; confirm to delete them too");
    assert_eq!(load(&app, project, image).await["annotations"].as_array().unwrap().len(), 2);

    let (status, _) = call(&app, Method::DELETE, &format!("/api/projects/{project}/labels/1?confirm=true"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The image changed, so its version moved on and an editor holding version 1 cannot resurrect them.
    assert_eq!(load(&app, project, image).await, json!({"version": 2, "annotations": []}));
    let (status, _) = save(&app, project, image, 1, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0))])).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_unused_label_is_deleted_without_confirmation() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    assert_eq!(label_usage(&app, project, 2).await, json!({"annotations": 0}));
    let (status, _) = call(&app, Method::DELETE, &format!("/api/projects/{project}/labels/2"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn removing_an_image_removes_its_annotations() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    save(&app, project, image, 0, json!([rectangle(1, (1.0, 1.0), (5.0, 5.0))])).await;
    call(&app, Method::POST, &format!("/api/projects/{project}/image-removals"), Some(json!({"ids": [image]}))).await;
    assert_eq!(label_usage(&app, project, 1).await, json!({"annotations": 0}));
}
