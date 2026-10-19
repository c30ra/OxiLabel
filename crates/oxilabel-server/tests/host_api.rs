//! Images from any folder of the host (ADR 0003): the host's own folder dialog and imports by
//! absolute path for requests from the server's machine, uploads from anywhere.

mod common;

use std::path::Path;

use axum::http::{Method, StatusCode};
use common::{TestApp, call, get_bytes, test_app, upload};
use serde_json::{Value, json};

fn png(shade: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(6, 4, image::Rgb([shade, 0, 0]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn write(folder: &Path, relative: &str, shade: u8) {
    let path = folder.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, png(shade)).unwrap();
}

async fn project(app: &TestApp) -> i64 {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Host"}))).await;
    project["id"].as_i64().unwrap()
}

#[tokio::test]
async fn the_server_says_whether_a_request_comes_from_its_own_machine() {
    let app = test_app().await;
    assert_eq!(call(&app.local, Method::GET, "/api/host", None).await.1, json!({"local": true}));
    assert_eq!(call(&app, Method::GET, "/api/host", None).await.1, json!({"local": false}));
}

#[tokio::test]
async fn the_host_folder_dialog_opens_only_for_the_servers_own_machine() {
    let app = test_app().await;
    let (status, picked) = call(&app.local, Method::POST, "/api/host/folder-dialog", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(picked["path"], app.picked.path().canonicalize().unwrap().to_str().unwrap());
    let (status, _) = call(&app, Method::POST, "/api/host/folder-dialog", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_host_folder_outside_the_image_root_is_counted_and_imported_by_its_absolute_path() {
    let app = test_app().await;
    let id = project(&app).await;
    write(app.picked.path(), "a.png", 1);
    write(app.picked.path(), "deep/b.png", 2);
    let folder = app.picked.path().canonicalize().unwrap().to_str().unwrap().to_owned();

    let (status, count) = call(&app.local, Method::GET, &format!("/api/browse/count?path={folder}"), None).await;
    assert_eq!((status, count), (StatusCode::OK, json!({"images": 2})));
    let (status, report) = call(&app.local, Method::POST, &format!("/api/projects/{id}/imports"), Some(json!({"paths": [folder]}))).await;
    assert_eq!((status, report["added"].clone()), (StatusCode::OK, json!(2)));

    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{id}/images"), None).await;
    let first = &page["images"][0];
    assert_eq!(first["path"], format!("{folder}/a.png"));
    let (status, _, _) = get_bytes(&app, &format!("/api/images/{}/thumbnail", first["id"])).await;
    assert_eq!(status, StatusCode::OK, "thumbnails read the file where it is");
}

#[tokio::test]
async fn absolute_paths_are_refused_from_other_machines() {
    let app = test_app().await;
    let id = project(&app).await;
    let folder = app.picked.path().canonicalize().unwrap().to_str().unwrap().to_owned();
    let (status, _) = call(&app, Method::GET, &format!("/api/browse/count?path={folder}"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::POST, &format!("/api/projects/{id}/imports"), Some(json!({"paths": ["/etc"]}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn uploaded_images_are_kept_by_the_server_and_imported_once() {
    let app = test_app().await;
    let id = project(&app).await;
    let uri = format!("/api/projects/{id}/uploads");
    let files = [("set/a.png", png(1)), ("set/sub/b.png", png(2)), ("set/notes.txt", b"text".to_vec())];
    let (status, report) = upload(&app, &uri, &files).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!((report["added"].clone(), report["unsupported"].clone()), (json!(2), json!(1)));

    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{id}/images"), None).await;
    let paths: Vec<String> = page["images"].as_array().unwrap().iter().map(|i| i["path"].as_str().unwrap().to_owned()).collect();
    let uploads = app.uploads.path().canonicalize().unwrap();
    assert!(paths.iter().all(|p| Path::new(p).starts_with(&uploads)), "kept in the upload folder: {paths:?}");
    let (status, _, _) = get_bytes(&app, &format!("/api/images/{}/thumbnail", page["images"][0]["id"])).await;
    assert_eq!(status, StatusCode::OK);

    // The same images again (another name, same content) add nothing and leave no extra copies.
    let (_, again) = upload(&app, &uri, &[("other/a.png", png(1))]).await;
    assert_eq!((again["added"].clone(), again["skipped"].clone()), (json!(0), json!(1)));
    let stored: usize = walk(&uploads);
    assert_eq!(stored, 2, "the duplicate's copy is removed");
    let (status, _): (StatusCode, Value) = upload(&app, &uri, &[("../escape.png", png(3))]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "names cannot leave the upload folder");
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().flatten().map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 }).sum()
}
