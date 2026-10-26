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
    assert_eq!(list, json!([{"id": created["id"], "name": "Cracks", "images": 0, "annotated": 0, "jobs": 0, "label_colors": []}]));
}

#[tokio::test]
async fn deleted_project_disappears_from_the_list() {
    let app = test_app().await;
    let (_, keep) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Keep"}))).await;
    let (_, gone) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Gone"}))).await;

    let (status, _) = call(&app, Method::DELETE, &format!("/api/projects/{}", gone["id"]), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(list[0]["id"], keep["id"]);
    assert_eq!(list.as_array().unwrap().len(), 1);
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

#[tokio::test]
async fn the_list_summarises_each_projects_images_and_labels() {
    let app = test_app().await;
    let (_, project) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    let id = project["id"].as_i64().unwrap();
    for (name, color) in [("crack", "#e74c3c"), ("chip", "#3498db")] {
        call(&app, Method::POST, &format!("/api/projects/{id}/labels"), Some(json!({"name": name, "color": color}))).await;
    }
    for (file, shade) in [("a.png", 1), ("b.png", 2)] {
        image::RgbImage::from_pixel(20, 10, image::Rgb([shade, 0, 0])).save(app.images.path().join(file)).unwrap();
    }
    call(&app, Method::POST, &format!("/api/projects/{id}/imports"), Some(json!({"paths": ["a.png", "b.png"]}))).await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{id}/images"), None).await;
    let image = page["images"][0]["id"].as_i64().unwrap();
    let rectangle = json!({"label": 1, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 5.0, "y": 5.0}}});
    call(&app, Method::PUT, &format!("/api/projects/{id}/images/{image}/annotations"), Some(json!({"base_version": 0, "annotations": [rectangle]}))).await;

    // Saving an annotation marked the image annotated (ticket 26).
    let (_, list) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(list, json!([{"id": id, "name": "Cells", "images": 2, "annotated": 1, "jobs": 0, "label_colors": ["#e74c3c", "#3498db"]}]));
}

#[tokio::test]
async fn a_new_project_can_copy_another_projects_labels_with_their_ids() {
    let app = test_app().await;
    let (_, source) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "EL defects"}))).await;
    let labels = format!("/api/projects/{}/labels", source["id"]);
    let attributes = json!([{"name": "occluded", "type": "checkbox", "default": false}]);
    call(&app, Method::POST, &labels, Some(json!({"name": "crack", "color": "#e74c3c", "hotkey": "1", "attributes": attributes}))).await;
    call(&app, Method::POST, &labels, Some(json!({"name": "gone", "color": "#3498db"}))).await;
    call(&app, Method::POST, &labels, Some(json!({"name": "ignore", "color": "#7f8c8d", "ignore": true, "shape_types": ["mask"]}))).await;
    call(&app, Method::DELETE, &format!("{labels}/2"), None).await;

    let (status, copy) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Cell broken", "copy_labels_from": source["id"]}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let copied_labels = format!("/api/projects/{}/labels", copy["id"]);
    let (_, original) = call(&app, Method::GET, &labels, None).await;
    let (_, copied) = call(&app, Method::GET, &copied_labels, None).await;
    assert_eq!(copied, original, "same ids (1 and 3), names, colors, keys, shapes and attributes");
    let (_, next) = call(&app, Method::POST, &copied_labels, Some(json!({"name": "new", "color": "#e67e22"}))).await;
    assert_eq!(next["id"], 4, "the copy never hands out an id the source used");

    let (status, _) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "X", "copy_labels_from": 999}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
