mod common;

use std::time::Duration;

use axum::{
    Router,
    http::{Method, StatusCode},
};
use common::{TestApp, call, login, test_app, test_app_with_worker};
use oxilabel_core::{
    inference::DEFAULT_WORKER_URL,
    worker::{PredictRequest, WorkerModel},
};
use oxilabel_server::fake_worker::{FakeRequest, FakeWorker};
use serde_json::{Value, json};

/// A project with labels "crack" (boxes) and "chip" (boxes and polygons) and `count` 20 × 10
/// images; returns its id and the image ids in path order.
async fn project(app: &TestApp, count: usize) -> (i64, Vec<i64>) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "p"}))).await;
    let p = project["id"].as_i64().unwrap();
    call(app, Method::POST, &format!("/api/projects/{p}/labels"), Some(json!({"name": "Crack", "color": "#ff0000", "shape_types": ["rectangle", "polygon"]}))).await;
    call(app, Method::POST, &format!("/api/projects/{p}/labels"), Some(json!({"name": "chip", "color": "#00ff00", "shape_types": ["rectangle"]}))).await;
    let paths: Vec<String> = (0..count).map(|i| format!("img{i}.png")).collect();
    for (i, path) in paths.iter().enumerate() {
        image::RgbImage::from_pixel(20, 10, image::Rgb([i as u8, 1, 2])).save(app.images.path().join(path)).unwrap();
    }
    call(app, Method::POST, &format!("/api/projects/{p}/imports"), Some(json!({"paths": paths}))).await;
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    (p, page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect())
}

/// Registers the fake model: "dent" becomes "chip"; "crack" finds "Crack" by name.
async fn model(app: &TestApp) -> i64 {
    let definition = json!({"name": "fake v1", "adapter": "ultralytics", "weights": "/models/fake.pt", "options": {"conf": 0.25}, "class_map": {"dent": "chip"}});
    let (status, model) = call(app, Method::POST, "/api/models", Some(definition)).await;
    assert_eq!(status, StatusCode::CREATED, "{model}");
    model["id"].as_i64().unwrap()
}

/// Starts a run and waits until it ends; returns its final status.
async fn run(app: &Router, project: i64, request: Value) -> Value {
    let (status, run) = call(app, Method::POST, &format!("/api/projects/{project}/pre-annotations"), Some(request)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    wait(app, run["id"].as_u64().unwrap()).await
}

async fn wait(app: &Router, run: u64) -> Value {
    for _ in 0..200 {
        let (_, status) = call(app, Method::GET, &format!("/api/runs/{run}"), None).await;
        if status["state"] != "running" {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the run did not end");
}

async fn annotations(app: &Router, project: i64, image: i64) -> Value {
    call(app, Method::GET, &format!("/api/projects/{project}/images/{image}/annotations"), None).await.1
}

#[tokio::test]
async fn models_are_registered_changed_and_deleted_by_admins() {
    let app = test_app().await;
    let id = model(&app).await;
    let (_, list) = call(&app, Method::GET, "/api/models", None).await;
    assert_eq!(list, json!([{"id": id, "name": "fake v1", "adapter": "ultralytics", "weights": "/models/fake.pt", "options": {"conf": 0.25}, "class_map": {"dent": "chip"}}]));
    let changed = json!({"name": "fake v2", "adapter": "/adapters/defectnet.py", "weights": "/models/best.pt"});
    let (status, model) = call(&app, Method::PUT, &format!("/api/models/{id}"), Some(changed)).await;
    assert_eq!((status, &model["name"], &model["class_map"]), (StatusCode::OK, &json!("fake v2"), &json!({})));
    let (status, _) = call(&app, Method::POST, "/api/models", Some(json!({"name": "FAKE V2", "adapter": "x", "weights": "y"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&app, Method::POST, "/api/models", Some(json!({"name": " ", "adapter": "x", "weights": "y"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    assert_eq!(call(&anna, Method::GET, "/api/models", None).await.0, StatusCode::OK);
    assert_eq!(call(&anna, Method::POST, "/api/models", Some(json!({"name": "mine", "adapter": "x", "weights": "y"}))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&anna, Method::DELETE, &format!("/api/models/{id}"), None).await.0, StatusCode::FORBIDDEN);

    assert_eq!(call(&app, Method::DELETE, &format!("/api/models/{id}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, Method::GET, "/api/models", None).await.1, json!([]));
}

#[tokio::test]
async fn the_worker_health_and_a_models_classes_come_from_the_worker() {
    let worker = FakeWorker::default();
    let app = test_app_with_worker(Some(worker.clone().serve().await)).await;
    let (_, health) = call(&app, Method::GET, "/api/worker", None).await;
    assert_eq!(health, json!({"reachable": true, "device": "fake gpu", "error": null}));
    let id = model(&app).await;
    let (status, classes) = call(&app, Method::GET, &format!("/api/models/{id}/classes"), None).await;
    assert_eq!((status, classes), (StatusCode::OK, json!(["crack", "dent", "smudge"])));
    let fake = WorkerModel { adapter: "ultralytics".into(), weights: "/models/fake.pt".into(), options: json!({"conf": 0.25}).as_object().unwrap().clone() };
    assert_eq!(worker.requests(), [FakeRequest::Classes(fake)], "the worker loads the model as registered");

    // Without a worker the health says so, and runs are refused.
    let lonely = test_app_with_worker(None).await;
    let (_, health) = call(&lonely, Method::GET, "/api/worker", None).await;
    assert_eq!(health["reachable"], json!(false));
    assert!(health["error"].as_str().unwrap().contains("Settings screen"), "{health}");
    let unreachable = test_app_with_worker(Some("http://127.0.0.1:9".into())).await;
    assert_eq!(call(&unreachable, Method::GET, "/api/worker", None).await.1["reachable"], json!(false));
}

#[tokio::test]
async fn a_run_makes_pending_pre_annotations_with_source_and_confidence() {
    let worker = FakeWorker::default();
    let app = test_app_with_worker(Some(worker.clone().serve().await)).await;
    let (p, images) = project(&app, 2).await;
    let m = model(&app).await;
    let status = run(&app, p, json!({"model": m, "image_ids": images})).await;
    // Each image goes to the worker as its file's bytes, with the model's options.
    let sent: Vec<_> = worker.requests().into_iter().map(|request| match request {
        FakeRequest::Predict(PredictRequest { model, image }) => (model.options, image),
        other => panic!("{other:?}"),
    }).collect();
    let file = |name: &str| std::fs::read(app.images.path().join(name)).unwrap();
    let options = json!({"conf": 0.25}).as_object().unwrap().clone();
    assert_eq!(sent, [(options.clone(), file("img0.png")), (options, file("img1.png"))]);
    assert_eq!((&status["state"], &status["total"], &status["done"], &status["created"]), (&json!("finished"), &json!(2), &json!(2), &json!(4)), "{status}");
    let skipped: Vec<_> = status["skipped"].as_array().unwrap().iter().map(|s| (s["reason"].as_str().unwrap().to_owned(), s["count"].as_i64().unwrap())).collect();
    assert_eq!(skipped, [
        ("no label for class \"smudge\"".to_owned(), 2),
        ("the shape must lie inside the 20 × 10 image".to_owned(), 2),
    ]);
    let list = annotations(&app, p, images[0]).await;
    assert_eq!(list["annotations"], json!([
        {"label": 1, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 9.0, "y": 6.0}}, "source": "fake v1", "confidence": 0.91, "pending": true},
        {"label": 2, "shape": {"type": "rectangle", "min": {"x": 2.0, "y": 2.0}, "max": {"x": 4.0, "y": 4.0}}, "source": "fake v1", "confidence": 0.4, "pending": true},
    ]));
    assert_eq!(list["version"], json!(1), "an open editor is told to reload");
    // Unreviewed pre-annotations do not count yet.
    let (_, statistics) = call(&app, Method::GET, &format!("/api/projects/{p}/statistics"), None).await;
    assert_eq!(statistics["labels"][0]["annotations"], json!(0), "{statistics}");
}

#[tokio::test]
async fn running_again_replaces_unreviewed_pre_annotations_and_keeps_the_rest() {
    let app = test_app_with_worker(Some(FakeWorker::default().serve().await)).await;
    let (p, images) = project(&app, 1).await;
    let m = model(&app).await;
    run(&app, p, json!({"model": m, "image_ids": images})).await;
    // Accept the crack, add a manual box.
    let mut list = annotations(&app, p, images[0]).await;
    list["annotations"][0]["pending"] = json!(false);
    list["annotations"].as_array_mut().unwrap().push(json!({"label": 2, "shape": {"type": "rectangle", "min": {"x": 5.0, "y": 5.0}, "max": {"x": 7.0, "y": 7.0}}}));
    let uri = format!("/api/projects/{p}/images/{}/annotations", images[0]);
    let (status, _) = call(&app, Method::PUT, &uri, Some(json!({"base_version": list["version"], "annotations": list["annotations"]}))).await;
    assert_eq!(status, StatusCode::OK);

    run(&app, p, json!({"model": m, "image_ids": images})).await;
    let list = annotations(&app, p, images[0]).await["annotations"].clone();
    let summary: Vec<_> = list.as_array().unwrap().iter().map(|a| (a["label"].clone(), a["source"].clone(), a.get("pending").cloned().unwrap_or(json!(false)))).collect();
    assert_eq!(summary, [
        (json!(1), json!("fake v1"), json!(false)),
        (json!(2), json!(null), json!(false)),
        (json!(1), json!("fake v1"), json!(true)),
        (json!(2), json!("fake v1"), json!(true)),
    ]);
}

#[tokio::test]
async fn exports_leave_out_unreviewed_pre_annotations() {
    let app = test_app_with_worker(Some(FakeWorker::default().serve().await)).await;
    let (p, images) = project(&app, 1).await;
    let m = model(&app).await;
    run(&app, p, json!({"model": m, "image_ids": images})).await;
    let uri = format!("/api/projects/{p}/exports");
    let request = json!({"format": "yolo-detect", "images": {"kind": "all"}, "include_images": "symlink", "split": {"train": 100, "val": 0, "test": 0}});
    let export = || call(&app, Method::POST, &uri, Some(request.clone()));
    let (status, result) = export().await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["annotation_count"], json!(0));
    let mut list = annotations(&app, p, images[0]).await;
    list["annotations"][0]["pending"] = json!(false);
    let uri = format!("/api/projects/{p}/images/{}/annotations", images[0]);
    call(&app, Method::PUT, &uri, Some(json!({"base_version": list["version"], "annotations": list["annotations"]}))).await;
    assert_eq!(export().await.1["annotation_count"], json!(1));
}

#[tokio::test]
async fn annotators_run_models_only_on_images_of_their_jobs() {
    let app = test_app_with_worker(Some(FakeWorker::default().serve().await)).await;
    let (p, images) = project(&app, 2).await;
    let m = model(&app).await;
    let (_, anna) = call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    call(&app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": [images[0]], "assignee": anna["id"]}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    let (status, _) = call(&anna, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": m, "image_ids": images}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let status = run(&anna, p, json!({"model": m, "image_ids": [images[0]]})).await;
    assert_eq!(status["created"], json!(2));
    let (status, _) = call(&anna, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": m, "image_ids": []}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admins_set_where_the_worker_is_and_it_applies_at_once() {
    let app = test_app().await;
    let (_, settings) = call(&app, Method::GET, "/api/settings/worker", None).await;
    assert_eq!(settings, json!({"command": null, "url": null}));
    assert_eq!(call(&app, Method::GET, "/api/worker", None).await.1["reachable"], json!(false));

    let url = FakeWorker::default().serve().await;
    let (status, settings) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"url": format!(" {url}/ ")}))).await;
    assert_eq!((status, &settings), (StatusCode::OK, &json!({"command": null, "url": url})));
    assert_eq!(call(&app, Method::GET, "/api/worker", None).await.1["device"], json!("fake gpu"));
    // A command without a URL listens on the default one.
    let (_, settings) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"command": "exit 0"}))).await;
    assert_eq!(settings, json!({"command": "exit 0", "url": DEFAULT_WORKER_URL}));
    let (status, _) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"url": "ftp://x"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    call(&app, Method::PUT, "/api/settings/worker", Some(json!({}))).await;
    assert_eq!(call(&app, Method::GET, "/api/settings/worker", None).await.1, json!({"command": null, "url": null}));

    call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    assert_eq!(call(&anna, Method::GET, "/api/settings/worker", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&anna, Method::PUT, "/api/settings/worker", Some(json!({"command": "rm -rf /"}))).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_worker_command_is_started_by_the_server() {
    let app = test_app().await;
    let marker = app.cache.path().join("started");
    let command = format!("touch {} && sleep 30", marker.display());
    let (status, _) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"command": command, "url": "http://127.0.0.1:9"}))).await;
    assert_eq!(status, StatusCode::OK);
    for _ in 0..50 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(marker.exists(), "the command ran");
    call(&app, Method::PUT, "/api/settings/worker", Some(json!({}))).await;
}
