//! The access matrix: what an admin, an annotator with a job in the project and an annotator
//! without one get from every entry point that reads or edits a project's images. Whatever a
//! user cannot see is not found, never forbidden.

mod common;

use std::time::Duration;

use axum::{
    Router,
    http::{Method, StatusCode},
};
use common::{TestApp, call, get_bytes, login, test_app_with_worker};
use oxilabel_server::fake_worker::FakeWorker;
use serde_json::{Value, json};

/// What the matrix is run on: project `p` with images `a` (in anna's job) and `b` (in a job of
/// nobody's), its label, the model, and the jobs.
struct World {
    app: TestApp,
    p: i64,
    a: i64,
    b: i64,
    label: i64,
    model: i64,
    anna_job: i64,
    other_job: i64,
}

async fn world() -> World {
    let app = test_app_with_worker(Some(FakeWorker::default().serve().await)).await;
    let (_, project) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "p"}))).await;
    let p = project["id"].as_i64().unwrap();
    let (_, label) = call(&app, Method::POST, &format!("/api/projects/{p}/labels"), Some(json!({"name": "crack", "color": "#ff0000", "shape_types": ["rectangle"]}))).await;
    for (i, name) in ["a.png", "b.png"].iter().enumerate() {
        image::RgbImage::from_pixel(20, 10, image::Rgb([i as u8, 1, 2])).save(app.images.path().join(name)).unwrap();
    }
    let (status, _) = call(&app, Method::POST, &format!("/api/projects/{p}/imports"), Some(json!({"paths": ["a.png", "b.png"]}))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, ids) = call(&app, Method::GET, &format!("/api/projects/{p}/image-ids"), None).await;
    assert_eq!(status, StatusCode::OK, "{ids}");
    let (a, b) = (ids[0].as_i64().unwrap(), ids[1].as_i64().unwrap());
    let mut user_ids = Vec::new();
    for name in ["anna", "bob"] {
        let (_, user) = call(&app, Method::POST, "/api/users", Some(json!({"username": name, "password": format!("{name}'s password"), "role": "annotator"}))).await;
        user_ids.push(user["id"].clone());
    }
    let anna = &user_ids[0];
    let (_, anna_job) = call(&app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": [a], "assignee": anna}))).await;
    let (_, other_job) = call(&app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": [b]}))).await;
    let definition = json!({"name": "fake", "adapter": "ultralytics", "weights": "/models/fake.pt"});
    let (_, model) = call(&app, Method::POST, "/api/models", Some(definition)).await;
    World {
        p,
        a,
        b,
        label: label["id"].as_i64().unwrap(),
        model: model["id"].as_i64().unwrap(),
        anna_job: anna_job[0]["id"].as_i64().unwrap(),
        other_job: other_job[0]["id"].as_i64().unwrap(),
        app,
    }
}

/// The ids in a JSON array of objects, or of ids.
fn ids(list: &Value) -> Vec<i64> {
    list.as_array().unwrap().iter().map(|item| item.get("id").unwrap_or(item).as_i64().unwrap()).collect()
}

/// Starts a pre-annotation run; once it is accepted, waits until it ends, so it does not race the
/// edits that follow.
async fn start_run(user: &Router, w: &World, request: Value) -> StatusCode {
    let (status, run) = call(user, Method::POST, &format!("/api/projects/{}/pre-annotations", w.p), Some(request)).await;
    if status == StatusCode::ACCEPTED {
        wait_for_run(&w.app, run["id"].as_u64().unwrap()).await;
    }
    status
}

async fn wait_for_run(app: &Router, run: u64) {
    for _ in 0..200 {
        if call(app, Method::GET, &format!("/api/runs/{run}"), None).await.1["state"] != "running" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the run did not end");
}

/// The status of every image entry point for `image`, by name.
async fn image_routes(user: &Router, w: &World, image: i64) -> Vec<(&'static str, StatusCode)> {
    let p = w.p;
    let at = format!("/api/projects/{p}/images/{image}");
    // The current version, as the admin sees it, so a refused save is refused for access only.
    let version = call(&w.app, Method::GET, &format!("{at}/annotations"), None).await.1["version"].clone();
    vec![
        ("image details", call(user, Method::GET, &at, None).await.0),
        ("status", call(user, Method::PUT, &format!("{at}/status"), Some(json!({"annotated": false}))).await.0),
        ("neighbours", call(user, Method::GET, &format!("{at}/neighbours"), None).await.0),
        ("load annotations", call(user, Method::GET, &format!("{at}/annotations"), None).await.0),
        ("save annotations", call(user, Method::PUT, &format!("{at}/annotations"), Some(json!({"base_version": version, "annotations": []}))).await.0),
        ("thumbnail", get_bytes(user, &format!("/api/images/{image}/thumbnail")).await.0),
        ("tile", get_bytes(user, &format!("/api/images/{image}/tiles/0/0/0")).await.0),
        ("analysis tile", get_bytes(user, &format!("/api/images/{image}/analysis/0/0/0")).await.0),
        ("pre-annotation run", start_run(user, w, json!({"model": w.model, "image_ids": [image]})).await),
    ]
}

/// Every image entry point gives `expected` for `image`.
async fn assert_image_routes(who: &str, user: &Router, w: &World, image: i64, expected: StatusCode) {
    for (route, status) in image_routes(user, w, image).await {
        let expected = if expected == StatusCode::OK && route == "pre-annotation run" { StatusCode::ACCEPTED } else { expected };
        assert_eq!(status, expected, "{who}: {route} of image {image}");
    }
}

/// The project entry points as `user` sees them: each route's status and the ids it lists.
async fn project_routes(user: &Router, w: &World) -> Vec<(&'static str, StatusCode, Option<Vec<i64>>)> {
    let p = w.p;
    let listed = |(status, body): (StatusCode, Value)| (status, (status == StatusCode::OK).then(|| ids(&body)));
    let (projects_status, projects) = listed(call(user, Method::GET, "/api/projects", None).await);
    let (project, _) = call(user, Method::GET, &format!("/api/projects/{p}"), None).await;
    let (labels, label_ids) = listed(call(user, Method::GET, &format!("/api/projects/{p}/labels"), None).await);
    let (usage, _) = call(user, Method::GET, &format!("/api/projects/{p}/labels/{}/usage", w.label), None).await;
    let (jobs, job_ids) = listed(call(user, Method::GET, &format!("/api/projects/{p}/jobs"), None).await);
    let (images, page) = call(user, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    let image_ids = (images == StatusCode::OK).then(|| ids(&page["images"]));
    let (id_list, listed_ids) = listed(call(user, Method::GET, &format!("/api/projects/{p}/image-ids"), None).await);
    let (_, run) = call(&w.app, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": w.model, "image_ids": [w.a]}))).await;
    wait_for_run(&w.app, run["id"].as_u64().unwrap()).await;
    let (run_status, _) = call(user, Method::GET, &format!("/api/runs/{}", run["id"]), None).await;
    vec![
        ("projects", projects_status, projects),
        ("project", project, None),
        ("labels", labels, label_ids),
        ("label usage", usage, None),
        ("jobs", jobs, job_ids),
        ("images", images, image_ids),
        ("image ids", id_list, listed_ids),
        ("run status", run_status, None),
        ("run on own job", start_run(user, w, json!({"model": w.model, "job": w.anna_job})).await, None),
        ("run on other job", start_run(user, w, json!({"model": w.model, "job": w.other_job})).await, None),
    ]
}

#[tokio::test]
async fn admins_see_every_image_and_annotators_only_their_jobs_and_the_rest_is_not_found() {
    let w = world().await;
    let anna = login(&w.app, "anna", "anna's password").await;
    let bob = login(&w.app, "bob", "bob's password").await;
    let (ok, accepted, not_found) = (StatusCode::OK, StatusCode::ACCEPTED, StatusCode::NOT_FOUND);
    let (a, b, label) = (w.a, w.b, w.label);
    let (p, anna_job, other_job) = (w.p, w.anna_job, w.other_job);

    // Admin: everything.
    let admin: &Router = &w.app;
    assert_image_routes("admin", admin, &w, a, ok).await;
    assert_image_routes("admin", admin, &w, b, ok).await;
    assert_eq!(project_routes(admin, &w).await, [
        ("projects", ok, Some(vec![p])),
        ("project", ok, None),
        ("labels", ok, Some(vec![label])),
        ("label usage", ok, None),
        ("jobs", ok, Some(vec![anna_job, other_job])),
        ("images", ok, Some(vec![a, b])),
        ("image ids", ok, Some(vec![a, b])),
        ("run status", ok, None),
        ("run on own job", accepted, None),
        ("run on other job", accepted, None),
    ]);

    // Anna, with job 1: the project, her job and its image; the other image is not found.
    assert_image_routes("anna", &anna, &w, a, ok).await;
    assert_image_routes("anna", &anna, &w, b, not_found).await;
    assert_eq!(project_routes(&anna, &w).await, [
        ("projects", ok, Some(vec![p])),
        ("project", ok, None),
        ("labels", ok, Some(vec![label])),
        ("label usage", ok, None),
        ("jobs", ok, Some(vec![anna_job])),
        ("images", ok, Some(vec![a])),
        ("image ids", ok, Some(vec![a])),
        ("run status", ok, None),
        ("run on own job", accepted, None),
        ("run on other job", not_found, None),
    ]);

    // Bob, with no job in the project: nothing of it exists for him.
    assert_image_routes("bob", &bob, &w, a, not_found).await;
    assert_image_routes("bob", &bob, &w, b, not_found).await;
    assert_eq!(project_routes(&bob, &w).await, [
        ("projects", ok, Some(vec![])),
        ("project", not_found, None),
        ("labels", not_found, None),
        ("label usage", not_found, None),
        ("jobs", not_found, None),
        ("images", not_found, None),
        ("image ids", not_found, None),
        ("run status", not_found, None),
        ("run on own job", not_found, None),
        ("run on other job", not_found, None),
    ]);

    // A run over images the user may see, plus one they may not, is refused as a whole.
    let (status, _) = call(&anna, Method::POST, &format!("/api/projects/{p}/pre-annotations"), Some(json!({"model": w.model, "image_ids": [a, b]}))).await;
    assert_eq!(status, not_found);
}
