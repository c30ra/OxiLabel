mod common;

use std::io::Write;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{TestApp, call, get_bytes, login, test_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Sends an archive and an import plan.
async fn import(app: &Router, archive: &[u8], plan: Value) -> (StatusCode, Value) {
    let boundary = "rtag-archive-boundary";
    let mut body = Vec::new();
    write!(body, "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"p.rtag.zip\"\r\nContent-Type: application/zip\r\n\r\n").unwrap();
    body.extend_from_slice(archive);
    write!(body, "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"plan\"\r\n\r\n{plan}\r\n--{boundary}--\r\n").unwrap();
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/project-archives")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn export(app: &Router, project: i64, images: bool) -> Vec<u8> {
    let (status, content_type, bytes) = get_bytes(app, &format!("/api/projects/{project}/archive?images={images}")).await;
    assert_eq!((status, content_type.as_str()), (StatusCode::OK, "application/zip"), "{}", String::from_utf8_lossy(&bytes));
    bytes
}

fn png(width: u32, height: u32, shade: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(width, height, image::Rgb([shade, 3, 4])).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

/// A project with a deleted label (so ids have a gap), attributes, two images in folders, a job
/// assigned to "anna", every kind of annotation on the first image and one image marked annotated.
async fn project(app: &TestApp) -> (i64, Vec<i64>) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "EL defects"}))).await;
    let p = project["id"].as_i64().unwrap();
    let labels = format!("/api/projects/{p}/labels");
    let attributes = json!([{"name": "severity", "type": "select", "options": ["low", "high"], "default": "low"}]);
    call(app, Method::POST, &labels, Some(json!({"name": "gone", "color": "#000000"}))).await;
    call(app, Method::POST, &labels, Some(json!({"name": "crack", "color": "#ff0000", "hotkey": "1", "attributes": attributes, "line_width": 5.0}))).await;
    call(app, Method::POST, &labels, Some(json!({"name": "good", "color": "#00ff00", "shape_types": ["tag"]}))).await;
    call(app, Method::DELETE, &format!("{labels}/1"), None).await;
    for (path, shade) in [("front/a.png", 1), ("back/b.png", 2)] {
        let file = app.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, png(40, 30, shade)).unwrap();
    }
    call(app, Method::POST, &format!("/api/projects/{p}/imports"), Some(json!({"paths": ["front", "back"]}))).await;
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    let images: Vec<i64> = page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect();
    let (_, anna) = call(app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    call(app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": [images[1]], "assignee": anna["id"]}))).await;
    let annotations = json!([
        {"label": 3, "shape": {"type": "tag"}},
        {"label": 2, "shape": {"type": "rectangle", "min": {"x": 1.5, "y": 1.0}, "max": {"x": 9.0, "y": 6.25}}, "attributes": {"severity": "high"}},
        {"label": 2, "shape": {"type": "oriented_rectangle", "centre": {"x": 20.0, "y": 15.0}, "width": 8.0, "height": 4.0, "angle": 0.5}},
        {"label": 2, "shape": {"type": "polygon", "points": [{"x": 1.0, "y": 1.0}, {"x": 9.0, "y": 1.0}, {"x": 5.0, "y": 9.0}]}, "source": "yolo", "confidence": 0.75, "pending": true},
        {"label": 2, "shape": {"type": "polyline", "points": [{"x": 1.0, "y": 20.0}, {"x": 30.0, "y": 25.0}]}, "hidden": true},
        {"label": 2, "shape": {"type": "points", "points": [{"x": 3.0, "y": 3.0}]}, "locked": true},
        {"label": 2, "shape": {"type": "ellipse", "centre": {"x": 20.0, "y": 15.0}, "rx": 5.0, "ry": 3.0, "angle": 0.25}},
        {"label": 2, "shape": {"type": "mask", "left": 2, "top": 3, "width": 2, "height": 2, "rle": [1, 3]}},
    ]);
    let uri = format!("/api/projects/{p}/images/{}/annotations", images[1]);
    let (status, body) = call(app, Method::PUT, &uri, Some(json!({"base_version": 0, "annotations": annotations}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    call(app, Method::PUT, &format!("/api/projects/{p}/images/{}/status", images[1]), Some(json!({"annotated": true}))).await;
    (p, images)
}

/// What a project holds, without ids that differ between servers: labels, jobs, and per image
/// (by file name) its size, status, job number and annotations.
async fn contents(app: &Router, project: i64) -> Value {
    let (_, labels) = call(app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    let (_, jobs) = call(app, Method::GET, &format!("/api/projects/{project}/jobs"), None).await;
    let jobs: Vec<Value> = jobs.as_array().unwrap().iter().map(|job| json!([job["number"], job["assignee"]["username"], job["images"]])).collect();
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    let mut images = Vec::new();
    for image in page["images"].as_array().unwrap() {
        let (_, annotations) = call(app, Method::GET, &format!("/api/projects/{project}/images/{}/annotations", image["id"]), None).await;
        let name = image["path"].as_str().unwrap().rsplit('/').next().unwrap().to_owned();
        images.push(json!([name, image["width"], image["height"], image["annotated"], annotations["annotations"]]));
    }
    images.sort_by_key(|image| image[0].as_str().unwrap().to_owned());
    json!({"labels": labels, "jobs": jobs, "images": images})
}

#[tokio::test]
async fn an_archive_with_images_moves_a_project_to_another_server_without_loss() {
    let source = test_app().await;
    let (p, _) = project(&source).await;
    let archive = export(&source, p, true).await;

    let target = test_app().await;
    call(&target, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let (status, report) = import(&target, &archive, json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    assert_eq!((&report["images"], &report["missing"], &report["annotations"], &report["unassigned_jobs"]), (&json!(2), &json!([]), &json!(8), &json!([])));
    let q = report["project"]["id"].as_i64().unwrap();
    assert_eq!(report["project"]["name"], json!("EL defects"));
    assert_eq!(contents(&target, q).await, contents(&source, p).await);
    // The images were kept on the target, and its label counter goes on after the source's.
    let (_, page) = call(&target, Method::GET, &format!("/api/projects/{q}/images"), None).await;
    let path = page["images"][0]["path"].as_str().unwrap();
    assert!(path.starts_with(target.uploads.path().canonicalize().unwrap().to_str().unwrap()), "{path}");
    let (_, label) = call(&target, Method::POST, &format!("/api/projects/{q}/labels"), Some(json!({"name": "new", "color": "#123456"}))).await;
    assert_eq!(label["id"], json!(4));
    let (_, thumbnail, _) = get_bytes(&target, &format!("/api/images/{}/thumbnail", page["images"][0]["id"])).await;
    assert_eq!(thumbnail, "image/jpeg");
}

#[tokio::test]
async fn without_images_they_are_found_at_their_path_or_in_a_folder_by_content() {
    let source = test_app().await;
    let (p, _) = project(&source).await;
    let archive = export(&source, p, false).await;

    // On the same server the paths still hold the same files.
    let (status, report) = import(&source, &archive, json!({"name": "copy"})).await;
    assert_eq!((status, &report["missing"], &report["project"]["name"]), (StatusCode::CREATED, &json!([]), &json!("copy")), "{report}");

    // Elsewhere: a.png is in another folder, b.png too but with other content.
    let target = test_app().await;
    for (path, shade) in [("moved/deep/a.png", 1), ("moved/b.png", 9)] {
        let file = target.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, png(40, 30, shade)).unwrap();
    }
    let (_, report) = import(&target, &archive, json!({})).await;
    assert_eq!((&report["images"], &report["missing"]), (&json!(0), &json!(["back/b.png", "front/a.png"])));
    let (status, report) = import(&target, &archive, json!({"images_folder": "moved"})).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    assert_eq!((&report["images"], &report["missing"]), (&json!(1), &json!(["back/b.png"])));
    assert_eq!(report["annotations"], json!(8));
    // Anna is not on the target: her job stays, unassigned.
    assert_eq!(report["unassigned_jobs"], json!([1]));
    let q = report["project"]["id"].as_i64().unwrap();
    let (_, page) = call(&target, Method::GET, &format!("/api/projects/{q}/images"), None).await;
    assert_eq!(page["images"][0]["path"], json!("moved/deep/a.png"));
}

#[tokio::test]
async fn a_newer_or_foreign_file_is_refused_with_its_reason() {
    let app = test_app().await;
    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        zip.start_file("project.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"format": "rtag-project", "version": 7}"#).unwrap();
        zip.finish().unwrap();
    }
    let (status, body) = import(&app, &bytes, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("newer rtag (format version 7)"), "{body}");
    let (status, body) = import(&app, b"hello", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("not an rtag project archive"), "{body}");
    let (_, projects) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(projects, json!([]), "nothing was created");
}

#[tokio::test]
async fn annotators_can_neither_export_nor_import_archives() {
    let app = test_app().await;
    let (p, _) = project(&app).await;
    let archive = export(&app, p, false).await;
    let anna = login(&app, "anna", "anna's password").await;
    assert_eq!(get_bytes(&anna, &format!("/api/projects/{p}/archive?images=false")).await.0, StatusCode::FORBIDDEN);
    assert_eq!(import(&anna, &archive, json!({})).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn copies_of_one_image_export_and_import_and_odd_archives_get_sane_projects() {
    let source = test_app().await;
    let (_, project) = call(&source, Method::POST, "/api/projects", Some(json!({"name": "Café ü"}))).await;
    let p = project["id"].as_i64().unwrap();
    for path in ["one/x.png", "two/x.png"] {
        let file = source.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, png(20, 10, 5)).unwrap();
    }
    // The second copy is skipped by hash on import into a project; put both in by hand.
    let (_, report) = call(&source, Method::POST, &format!("/api/projects/{p}/imports"), Some(json!({"paths": ["one/x.png"]}))).await;
    assert_eq!(report["added"], json!(1));
    let archive = export(&source, p, true).await;
    let (_, _, bytes) = get_bytes(&source, &format!("/api/projects/{p}/archive?images=false")).await;
    let mut reader = rtag_formats::archive::ArchiveReader::open(std::io::Cursor::new(bytes)).unwrap();
    let mut project_json = reader.project().unwrap();
    // Two images with the same content, a blank name, a next label id below the labels' ids.
    let mut copy = project_json.images[0].clone();
    copy.path = "gone/x.png".into();
    project_json.images.push(copy);
    project_json.name = " ".into();
    project_json.labels.push(serde_json::from_value(json!({"id": 5, "name": "late", "color": "#00ff00", "hotkey": null, "shape_types": ["rectangle"], "ignore": false})).unwrap());
    project_json.next_label_id = 1;
    let mut forged = Vec::new();
    rtag_formats::archive::ArchiveWriter::new(std::io::Cursor::new(&mut forged)).finish(&project_json).unwrap();

    let target = test_app().await;
    let (status, report) = import(&target, &archive, json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    let (status, report) = import(&source, &forged, json!({"images_folder": "."})).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    assert_eq!(report["project"]["name"], json!("Imported project"));
    assert_eq!((&report["images"], &report["missing"]), (&json!(1), &json!(["gone/x.png"])), "one file, imported once");
    let q = report["project"]["id"].as_i64().unwrap();
    let (status, label) = call(&source, Method::POST, &format!("/api/projects/{q}/labels"), Some(json!({"name": "new", "color": "#123456"}))).await;
    assert_eq!((status, &label["id"]), (StatusCode::CREATED, &json!(6)));

    let (_, _, _) = get_bytes(&source, &format!("/api/projects/{p}/archive?images=true")).await;
    let response = source
        .clone()
        .oneshot(Request::builder().uri(format!("/api/projects/{p}/archive?images=false")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let disposition = response.headers()["content-disposition"].to_str().unwrap().to_owned();
    assert_eq!(disposition, "attachment; filename=\"Caf---.rtag.zip\"; filename*=UTF-8''Caf%C3%A9%20%C3%BC.rtag.zip");
}
