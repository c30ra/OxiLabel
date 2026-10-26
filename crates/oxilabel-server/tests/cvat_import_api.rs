mod common;

use std::io::Write;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{TestApp, call, test_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Two images from the author's `defect_detection` project, renamed image1.jpg (2048 × 941) and
/// image2.jpg (2048 × 960): 35 and 30 shapes of 8 labels.
const DEFECT_DETECTION: &str =
    include_str!("../../oxilabel-formats/tests/fixtures/cvat/defect_detection.xml");

/// Sends a CVAT export (as `file_name`) and a plan to the project's import.
async fn import(
    app: &Router,
    project: i64,
    file_name: &str,
    file: &[u8],
    plan: Value,
) -> (StatusCode, Value) {
    let boundary = "oxilabel-cvat-boundary";
    let mut body = Vec::new();
    write!(body, "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").unwrap();
    body.extend_from_slice(file);
    write!(body, "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"plan\"\r\n\r\n{plan}\r\n--{boundary}--\r\n").unwrap();
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/projects/{project}/cvat-imports"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A project with images at `paths` (each `(path, width, height)`) and the given labels; returns
/// its id and the image ids in path order.
async fn project(app: &TestApp, paths: &[(&str, u32, u32)], labels: &[Value]) -> (i64, Vec<i64>) {
    let (_, project) = call(
        app,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "imported"})),
    )
    .await;
    let id = project["id"].as_i64().unwrap();
    for label in labels {
        let (status, body) = call(
            app,
            Method::POST,
            &format!("/api/projects/{id}/labels"),
            Some(label.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for (i, (path, width, height)) in paths.iter().enumerate() {
        let file = app.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(*width, *height, image::Rgb([i as u8, 9, 9]))
            .save(&file)
            .unwrap();
    }
    let paths: Vec<&str> = paths.iter().map(|(path, _, _)| *path).collect();
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/api/projects/{id}/imports"),
        Some(json!({"paths": paths})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, page) = call(
        app,
        Method::GET,
        &format!("/api/projects/{id}/images"),
        None,
    )
    .await;
    (
        id,
        page["images"]
            .as_array()
            .unwrap()
            .iter()
            .map(|image| image["id"].as_i64().unwrap())
            .collect(),
    )
}

const FIXTURE_IMAGES: [(&str, u32, u32); 2] =
    [("image1.jpg", 2048, 941), ("image2.jpg", 2048, 960)];

async fn annotations(app: &Router, project: i64, image: i64) -> Vec<Value> {
    let (_, body) = call(
        app,
        Method::GET,
        &format!("/api/projects/{project}/images/{image}/annotations"),
        None,
    )
    .await;
    body["annotations"].as_array().unwrap().clone()
}

async fn label_names(app: &Router, project: i64) -> Vec<String> {
    let (_, labels) = call(
        app,
        Method::GET,
        &format!("/api/projects/{project}/labels"),
        None,
    )
    .await;
    labels
        .as_array()
        .unwrap()
        .iter()
        .map(|label| label["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn preview_reports_the_shared_plan_without_writing() {
    let app = test_app().await;
    let crack = json!({"name": "Crack", "color": "#ff0000", "shape_types": ["polyline"]});
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[crack]).await;
    let (status, report) = import(
        &app,
        p,
        "annotations.xml",
        DEFECT_DETECTION.as_bytes(),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let choices: Vec<_> = report["labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|label| (label["name"].as_str().unwrap(), label["choice"].clone()))
        .collect();
    assert_eq!(choices[0], ("crack", json!({"action": "map", "label": 1})));
    assert!(
        choices[1..]
            .iter()
            .all(|(_, choice)| *choice == json!({"action": "create"})),
        "{choices:?}"
    );
    assert_eq!(report["labels"][2]["annotations"], json!(51));
    assert_eq!(
        (&report["images"], &report["matched"], &report["annotated"]),
        (&json!(2), &json!(2), &json!(0))
    );
    assert_eq!(report["annotations"], json!(65));
    assert_eq!(report["skipped"], json!([]));
    assert_eq!(report["committed"], json!(false));
    assert_eq!(label_names(&app, p).await, ["Crack"]);
    assert!(annotations(&app, p, images[0]).await.is_empty());
}

#[tokio::test]
async fn commit_applies_the_plan_and_the_annotation_store_rules() {
    let app = test_app().await;
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let (status, report) = import(
        &app,
        p,
        "annotations.xml",
        DEFECT_DETECTION.as_bytes(),
        json!({"commit": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(
        (&report["committed"], &report["annotations"]),
        (&json!(true), &json!(65))
    );
    assert_eq!(
        label_names(&app, p).await,
        [
            "crack",
            "line_interruption",
            "surface_defect",
            "shunt_top",
            "finger_delamination",
            "chip",
            "ignore",
            "shunt_middle"
        ]
    );
    let (_, labels) = call(
        &app,
        Method::GET,
        &format!("/api/projects/{p}/labels"),
        None,
    )
    .await;
    assert_eq!(
        (&labels[0]["color"], &labels[0]["shape_types"]),
        (&json!("#e6194b"), &json!(["polyline"]))
    );
    let first = annotations(&app, p, images[0]).await;
    let second = annotations(&app, p, images[1]).await;
    assert_eq!((first.len(), second.len()), (35, 30));
    let ellipse = first
        .iter()
        .find(|a| a["shape"]["type"] == "ellipse")
        .unwrap();
    assert_eq!(
        ellipse["shape"],
        json!({"type": "ellipse", "centre": {"x": 1613.55, "y": 839.95}, "rx": 23.25, "ry": 21.38, "angle": 0.0})
    );
    assert_eq!(ellipse["label"], json!(3));
    assert!(
        second
            .iter()
            .any(|a| a["shape"]["type"] == "oriented_rectangle")
    );
    assert!(
        first.iter().all(|a| a.get("source").is_none()),
        "imported as manual work"
    );

    // A second import appends, while replace writes the same planned set in place of the old one.
    let one_box = r#"<annotations><version>1.1</version><image id="0" name="image1.jpg" width="2048" height="941"><box label="chip" source="manual" occluded="0" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/></image></annotations>"#;
    let (_, report) = import(
        &app,
        p,
        "annotations.xml",
        one_box.as_bytes(),
        json!({"commit": true}),
    )
    .await;
    assert_eq!(report["annotations"], json!(1));
    assert_eq!(annotations(&app, p, images[0]).await.len(), 36);
    let (_, report) = import(
        &app,
        p,
        "annotations.xml",
        one_box.as_bytes(),
        json!({"replace": true, "commit": true}),
    )
    .await;
    assert_eq!(report["annotations"], json!(1));
    assert_eq!(annotations(&app, p, images[0]).await.len(), 1);
}
