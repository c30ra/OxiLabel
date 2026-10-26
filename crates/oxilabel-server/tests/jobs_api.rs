mod common;

use axum::{
    Router,
    http::{Method, StatusCode},
};
use common::{TestApp, call, get_bytes, login, test_app};
use serde_json::{Value, json};

/// A project with images `a.png`, `b.png`, … (`count` of them); returns its id and the image ids
/// in path order.
async fn project_with_images(app: &TestApp, name: &str, count: usize) -> (i64, Vec<i64>) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": name}))).await;
    let id = project["id"].as_i64().unwrap();
    call(app, Method::POST, &format!("/api/projects/{id}/labels"), Some(json!({"name": "crack", "color": "#ff0000", "shape_types": ["rectangle", "tag"]}))).await;
    call(app, Method::POST, &format!("/api/projects/{id}/labels"), Some(json!({"name": "chip", "color": "#00ff00", "shape_types": ["rectangle"]}))).await;
    let paths: Vec<String> = (0..count).map(|i| format!("{name}/{}.png", (b'a' + i as u8) as char)).collect();
    for (i, path) in paths.iter().enumerate() {
        let file = app.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(20, 10, image::Rgb([i as u8, 0, 0])).save(file).unwrap();
    }
    let (status, _) = call(app, Method::POST, &format!("/api/projects/{id}/imports"), Some(json!({"paths": paths}))).await;
    assert_eq!(status, StatusCode::OK);
    (id, image_ids(app, id, "").await)
}

async fn image_ids(app: &Router, project: i64, query: &str) -> Vec<i64> {
    let (status, page) = call(app, Method::GET, &format!("/api/projects/{project}/images?{query}"), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect()
}

/// Creates an annotator and returns their id and a server signed in as them.
async fn annotator(app: &TestApp, name: &str) -> (i64, Router) {
    let password = format!("{name}'s password");
    let (_, user) = call(app, Method::POST, "/api/users", Some(json!({"username": name, "password": password, "role": "annotator"}))).await;
    (user["id"].as_i64().unwrap(), login(app, name, &password).await)
}

async fn create_jobs(app: &Router, project: i64, body: Value) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/jobs"), Some(body)).await
}

fn rectangle(label: i64) -> Value {
    json!({"label": label, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 5.0, "y": 5.0}}})
}

async fn save(app: &Router, project: i64, image: i64, annotations: Value) -> StatusCode {
    let uri = format!("/api/projects/{project}/images/{image}/annotations");
    let (_, current) = call(app, Method::GET, &uri, None).await;
    let version = current["version"].as_i64().unwrap_or(0);
    call(app, Method::PUT, &uri, Some(json!({"base_version": version, "annotations": annotations}))).await.0
}

/// Each job as (number, assignee's name, images).
fn summary(jobs: &Value) -> Vec<(i64, Option<String>, i64)> {
    jobs.as_array()
        .unwrap()
        .iter()
        .map(|job| (job["number"].as_i64().unwrap(), job["assignee"]["username"].as_str().map(str::to_owned), job["images"].as_i64().unwrap()))
        .collect()
}

#[tokio::test]
async fn a_job_is_created_from_images_and_listed_with_its_progress() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 3).await;
    let (status, created) = create_jobs(&app, p, json!({"image_ids": images})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(summary(&created), vec![(1, None, 3)]);
    assert_eq!(created[0]["annotated"], json!(0));
    assert_eq!(created[0]["annotations"], json!(0));
    let (_, jobs) = call(&app, Method::GET, &format!("/api/projects/{p}/jobs"), None).await;
    assert_eq!(jobs, created);
}

#[tokio::test]
async fn a_selection_is_split_into_jobs_of_n_images_in_path_order() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 5).await;
    let reversed: Vec<i64> = images.iter().rev().copied().collect();
    let (_, created) = create_jobs(&app, p, json!({"image_ids": reversed, "per_job": 2})).await;
    assert_eq!(summary(&created), vec![(1, None, 2), (2, None, 2), (3, None, 1)]);
    let first = created[0]["id"].as_i64().unwrap();
    assert_eq!(image_ids(&app, p, &format!("job={first}")).await, images[..2]);
}

#[tokio::test]
async fn images_already_in_a_job_move_to_the_new_one() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 3).await;
    create_jobs(&app, p, json!({"image_ids": [images[0], images[1]]})).await;
    create_jobs(&app, p, json!({"image_ids": [images[1], images[2]]})).await;
    let (_, jobs) = call(&app, Method::GET, &format!("/api/projects/{p}/jobs"), None).await;
    assert_eq!(summary(&jobs), vec![(1, None, 1), (2, None, 2)]);
}

#[tokio::test]
async fn a_job_needs_images_of_its_project_and_a_known_assignee() {
    let app = test_app().await;
    let (p, _) = project_with_images(&app, "p", 1).await;
    let (_, other) = project_with_images(&app, "q", 1).await;
    for body in [json!({"image_ids": []}), json!({"image_ids": other}), json!({"image_ids": other, "per_job": 0})] {
        let (status, _) = create_jobs(&app, p, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (_, mine) = call(&app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    let (status, _) = create_jobs(&app, p, json!({"image_ids": [mine["images"][0]["id"]], "assignee": 999})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, jobs) = call(&app, Method::GET, &format!("/api/projects/{p}/jobs"), None).await;
    assert_eq!(jobs, json!([]));
}

#[tokio::test]
async fn jobs_are_assigned_on_creation_and_can_be_reassigned_or_unassigned() {
    let app = test_app().await;
    let (anna, _) = annotator(&app, "anna").await;
    let (bob, _) = annotator(&app, "bob").await;
    let (p, images) = project_with_images(&app, "p", 2).await;
    let (_, created) = create_jobs(&app, p, json!({"image_ids": images, "per_job": 1, "assignee": anna})).await;
    assert_eq!(summary(&created), vec![(1, Some("anna".into()), 1), (2, Some("anna".into()), 1)]);

    let uri = format!("/api/projects/{p}/jobs/{}", created[1]["id"]);
    let (status, job) = call(&app, Method::PUT, &uri, Some(json!({"assignee": bob}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(job["assignee"]["username"], json!("bob"));
    let (_, job) = call(&app, Method::PUT, &uri, Some(json!({"assignee": null}))).await;
    assert_eq!(job["assignee"], json!(null));
    let (status, _) = call(&app, Method::PUT, &uri, Some(json!({"assignee": 999}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, Method::PUT, &format!("/api/projects/{p}/jobs/999"), Some(json!({"assignee": null}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_deleted_jobs_images_are_in_no_job_and_its_number_is_not_reused() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 3).await;
    let (_, created) = create_jobs(&app, p, json!({"image_ids": [images[0], images[1]]})).await;
    let uri = format!("/api/projects/{p}/jobs/{}", created[0]["id"]);
    assert_eq!(call(&app, Method::DELETE, &uri, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, Method::DELETE, &uri, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(image_ids(&app, p, "without_job=true").await, images);
    let (_, created) = create_jobs(&app, p, json!({"image_ids": [images[2]]})).await;
    assert_eq!(summary(&created), vec![(2, None, 1)]);
}

#[tokio::test]
async fn the_projects_list_counts_jobs() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 2).await;
    create_jobs(&app, p, json!({"image_ids": images, "per_job": 1})).await;
    let (_, projects) = call(&app, Method::GET, "/api/projects", None).await;
    assert_eq!(projects[0]["jobs"], json!(2));
}

#[tokio::test]
async fn annotators_see_only_their_jobs_and_the_projects_and_images_in_them() {
    let app = test_app().await;
    let (anna_id, anna) = annotator(&app, "anna").await;
    let (bob_id, _) = annotator(&app, "bob").await;
    let (p, images) = project_with_images(&app, "p", 5).await;
    let (q, _) = project_with_images(&app, "q", 1).await;
    // anna: a and c; bob: b; d and e in no job.
    create_jobs(&app, p, json!({"image_ids": [images[0], images[2]], "assignee": anna_id})).await;
    create_jobs(&app, p, json!({"image_ids": [images[1]], "assignee": bob_id})).await;
    call(&app, Method::PUT, &format!("/api/projects/{p}/images/{}/status", images[2]), Some(json!({"annotated": true}))).await;

    let (_, projects) = call(&anna, Method::GET, "/api/projects", None).await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert_eq!((projects[0]["id"].as_i64(), &projects[0]["images"], &projects[0]["annotated"], &projects[0]["jobs"]), (Some(p), &json!(2), &json!(1), &json!(1)));
    assert_eq!(call(&anna, Method::GET, &format!("/api/projects/{q}"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&anna, Method::GET, &format!("/api/projects/{q}/labels"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&anna, Method::GET, &format!("/api/projects/{p}"), None).await.0, StatusCode::OK);

    assert_eq!(summary(&call(&anna, Method::GET, &format!("/api/projects/{p}/jobs"), None).await.1), vec![(1, Some("anna".into()), 2)]);
    assert_eq!(image_ids(&anna, p, "").await, vec![images[0], images[2]]);
    let (_, ids) = call(&anna, Method::GET, &format!("/api/projects/{p}/image-ids"), None).await;
    assert_eq!(ids, json!([images[0], images[2]]));
    // The filmstrip steps from a to c.
    let (_, around) = call(&anna, Method::GET, &format!("/api/projects/{p}/images/{}/neighbours", images[2]), None).await;
    assert_eq!((around["position"].as_i64(), around["total"].as_i64()), (Some(2), Some(2)));
    let ids: Vec<_> = around["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![images[0], images[2]]);
}

#[tokio::test]
async fn annotators_can_only_open_and_edit_images_in_jobs_they_own_and_the_rest_is_not_found() {
    let app = test_app().await;
    let (anna_id, anna) = annotator(&app, "anna").await;
    let (bob_id, _) = annotator(&app, "bob").await;
    let (p, images) = project_with_images(&app, "p", 3).await;
    create_jobs(&app, p, json!({"image_ids": [images[0]], "assignee": anna_id})).await;
    let (_, bobs) = create_jobs(&app, p, json!({"image_ids": [images[1]], "assignee": bob_id})).await;

    assert_eq!(save(&anna, p, images[0], json!([rectangle(1)])).await, StatusCode::OK);
    for other in [images[1], images[2]] {
        assert_eq!(save(&anna, p, other, json!([rectangle(1)])).await, StatusCode::NOT_FOUND);
        let image = format!("/api/projects/{p}/images/{other}");
        assert_eq!(call(&anna, Method::PUT, &format!("{image}/status"), Some(json!({"annotated": true}))).await.0, StatusCode::NOT_FOUND);
        assert_eq!(call(&anna, Method::GET, &format!("{image}/annotations"), None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(call(&anna, Method::GET, &image, None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(get_bytes(&anna, &format!("/api/images/{other}/thumbnail")).await.0, StatusCode::NOT_FOUND);
        assert_eq!(get_bytes(&anna, &format!("/api/images/{other}/tiles/0/0/0")).await.0, StatusCode::NOT_FOUND);
    }
    // Admins edit everywhere.
    assert_eq!(save(&app, p, images[1], json!([rectangle(2)])).await, StatusCode::OK);

    // Reassigned to anna, bob's job becomes hers to edit.
    call(&app, Method::PUT, &format!("/api/projects/{p}/jobs/{}", bobs[0]["id"]), Some(json!({"assignee": anna_id}))).await;
    assert_eq!(save(&anna, p, images[1], json!([rectangle(1)])).await, StatusCode::OK);
}

#[tokio::test]
async fn annotators_cannot_create_change_or_delete_jobs_or_see_statistics() {
    let app = test_app().await;
    let (anna_id, anna) = annotator(&app, "anna").await;
    let (p, images) = project_with_images(&app, "p", 1).await;
    let (_, created) = create_jobs(&app, p, json!({"image_ids": images, "assignee": anna_id})).await;
    let job = format!("/api/projects/{p}/jobs/{}", created[0]["id"]);
    assert_eq!(create_jobs(&anna, p, json!({"image_ids": images})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&anna, Method::PUT, &job, Some(json!({"assignee": null}))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&anna, Method::DELETE, &job, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&anna, Method::GET, &format!("/api/projects/{p}/statistics"), None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn statistics_count_annotations_per_label_job_and_user() {
    let app = test_app().await;
    let (anna_id, _) = annotator(&app, "anna").await;
    let (p, images) = project_with_images(&app, "p", 4).await;
    // anna's job: a (2 cracks, annotated) and b (1 chip); an unassigned job: c (1 crack); d in no job.
    create_jobs(&app, p, json!({"image_ids": [images[0], images[1]], "assignee": anna_id})).await;
    create_jobs(&app, p, json!({"image_ids": [images[2]]})).await;
    save(&app, p, images[0], json!([rectangle(1), rectangle(1)])).await;
    save(&app, p, images[1], json!([rectangle(2)])).await;
    save(&app, p, images[2], json!([rectangle(1)])).await;
    // Saving marked all three; b and c are unmarked by hand.
    for image in [images[1], images[2]] {
        call(&app, Method::PUT, &format!("/api/projects/{p}/images/{image}/status"), Some(json!({"annotated": false}))).await;
    }

    let (status, statistics) = call(&app, Method::GET, &format!("/api/projects/{p}/statistics"), None).await;
    assert_eq!(status, StatusCode::OK);
    let labels: Vec<_> = statistics["labels"].as_array().unwrap().iter().map(|l| (l["name"].clone(), l["annotations"].clone(), l["images"].clone())).collect();
    assert_eq!(labels, vec![(json!("crack"), json!(3), json!(2)), (json!("chip"), json!(1), json!(1))]);
    let jobs: Vec<_> = statistics["jobs"].as_array().unwrap().iter().map(|j| (j["number"].clone(), j["images"].clone(), j["annotated"].clone(), j["annotations"].clone())).collect();
    assert_eq!(jobs, vec![(json!(1), json!(2), json!(1), json!(3)), (json!(2), json!(1), json!(0), json!(1))]);
    let users: Vec<_> = statistics["users"].as_array().unwrap().iter().map(|u| (u["user"]["username"].clone(), u["images"].clone(), u["annotated"].clone(), u["annotations"].clone())).collect();
    assert_eq!(users, vec![(json!("anna"), json!(2), json!(1), json!(3)), (json!(null), json!(2), json!(0), json!(1))]);
}

#[tokio::test]
async fn a_job_can_be_made_from_more_images_than_sqlite_binds_at_once() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 2).await;
    // Ids of no image pad the selection past SQLite's limit of 32766 bound values.
    let mut ids: Vec<i64> = (1_000_000..1_040_000).collect();
    ids.extend(&images);
    let (status, created) = create_jobs(&app, p, json!({"image_ids": ids})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(summary(&created), vec![(1, None, 2)]);
}

#[tokio::test]
async fn images_can_be_filtered_by_status() {
    let app = test_app().await;
    let (p, images) = project_with_images(&app, "p", 3).await;
    call(&app, Method::PUT, &format!("/api/projects/{p}/images/{}/status", images[1]), Some(json!({"annotated": true}))).await;
    assert_eq!(image_ids(&app, p, "annotated=false").await, vec![images[0], images[2]]);
    assert_eq!(image_ids(&app, p, "annotated=true").await, vec![images[1]]);
}
