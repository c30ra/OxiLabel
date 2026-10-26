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
    assert_eq!(load(&app, project, image).await, json!({"version": 0, "annotations": [], "annotated": false}));
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
    assert_eq!(load(&app, project, image).await, json!({"version": 3, "annotations": [], "annotated": false}));
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
    assert_eq!(load(&app, project, image).await, json!({"version": 0, "annotations": [], "annotated": false}));
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
    // Only saves change the status: an image emptied by deleting a label keeps it.
    assert_eq!(load(&app, project, image).await, json!({"version": 2, "annotations": [], "annotated": true}));
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

#[tokio::test]
async fn every_vector_shape_type_is_saved_and_reloaded() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    call(
        &app,
        Method::POST,
        &format!("/api/projects/{project}/labels"),
        Some(json!({"name": "any", "color": "#ffffff", "shape_types": ["polygon", "polyline", "points", "oriented_rectangle"]})),
    )
    .await;
    let shapes = json!([
        {"label": 3, "shape": {"type": "polygon", "points": [{"x": 10.0, "y": 10.0}, {"x": 90.0, "y": 15.0}, {"x": 40.0, "y": 80.0}]}},
        {"label": 3, "shape": {"type": "polyline", "points": [{"x": 5.0, "y": 5.0}, {"x": 50.0, "y": 5.0}, {"x": 50.0, "y": 60.0}]}},
        {"label": 3, "shape": {"type": "points", "points": [{"x": 1.0, "y": 2.0}, {"x": 3.5, "y": 4.25}]}},
        {"label": 3, "shape": {"type": "oriented_rectangle", "centre": {"x": 200.0, "y": 150.0}, "width": 80.0, "height": 30.0, "angle": 0.5}}
    ]);
    let (status, saved) = save(&app, project, image, 0, shapes.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(load(&app, project, image).await["annotations"], shapes);
}

#[tokio::test]
async fn labels_reject_shapes_they_do_not_allow_and_bad_new_shapes() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    // Label 2 ("crack") only allows polylines.
    let polygon = json!({"label": 2, "shape": {"type": "polygon", "points": [{"x": 0.0, "y": 0.0}, {"x": 9.0, "y": 0.0}, {"x": 0.0, "y": 9.0}]}});
    let (status, body) = save(&app, project, image, 0, json!([polygon])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "annotation 1: label \"crack\" does not allow polygon shapes");
    let short = json!({"label": 2, "shape": {"type": "polyline", "points": [{"x": 1.0, "y": 1.0}]}});
    let (_, body) = save(&app, project, image, 0, json!([short])).await;
    assert_eq!(body["error"], "annotation 1: a polyline needs at least two different points");
}

#[tokio::test]
async fn masks_are_stored_as_rle_and_reload_identically() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let labels = format!("/api/projects/{project}/labels");
    call(&app, Method::POST, &labels, Some(json!({"name": "stain", "color": "#9b59b6", "shape_types": ["mask"]}))).await;
    // A 4 × 3 box at the bottom-right corner of the 400 × 300 image.
    let mask = json!({"label": 3, "shape": {"type": "mask", "left": 396, "top": 297, "width": 4, "height": 3, "rle": [1, 3, 2, 2, 4]}});
    let (status, _) = save(&app, project, image, 0, json!([mask])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(load(&app, project, image).await["annotations"], json!([mask]));

    let outside = json!({"label": 3, "shape": {"type": "mask", "left": 397, "top": 297, "width": 4, "height": 3, "rle": [1, 3, 2, 2, 4]}});
    let (status, body) = save(&app, project, image, 1, json!([outside])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let short = json!({"label": 3, "shape": {"type": "mask", "left": 0, "top": 0, "width": 4, "height": 3, "rle": [1, 3]}});
    let (status, _) = save(&app, project, image, 1, json!([short])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = save(&app, project, image, 1, json!([{"label": 1, "shape": mask["shape"]}])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "\"defect\" does not allow masks");
}

#[tokio::test]
async fn attribute_values_are_checked_against_the_label_and_tags_need_a_label_that_allows_them() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let labels = format!("/api/projects/{project}/labels");
    let attributes = json!([
        {"name": "severity", "type": "select", "options": ["low", "high"], "default": "low"},
        {"name": "occluded", "type": "checkbox", "default": false}
    ]);
    call(&app, Method::POST, &labels, Some(json!({"name": "faulty", "color": "#9b59b6", "shape_types": ["rectangle", "tag"], "attributes": attributes}))).await;
    let with = |values: Value| {
        let mut annotation = rectangle(3, (1.0, 1.0), (5.0, 5.0));
        annotation["attributes"] = values;
        annotation
    };
    let tag = json!({"label": 3, "shape": {"type": "tag"}, "attributes": {"occluded": true}});
    let good = json!([with(json!({"severity": "high", "occluded": true})), tag]);
    let (status, body) = save(&app, project, image, 0, good.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(load(&app, project, image).await["annotations"], good);

    for (values, why) in [
        (json!({"colour": "red"}), "not an attribute of the label"),
        (json!({"severity": "medium"}), "not an option"),
        (json!({"occluded": "yes"}), "not a checkbox value"),
    ] {
        let (status, body) = save(&app, project, image, 1, json!([with(values)])).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
    }
    let (status, _) = save(&app, project, image, 1, json!([{"label": 1, "shape": {"type": "tag"}}])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "\"defect\" does not allow tags");
}

#[tokio::test]
async fn editing_a_labels_attributes_conforms_the_stored_values_and_bumps_the_version() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let labels = format!("/api/projects/{project}/labels");
    let definition = |attributes: Value| json!({"name": "faulty", "color": "#9b59b6", "shape_types": ["rectangle", "tag"], "attributes": attributes});
    let before = json!([
        {"name": "severity", "type": "select", "options": ["low", "medium", "high"], "default": "low"},
        {"name": "occluded", "type": "checkbox", "default": false}
    ]);
    call(&app, Method::POST, &labels, Some(definition(before))).await;
    let mut annotation = rectangle(3, (1.0, 1.0), (5.0, 5.0));
    annotation["attributes"] = json!({"severity": "medium", "occluded": true});
    save(&app, project, image, 0, json!([annotation])).await;

    // "occluded" goes, "medium" is no longer an option, and "note" is new.
    let after = json!([
        {"name": "severity", "type": "select", "options": ["low", "high"], "default": "high"},
        {"name": "note", "type": "text", "default": "-"}
    ]);
    let (status, _) = call(&app, Method::PUT, &format!("{labels}/3"), Some(definition(after))).await;
    assert_eq!(status, StatusCode::OK);
    let loaded = load(&app, project, image).await;
    assert_eq!(loaded["version"], 2, "editors holding version 1 must reload");
    assert_eq!(loaded["annotations"][0]["attributes"], json!({"severity": "high", "note": "-"}));
    let (status, body) = save(&app, project, image, 2, loaded["annotations"].clone()).await;
    assert_eq!(status, StatusCode::OK, "what was loaded saves again: {body}");
}

#[tokio::test]
async fn an_image_has_each_tag_once() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    call(&app, Method::POST, &format!("/api/projects/{project}/labels"), Some(json!({"name": "ok", "color": "#4fb286", "shape_types": ["tag"]}))).await;
    let tag = json!({"label": 3, "shape": {"type": "tag"}});
    let (status, body) = save(&app, project, image, 0, json!([tag, tag])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn hidden_and_locked_flags_are_saved_with_the_annotations() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let mut annotation = rectangle(1, (1.0, 1.0), (5.0, 5.0));
    annotation["hidden"] = json!(true);
    annotation["locked"] = json!(true);
    let plain = rectangle(1, (6.0, 6.0), (9.0, 9.0));
    save(&app, project, image, 0, json!([annotation, plain])).await;
    assert_eq!(load(&app, project, image).await["annotations"], json!([annotation, plain]));
}

#[tokio::test]
async fn saving_reviewed_work_marks_the_image_annotated_and_removing_it_unmarks() {
    let app = test_app().await;
    let (project, image) = setup(&app).await;
    let status = || async { call(&app, Method::GET, &format!("/api/projects/{project}/images/{image}"), None).await.1["annotated"].clone() };

    let mut pending = rectangle(1, (1.0, 1.0), (5.0, 5.0));
    pending["pending"] = json!(true);
    let (_, saved) = save(&app, project, image, 0, json!([pending.clone()])).await;
    assert_eq!((&saved["annotated"], status().await), (&json!(false), json!(false)), "pending pre-annotations do not count");

    let (_, saved) = save(&app, project, image, 1, json!([pending, rectangle(1, (6.0, 6.0), (9.0, 9.0))])).await;
    assert_eq!((&saved["annotated"], status().await), (&json!(true), json!(true)));
    assert_eq!(load(&app, project, image).await["annotated"], json!(true));

    let (_, saved) = save(&app, project, image, 2, json!([])).await;
    assert_eq!((&saved["annotated"], status().await), (&json!(false), json!(false)), "an emptied image is unmarked");
}
