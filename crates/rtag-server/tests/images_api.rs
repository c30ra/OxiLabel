mod common;

use std::path::Path;

use axum::http::{Method, StatusCode};
use common::{TestApp, call, get_bytes, test_app};
use serde_json::{Value, json};

/// Writes a solid-color PNG of the given size; `shade` makes otherwise equal images differ in content.
fn write_png(root: &Path, relative: &str, width: u32, height: u32, shade: u8) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(width, height, image::Rgb([shade, 0, 0])).save(&path).unwrap();
}

async fn new_project(app: &TestApp) -> i64 {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    project["id"].as_i64().unwrap()
}

async fn import_selection(app: &TestApp, project: i64, paths: &[&str]) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": paths}))).await
}

async fn import(app: &TestApp, project: i64, folder: &str) -> (StatusCode, Value) {
    import_selection(app, project, &[folder]).await
}

async fn browse(app: &TestApp, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, &format!("/api/browse?path={path}"), None).await
}

async fn list(app: &TestApp, project: i64, query: &str) -> Value {
    let (status, page) = call(app, Method::GET, &format!("/api/projects/{project}/images{query}"), None).await;
    assert_eq!(status, StatusCode::OK);
    page
}

fn paths(page: &Value) -> Vec<&str> {
    page["images"].as_array().unwrap().iter().map(|image| image["path"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn importing_a_folder_adds_its_images_recursively() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "batch1/a.png", 40, 30, 1);
    write_png(app.images.path(), "batch1/nested/b.png", 20, 10, 2);
    write_png(app.images.path(), "other/c.png", 5, 5, 3);

    let (status, report) = import(&app, project, "batch1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"added": 2, "skipped": 0, "unsupported": 0, "failed": []}));

    let page = list(&app, project, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(paths(&page), vec!["batch1/a.png", "batch1/nested/b.png"]);
    let first = &page["images"][0];
    assert_eq!((first["width"].clone(), first["height"].clone()), (json!(40), json!(30)));
    assert_eq!(first["annotated"], false);
}

#[tokio::test]
async fn importing_again_skips_known_images_and_adds_new_ones() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 4, 4, 1);
    import(&app, project, "").await;
    write_png(app.images.path(), "b.png", 4, 4, 2);

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report, json!({"added": 1, "skipped": 1, "unsupported": 0, "failed": []}));
    assert_eq!(list(&app, project, "").await["total"], 2);
}

#[tokio::test]
async fn a_copy_of_a_known_image_under_another_name_is_skipped() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 4, 4, 1);
    write_png(app.images.path(), "copy/a-again.png", 4, 4, 1);

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report, json!({"added": 1, "skipped": 1, "unsupported": 0, "failed": []}));
}

#[tokio::test]
async fn unreadable_images_are_reported_and_other_files_counted() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "good.png", 4, 4, 1);
    std::fs::write(app.images.path().join("broken.jpg"), b"not really a jpeg").unwrap();
    std::fs::write(app.images.path().join("notes.txt"), b"hello").unwrap();

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report["added"], 1);
    assert_eq!(report["unsupported"], 1);
    assert_eq!(report["failed"][0]["path"], "broken.jpg");
    assert!(report["failed"][0]["reason"].as_str().is_some_and(|reason| !reason.is_empty()));
    assert_eq!(paths(&list(&app, project, "").await), vec!["good.png"]);
}

#[tokio::test]
async fn paths_outside_the_image_root_are_refused() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for path in ["..", "a/../../etc", "/etc"] {
        let (status, body) = import(&app, project, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["error"], "path must be inside the image root", "{path}");
        let (status, _) = browse(&app, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "browse {path}");
    }
}

#[tokio::test]
async fn a_missing_path_is_reported() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, body) = import(&app, project, "nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "\"nope\" not found");
}

#[tokio::test]
async fn an_empty_selection_is_rejected() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, body) = import_selection(&app, project, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "select at least one folder or image");
}

#[tokio::test]
async fn a_selection_can_mix_single_images_and_folders() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "loose/one.png", 2, 2, 1);
    write_png(app.images.path(), "loose/two.png", 2, 2, 2);
    write_png(app.images.path(), "batch/three.png", 2, 2, 3);
    write_png(app.images.path(), "batch/deep/four.png", 2, 2, 4);

    let (status, report) = import_selection(&app, project, &["loose/two.png", "batch"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"added": 3, "skipped": 0, "unsupported": 0, "failed": []}));
    assert_eq!(
        paths(&list(&app, project, "").await),
        vec!["batch/deep/four.png", "batch/three.png", "loose/two.png"]
    );
}

#[tokio::test]
async fn an_image_selected_together_with_its_folder_is_counted_once() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "batch/a.png", 2, 2, 1);

    let (_, report) = import_selection(&app, project, &["batch", "batch/a.png"]).await;
    assert_eq!(report, json!({"added": 1, "skipped": 0, "unsupported": 0, "failed": []}));
}

#[tokio::test]
async fn browsing_lists_folders_first_then_images_and_hides_other_files() {
    let app = test_app().await;
    write_png(app.images.path(), "set/b.png", 2, 2, 1);
    write_png(app.images.path(), "set/A.png", 2, 2, 2);
    write_png(app.images.path(), "set/zeta/c.png", 2, 2, 3);
    std::fs::create_dir_all(app.images.path().join("set/alpha")).unwrap();
    std::fs::write(app.images.path().join("set/readme.txt"), b"hi").unwrap();
    std::fs::write(app.images.path().join("set/.hidden.png"), b"hi").unwrap();

    let (status, listing) = browse(&app, "set").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!({
            "path": "set",
            "entries": [
                {"name": "alpha", "path": "set/alpha", "kind": "folder"},
                {"name": "zeta", "path": "set/zeta", "kind": "folder"},
                {"name": "A.png", "path": "set/A.png", "kind": "image"},
                {"name": "b.png", "path": "set/b.png", "kind": "image"}
            ]
        })
    );
}

#[tokio::test]
async fn browsing_without_a_path_lists_the_image_root() {
    let app = test_app().await;
    write_png(app.images.path(), "set/b.png", 2, 2, 1);
    let (_, listing) = call(&app, Method::GET, "/api/browse", None).await;
    assert_eq!(listing, json!({"path": "", "entries": [{"name": "set", "path": "set", "kind": "folder"}]}));
}

#[tokio::test]
async fn images_are_listed_in_pages() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for i in 0..5u8 {
        write_png(app.images.path(), &format!("img{i}.png"), 2, 2, i);
    }
    import(&app, project, "").await;

    let page = list(&app, project, "?offset=2&limit=2").await;
    assert_eq!(page["total"], 5);
    assert_eq!(paths(&page), vec!["img2.png", "img3.png"]);
}

#[tokio::test]
async fn images_of_an_unknown_or_deleted_project_are_not_found() {
    let app = test_app().await;
    let (status, _) = import(&app, 999, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 2, 2, 1);
    import(&app, project, "").await;
    call(&app, Method::DELETE, &format!("/api/projects/{project}"), None).await;
    let (status, _) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn first_image_id(app: &TestApp, project: i64) -> i64 {
    list(app, project, "").await["images"][0]["id"].as_i64().unwrap()
}

async fn thumbnail_size(app: &TestApp, image: i64) -> (u32, u32) {
    let (status, content_type, bytes) = get_bytes(app, &format!("/api/images/{image}/thumbnail")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "image/jpeg");
    let thumbnail = image::load_from_memory(&bytes).unwrap();
    (thumbnail.width(), thumbnail.height())
}

#[tokio::test]
async fn thumbnails_fit_in_256_pixels_keeping_the_aspect_ratio() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "wide.png", 1024, 512, 1);
    import(&app, project, "").await;

    assert_eq!(thumbnail_size(&app, first_image_id(&app, project).await).await, (256, 128));
}

#[tokio::test]
async fn small_images_are_not_upscaled_for_thumbnails() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "small.png", 40, 30, 1);
    import(&app, project, "").await;

    assert_eq!(thumbnail_size(&app, first_image_id(&app, project).await).await, (40, 30));
}

#[tokio::test]
async fn thumbnails_are_cached_after_the_first_request() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 300, 300, 1);
    import(&app, project, "").await;
    let image = first_image_id(&app, project).await;
    thumbnail_size(&app, image).await;

    std::fs::remove_file(app.images.path().join("a.png")).unwrap();
    assert_eq!(thumbnail_size(&app, image).await, (256, 256));
}

#[tokio::test]
async fn thumbnail_of_an_unknown_image_is_not_found() {
    let app = test_app().await;
    let (status, _, _) = get_bytes(&app, "/api/images/999/thumbnail").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn browsing_for_a_project_marks_images_already_imported() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "set/old.png", 2, 2, 1);
    import(&app, project, "set/old.png").await;
    write_png(app.images.path(), "set/new.png", 2, 2, 2);

    let (_, listing) = call(&app, Method::GET, &format!("/api/browse?path=set&project={project}"), None).await;
    assert_eq!(
        listing["entries"],
        json!([
            {"name": "new.png", "path": "set/new.png", "kind": "image", "imported": false},
            {"name": "old.png", "path": "set/old.png", "kind": "image", "imported": true}
        ])
    );
}
