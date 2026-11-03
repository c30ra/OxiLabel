mod common;

use std::path::{Path, PathBuf};

use axum::http::{Method, StatusCode};
use common::{TestApp, call, get_bytes, test_app, upload};
use serde_json::{Value, json};

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
    assert_eq!(status, StatusCode::OK);

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

fn png(shade: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(6, 4, image::Rgb([shade, 0, 0]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

async fn new_project(app: &TestApp, name: &str) -> i64 {
    call(app, Method::POST, "/api/projects", Some(json!({"name": name}))).await.1["id"].as_i64().unwrap()
}

async fn paths(app: &TestApp, project: i64) -> Vec<PathBuf> {
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    page["images"].as_array().unwrap().iter().map(|image| PathBuf::from(image["path"].as_str().unwrap())).collect()
}

/// A project ("Gone") with two stored copies and one original; a second project, "Other", also
/// uses one of the copies.
struct Gone {
    project: i64,
    shared_copy: PathBuf,
    own_copy: PathBuf,
    original: PathBuf,
}

async fn project_with_copies_and_an_original(app: &TestApp) -> Gone {
    let project = new_project(app, "Gone").await;
    let (status, report) = upload(app, &format!("/api/projects/{project}/uploads"), &[("set/a.png", png(1)), ("set/sub/b.png", png(2))]).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    std::fs::write(app.picked.path().join("c.png"), png(3)).unwrap();
    let original = app.picked.path().canonicalize().unwrap().join("c.png");
    call(&app.local, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": [original]}))).await;
    let copies = paths(app, project).await;
    let shared_copy = copies.iter().find(|p| p.ends_with("a.png")).unwrap().clone();
    let own_copy = copies.iter().find(|p| p.ends_with("b.png")).unwrap().clone();
    let other = new_project(app, "Other").await;
    call(&app.local, Method::POST, &format!("/api/projects/{other}/imports"), Some(json!({"paths": [shared_copy]}))).await;
    Gone { project, shared_copy, own_copy, original }
}

#[tokio::test]
async fn the_deletion_preview_counts_stored_copies_shared_copies_and_originals() {
    let app = test_app().await;
    let gone = project_with_copies_and_an_original(&app).await;
    let (status, preview) = call(&app, Method::GET, &format!("/api/projects/{}/deletion", gone.project), None).await;
    assert_eq!((status, preview), (StatusCode::OK, json!({"stored_copies": 1, "shared_copies": 1, "originals": 1})));
}

#[tokio::test]
async fn deleting_a_project_keeps_its_image_files_unless_asked() {
    let app = test_app().await;
    let gone = project_with_copies_and_an_original(&app).await;
    let (status, report) = call(&app, Method::DELETE, &format!("/api/projects/{}", gone.project), None).await;
    assert_eq!((status, report), (StatusCode::OK, json!({"deleted": [], "kept": [], "failed": []})));
    assert!(gone.shared_copy.exists() && gone.own_copy.exists() && gone.original.exists());
}

#[tokio::test]
async fn deleting_a_project_with_its_stored_copies_spares_originals_and_shared_copies() {
    let app = test_app().await;
    let gone = project_with_copies_and_an_original(&app).await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{}/images", gone.project), None).await;
    let own_id = page["images"].as_array().unwrap().iter().find(|i| i["path"].as_str().unwrap().ends_with("b.png")).unwrap()["id"].clone();
    assert_eq!(get_bytes(&app, &format!("/api/images/{own_id}/thumbnail")).await.0, StatusCode::OK);
    let thumbnails = app.cache.path().join("thumbnails");
    assert_eq!(std::fs::read_dir(&thumbnails).unwrap().count(), 1);

    let (status, report) = call(&app, Method::DELETE, &format!("/api/projects/{}?delete_stored_copies=true", gone.project), None).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report, json!({"deleted": [gone.own_copy], "kept": [{"path": gone.shared_copy, "project_name": "Other"}], "failed": []}));
    assert!(!gone.own_copy.exists(), "the copy only this project used is gone");
    assert!(!gone.own_copy.parent().unwrap().exists(), "and so is its emptied folder");
    assert!(gone.shared_copy.exists() && gone.original.exists());
    assert_eq!(std::fs::read_dir(&thumbnails).unwrap().count(), 0, "no remaining image has the deleted copy's content");
    assert!(Path::new(app.uploads.path()).exists(), "the upload folder itself stays");
}

#[tokio::test]
async fn a_stored_copy_already_missing_is_reported_as_failed() {
    let app = test_app().await;
    let gone = project_with_copies_and_an_original(&app).await;
    std::fs::remove_file(&gone.own_copy).unwrap();
    let (_, report): (_, Value) = call(&app, Method::DELETE, &format!("/api/projects/{}?delete_stored_copies=true", gone.project), None).await;
    assert_eq!(report["deleted"], json!([]));
    assert_eq!(report["failed"][0]["path"], json!(gone.own_copy));
}
