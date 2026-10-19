mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    http::{Method, StatusCode},
    routing::{get, post},
};
use common::{TestApp, call, login, test_app, test_app_with_worker};
use serde_json::{Value, json};

/// A worker implementing the contract: it predicts, for every image, a crack box, a "dent" box,
/// a class no label has, and a polygon reaching outside the image. With `delay` it takes that
/// long per image; with `failing` every prediction fails.
#[derive(Clone, Default)]
struct FakeWorker {
    delay: Duration,
    failing: bool,
    /// Images predicted so far.
    seen: Arc<AtomicUsize>,
}

impl FakeWorker {
    /// Serves the worker on a free local port and returns its URL.
    async fn start(self) -> String {
        let app = Router::new()
            .route("/health", get(|| async { Json(json!({"device": "fake gpu"})) }))
            .route("/classes", post(|Json(model): Json<Value>| async move {
                assert_eq!(model["weights"], json!("/models/fake.pt"));
                Json(json!({"classes": ["crack", "dent", "smudge"]}))
            }))
            .route("/predict", post(move |Json(request): Json<Value>| {
                let worker = self.clone();
                async move {
                    tokio::time::sleep(worker.delay).await;
                    worker.seen.fetch_add(1, Ordering::SeqCst);
                    if worker.failing {
                        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "out of GPU memory"}))));
                    }
                    // The image arrives as its bytes, base64 encoded.
                    assert!(request["image"].as_str().is_some_and(|image| image.len() > 50), "{request}");
                    assert_eq!(request["options"], json!({"conf": 0.25}));
                    Ok(Json(json!({"predictions": [
                        {"class": "crack", "confidence": 0.91, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 9.0, "y": 6.0}}},
                        {"class": "dent", "confidence": 0.4, "shape": {"type": "rectangle", "min": {"x": 2.0, "y": 2.0}, "max": {"x": 4.0, "y": 4.0}}},
                        {"class": "smudge", "confidence": 0.8, "shape": {"type": "points", "points": [{"x": 3.0, "y": 3.0}]}},
                        {"class": "crack", "confidence": 0.7, "shape": {"type": "polygon", "points": [{"x": 0.0, "y": 0.0}, {"x": 500.0, "y": 0.0}, {"x": 0.0, "y": 5.0}]}},
                    ]})))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    }
}

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
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
    let (_, health) = call(&app, Method::GET, "/api/worker", None).await;
    assert_eq!(health, json!({"reachable": true, "device": "fake gpu", "error": null}));
    let id = model(&app).await;
    let (status, classes) = call(&app, Method::GET, &format!("/api/models/{id}/classes"), None).await;
    assert_eq!((status, classes), (StatusCode::OK, json!(["crack", "dent", "smudge"])));

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
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
    let (p, images) = project(&app, 2).await;
    let m = model(&app).await;
    let status = run(&app, p, json!({"model": m, "image_ids": images})).await;
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
}

#[tokio::test]
async fn running_again_replaces_unreviewed_pre_annotations_and_keeps_the_rest() {
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
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
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
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
async fn a_run_over_a_job_can_be_cancelled_and_reports_progress() {
    let worker = FakeWorker { delay: Duration::from_millis(100), ..FakeWorker::default() };
    let seen = worker.seen.clone();
    let app = test_app_with_worker(Some(worker.start().await)).await;
    let (p, images) = project(&app, 6).await;
    let (_, jobs) = call(&app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": images}))).await;
    let m = model(&app).await;
    let (_, started) = call(&app, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": m, "job": jobs[0]["id"]}))).await;
    let id = started["id"].as_u64().unwrap();
    assert_eq!((&started["total"], &started["state"]), (&json!(6), &json!("running")));
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (status, _) = call(&app, Method::DELETE, &format!("/api/runs/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let status = wait(&app, id).await;
    assert_eq!(status["state"], json!("cancelled"));
    let done = status["done"].as_u64().unwrap();
    assert!((1..6).contains(&done), "{status}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(seen.load(Ordering::SeqCst) as u64 <= done + 1, "no more images go to the worker");
}

#[tokio::test]
async fn a_failing_worker_ends_the_run_with_its_message() {
    let app = test_app_with_worker(Some(FakeWorker { failing: true, ..FakeWorker::default() }.start().await)).await;
    let (p, images) = project(&app, 2).await;
    let m = model(&app).await;
    let status = run(&app, p, json!({"model": m, "image_ids": images})).await;
    assert_eq!(status["state"], json!("failed"));
    assert!(status["error"].as_str().unwrap().contains("out of GPU memory"), "{status}");
}

#[tokio::test]
async fn annotators_run_models_only_on_images_of_their_jobs() {
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
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
async fn an_image_removed_during_a_run_is_skipped_and_statistics_wait_for_review() {
    let worker = FakeWorker { delay: Duration::from_millis(150), ..FakeWorker::default() };
    let app = test_app_with_worker(Some(worker.start().await)).await;
    let (p, images) = project(&app, 3).await;
    let m = model(&app).await;
    let (_, started) = call(&app, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": m, "image_ids": images}))).await;
    call(&app, Method::POST, &format!("/api/projects/{p}/image-removals"), Some(json!({"ids": [images[2]]}))).await;
    let status = wait(&app, started["id"].as_u64().unwrap()).await;
    assert_eq!((&status["state"], &status["done"], &status["created"]), (&json!("finished"), &json!(2), &json!(4)), "{status}");
    assert!(status["skipped"].as_array().unwrap().iter().any(|s| s["reason"] == "images removed meanwhile"), "{status}");

    // Unreviewed pre-annotations do not count yet.
    let (_, statistics) = call(&app, Method::GET, &format!("/api/projects/{p}/statistics"), None).await;
    assert_eq!(statistics["labels"][0]["annotations"], json!(0), "{statistics}");
}

#[tokio::test]
async fn admins_set_where_the_worker_is_and_it_applies_at_once() {
    let app = test_app().await;
    let (_, settings) = call(&app, Method::GET, "/api/settings/worker", None).await;
    assert_eq!(settings, json!({"command": null, "url": null}));
    assert_eq!(call(&app, Method::GET, "/api/worker", None).await.1["reachable"], json!(false));

    let url = FakeWorker::default().start().await;
    let (status, settings) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"url": format!(" {url}/ ")}))).await;
    assert_eq!((status, &settings), (StatusCode::OK, &json!({"command": null, "url": url})));
    assert_eq!(call(&app, Method::GET, "/api/worker", None).await.1["device"], json!("fake gpu"));
    // A command without a URL listens on the default one.
    let (_, settings) = call(&app, Method::PUT, "/api/settings/worker", Some(json!({"command": "exit 0"}))).await;
    assert_eq!(settings, json!({"command": "exit 0", "url": "http://127.0.0.1:3112"}));
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

#[tokio::test]
async fn a_confidence_threshold_leaves_out_less_certain_predictions() {
    let app = test_app_with_worker(Some(FakeWorker::default().start().await)).await;
    let (p, images) = project(&app, 1).await;
    let m = model(&app).await;
    let status = run(&app, p, json!({"model": m, "image_ids": images, "threshold": 0.5})).await;
    assert_eq!(status["created"], json!(1), "{status}");
    assert!(status["skipped"].as_array().unwrap().iter().any(|s| s["reason"] == "less confident than 0.5" && s["count"] == 1), "{status}");
}
