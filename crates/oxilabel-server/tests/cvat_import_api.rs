mod common;

use std::io::Write;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{TestApp, call, login, test_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Two images from the author's `defect_detection` project, renamed image1.jpg (2048 × 941) and
/// image2.jpg (2048 × 960): 35 and 30 shapes of 8 labels.
const DEFECT_DETECTION: &str = include_str!("../../oxilabel-formats/tests/fixtures/cvat/defect_detection.xml");

/// Sends a CVAT export (as `file_name`) and a plan to the project's import.
async fn import(app: &Router, project: i64, file_name: &str, file: &[u8], plan: Value) -> (StatusCode, Value) {
    let boundary = "oxilabel-cvat-boundary";
    let mut body = Vec::new();
    write!(body, "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").unwrap();
    body.extend_from_slice(file);
    write!(body, "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"plan\"\r\n\r\n{plan}\r\n--{boundary}--\r\n").unwrap();
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/projects/{project}/cvat-imports"))
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// A project with images at `paths` (each `(path, width, height)`) and the given labels; returns
/// its id and the image ids in path order.
async fn project(app: &TestApp, paths: &[(&str, u32, u32)], labels: &[Value]) -> (i64, Vec<i64>) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "imported"}))).await;
    let id = project["id"].as_i64().unwrap();
    for label in labels {
        let (status, body) = call(app, Method::POST, &format!("/api/projects/{id}/labels"), Some(label.clone())).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for (i, (path, width, height)) in paths.iter().enumerate() {
        let file = app.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(*width, *height, image::Rgb([i as u8, 9, 9])).save(&file).unwrap();
    }
    let paths: Vec<&str> = paths.iter().map(|(path, _, _)| *path).collect();
    let (status, body) = call(app, Method::POST, &format!("/api/projects/{id}/imports"), Some(json!({"paths": paths}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{id}/images"), None).await;
    (id, page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect())
}

const FIXTURE_IMAGES: [(&str, u32, u32); 2] = [("image1.jpg", 2048, 941), ("image2.jpg", 2048, 960)];

async fn annotations(app: &Router, project: i64, image: i64) -> Vec<Value> {
    let (_, body) = call(app, Method::GET, &format!("/api/projects/{project}/images/{image}/annotations"), None).await;
    body["annotations"].as_array().unwrap().clone()
}

async fn label_names(app: &Router, project: i64) -> Vec<String> {
    let (_, labels) = call(app, Method::GET, &format!("/api/projects/{project}/labels"), None).await;
    labels.as_array().unwrap().iter().map(|label| label["name"].as_str().unwrap().to_owned()).collect()
}

fn choices(report: &Value) -> Vec<(String, Value)> {
    report["labels"].as_array().unwrap().iter().map(|label| (label["name"].as_str().unwrap().to_owned(), label["choice"].clone())).collect()
}

#[tokio::test]
async fn a_preview_suggests_label_choices_and_counts_without_writing() {
    let app = test_app().await;
    let crack = json!({"name": "Crack", "color": "#ff0000", "shape_types": ["polyline"]});
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[crack]).await;
    let (status, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let choices = choices(&report);
    // The project's "Crack" takes CVAT's "crack"; the others would be created.
    assert_eq!(choices[0], ("crack".to_owned(), json!({"action": "map", "label": 1})));
    assert!(choices[1..].iter().all(|(_, choice)| *choice == json!({"action": "create"})), "{choices:?}");
    assert_eq!(report["labels"][2]["annotations"], json!(51), "surface_defect shapes");
    assert!(report["labels"].as_array().unwrap().iter().all(|label| label["problem"].is_null()), "{report}");
    assert_eq!((&report["images"], &report["matched"], &report["annotated"]), (&json!(2), &json!(2), &json!(0)));
    assert_eq!(report["annotations"], json!(65));
    assert_eq!(report["skipped"], json!([]));
    assert_eq!(report["committed"], json!(false));
    assert_eq!(label_names(&app, p).await, ["Crack"]);
    assert!(annotations(&app, p, images[0]).await.is_empty());
}

#[tokio::test]
async fn a_new_label_clashing_with_an_existing_one_is_named_and_refused() {
    let app = test_app().await;
    let (p, _) = project(&app, &FIXTURE_IMAGES, &[json!({"name": "chip", "color": "#00ff00"})]).await;
    let plan = json!({"labels": {"chip": {"action": "create"}}});
    let (_, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), plan.clone()).await;
    assert_eq!(report["labels"][5]["problem"], json!("a label named \"chip\" already exists"));
    let mut commit = plan;
    commit["commit"] = json!(true);
    let (status, body) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), commit).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("cannot create label \"chip\""), "{body}");
    assert_eq!(label_names(&app, p).await, ["chip"], "nothing was written");
}

#[tokio::test]
async fn committing_creates_the_labels_and_annotations() {
    let app = test_app().await;
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let (status, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"commit": true})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!((&report["committed"], &report["annotations"]), (&json!(true), &json!(65)));
    assert_eq!(label_names(&app, p).await, ["crack", "line_interruption", "surface_defect", "shunt_top", "finger_delamination", "chip", "ignore", "shunt_middle"]);
    let (_, labels) = call(&app, Method::GET, &format!("/api/projects/{p}/labels"), None).await;
    assert_eq!((&labels[0]["color"], &labels[0]["shape_types"]), (&json!("#e6194b"), &json!(["polyline"])));

    let first = annotations(&app, p, images[0]).await;
    let second = annotations(&app, p, images[1]).await;
    assert_eq!((first.len(), second.len()), (35, 30));
    let ellipse = first.iter().find(|a| a["shape"]["type"] == "ellipse").unwrap();
    assert_eq!(ellipse["shape"], json!({"type": "ellipse", "centre": {"x": 1613.55, "y": 839.95}, "rx": 23.25, "ry": 21.38, "angle": 0.0}));
    assert_eq!(ellipse["label"], json!(3));
    assert!(second.iter().any(|a| a["shape"]["type"] == "oriented_rectangle"));
    assert!(first.iter().all(|a| a.get("source").is_none()), "imported as manual work");
}

#[tokio::test]
async fn images_are_matched_by_file_name_and_size() {
    let app = test_app().await;
    let xml = |images: &str| format!(r#"<annotations><version>1.1</version>{images}</annotations>"#);
    let image = |name: &str, width: u32| format!(r#"<image id="0" name="{name}" width="{width}" height="20"><box label="x" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/></image>"#);
    let paths = [("a/one.png", 30, 20), ("b/two.png", 30, 20), ("c/two.png", 30, 20), ("three.png", 30, 20)];
    let (p, _) = project(&app, &paths, &[]).await;
    let export = xml(&[image("folder/one.png", 30), image("two.png", 30), image("three.png", 40), image("four.png", 30)].concat());
    let (_, report) = import(&app, p, "annotations.xml", export.as_bytes(), json!({})).await;
    assert_eq!(report["matched"], json!(1));
    assert_eq!(report["ambiguous"], json!(["two.png"]));
    assert_eq!(report["size_mismatch"], json!(["three.png"]));
    assert_eq!(report["unmatched"], json!(["four.png"]));
    assert_eq!(report["annotations"], json!(1));
}

#[tokio::test]
async fn labels_can_be_mapped_or_skipped_and_disallowed_shapes_are_left_out() {
    let app = test_app().await;
    let boxes_only = json!({"name": "boxes", "color": "#00ff00", "shape_types": ["rectangle"]});
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[boxes_only]).await;
    // surface_defect's 51 shapes (points, ellipses, masks, a polyline) go to a label allowing
    // only boxes;
    // chip's 2 boxes are skipped; everything else is skipped too.
    let mut labels = serde_json::Map::new();
    for name in ["crack", "line_interruption", "shunt_top", "finger_delamination", "ignore", "shunt_middle", "chip"] {
        labels.insert(name.into(), json!({"action": "skip"}));
    }
    labels.insert("surface_defect".into(), json!({"action": "map", "label": 1}));
    let (status, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"labels": labels, "commit": true})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["annotations"], json!(0));
    let skipped: Vec<(String, i64)> = report["skipped"].as_array().unwrap().iter().map(|s| (s["reason"].as_str().unwrap().to_owned(), s["count"].as_i64().unwrap())).collect();
    assert!(skipped.contains(&("label \"boxes\" does not allow points shapes".to_owned(), 39)), "{skipped:?}");
    assert!(skipped.contains(&("label \"boxes\" does not allow ellipse shapes".to_owned(), 6)), "{skipped:?}");
    assert!(skipped.contains(&("label \"chip\" is skipped".to_owned(), 2)), "{skipped:?}");
    assert_eq!(label_names(&app, p).await, ["boxes"]);
    assert!(annotations(&app, p, images[0]).await.is_empty());

    // Mapping to a label of another project is refused.
    let (status, _) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"labels": {"crack": {"action": "map", "label": 99}}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

const ONE_BOX: &str = r#"<annotations><version>1.1</version><image id="0" name="image1.jpg" width="2048" height="941"><box label="chip" source="manual" occluded="0" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/></image></annotations>"#;

#[tokio::test]
async fn existing_annotations_are_kept_or_replaced_and_the_preview_counts_them() {
    let app = test_app().await;
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[]).await;
    import(&app, p, "annotations.xml", ONE_BOX.as_bytes(), json!({"commit": true})).await;
    let (_, report) = import(&app, p, "annotations.xml", ONE_BOX.as_bytes(), json!({})).await;
    assert_eq!(report["annotated"], json!(1));
    import(&app, p, "annotations.xml", ONE_BOX.as_bytes(), json!({"commit": true})).await;
    assert_eq!(annotations(&app, p, images[0]).await.len(), 2, "added");
    let (_, report) = import(&app, p, "annotations.xml", ONE_BOX.as_bytes(), json!({"replace": true, "commit": true})).await;
    assert_eq!(report["annotations"], json!(1));
    assert_eq!(annotations(&app, p, images[0]).await.len(), 1, "replaced");
    // An editor that opened the image before is told it changed.
    let (_, current) = call(&app, Method::GET, &format!("/api/projects/{p}/images/{}/annotations", images[0]), None).await;
    assert_eq!(current["version"], json!(3));
}

const WITH_ATTRIBUTES: &str = r#"<annotations><version>1.1</version>
  <meta><task><labels>
    <label><name>defect</name><color>#ff0000</color><type>any</type><attributes>
      <attribute><name>occluded</name><input_type>checkbox</input_type><default_value>false</default_value><values>false
true</values></attribute>
      <attribute><name>severity</name><input_type>select</input_type><default_value>low</default_value><values>low
high</values></attribute>
      <attribute><name>length</name><input_type>number</input_type><default_value>2</default_value><values>0
10
1</values></attribute>
    </attributes></label>
    <label><name>good</name><color>#00ff00</color><type>tag</type><attributes></attributes></label>
  </labels></task></meta>
  <image id="0" name="image1.jpg" width="2048" height="941">
    <tag label="good" source="manual"></tag>
    <box label="defect" source="manual" occluded="0" xtl="10" ytl="10" xbr="20" ybr="20" z_order="0">
      <attribute name="occluded">true</attribute>
      <attribute name="severity">high</attribute>
      <attribute name="length">7.5</attribute>
      <attribute name="colour">red</attribute>
    </box>
  </image>
</annotations>"#;

#[tokio::test]
async fn attributes_and_tags_are_carried_over() {
    let app = test_app().await;
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let (status, report) = import(&app, p, "annotations.xml", WITH_ATTRIBUTES.as_bytes(), json!({"commit": true})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, labels) = call(&app, Method::GET, &format!("/api/projects/{p}/labels"), None).await;
    assert_eq!(labels[0]["attributes"], json!([
        {"name": "occluded", "type": "checkbox", "default": false},
        {"name": "severity", "type": "select", "options": ["low", "high"], "default": "low"},
        {"name": "length", "type": "number", "min": 0.0, "max": 10.0, "default": 2.0},
    ]));
    assert_eq!(labels[1]["shape_types"], json!(["tag"]));
    let list = annotations(&app, p, images[0]).await;
    assert_eq!(list[0]["shape"], json!({"type": "tag"}));
    assert_eq!(list[1]["attributes"], json!({"occluded": true, "severity": "high", "length": 7.5}));
    let skipped: Vec<_> = report["skipped"].as_array().unwrap().iter().map(|s| (s["reason"].clone(), s["count"].clone())).collect();
    assert_eq!(skipped, [(json!("attribute values with no matching attribute"), json!(1))]);
    // Adding again does not tag the image twice.
    import(&app, p, "annotations.xml", WITH_ATTRIBUTES.as_bytes(), json!({"commit": true})).await;
    assert_eq!(annotations(&app, p, images[0]).await.iter().filter(|a| a["shape"]["type"] == "tag").count(), 1);
}

#[tokio::test]
async fn a_zipped_export_is_read_and_unreadable_files_are_refused() {
    let app = test_app().await;
    let (p, _) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let mut zipped = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
        zip.start_file("annotations.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(DEFECT_DETECTION.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let (status, report) = import(&app, p, "export.zip", &zipped, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["annotations"], json!(65));
    let (status, body) = import(&app, p, "annotations.xml", b"<nonsense", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("XML"), "{body}");
}

#[tokio::test]
async fn annotators_cannot_import() {
    let app = test_app().await;
    let (p, _) = project(&app, &FIXTURE_IMAGES, &[]).await;
    call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    let (status, _) = import(&anna, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"commit": true})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn file_names_match_ignoring_case_and_same_named_images_are_told_apart_by_size() {
    let app = test_app().await;
    let paths = [("a/Photo.JPG", 30, 20), ("b/photo.jpg", 40, 20)];
    let (p, images) = project(&app, &paths, &[]).await;
    let export = r#"<annotations><version>1.1</version><image id="0" name="PHOTO.jpg" width="40" height="20"><box label="x" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/></image></annotations>"#;
    let (_, report) = import(&app, p, "annotations.xml", export.as_bytes(), json!({"commit": true})).await;
    assert_eq!((&report["matched"], &report["ambiguous"]), (&json!(1), &json!([])), "{report}");
    assert_eq!(annotations(&app, p, images[1]).await.len(), 1, "the 40 px one");
}

#[tokio::test]
async fn a_label_listed_twice_is_one_label() {
    let app = test_app().await;
    let (p, _) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let label = "<label><name>chip</name><color>#00ff00</color><type>any</type><attributes></attributes></label>";
    let export = format!("<annotations><version>1.1</version><meta><project><labels>{label}{label}</labels></project></meta>{}</annotations>", &ONE_BOX[ONE_BOX.find("<image").unwrap()..ONE_BOX.rfind("</annotations>").unwrap()]);
    let (status, report) = import(&app, p, "annotations.xml", export.as_bytes(), json!({"commit": true})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["labels"].as_array().unwrap().len(), 1);
    assert_eq!(label_names(&app, p).await, ["chip"]);
}

#[tokio::test]
async fn the_annotations_xml_of_a_zip_is_found_wherever_it_is() {
    let app = test_app().await;
    let (p, _) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let mut zipped = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("__MACOSX/export/._annotations.xml", options).unwrap();
        zip.write_all(b"junk").unwrap();
        zip.start_file("export/notes.xml", options).unwrap();
        zip.write_all(b"<notes/>").unwrap();
        zip.start_file("export/ANNOTATIONS.XML", options).unwrap();
        zip.write_all(ONE_BOX.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let (status, report) = import(&app, p, "export.zip", &zipped, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["annotations"], json!(1));
}

#[tokio::test]
async fn attribute_values_are_trimmed_and_left_out_shapes_count_once() {
    let app = test_app().await;
    let (p, images) = project(&app, &FIXTURE_IMAGES, &[]).await;
    let export = WITH_ATTRIBUTES
        .replace("<attribute name=\"severity\">high</attribute>", "<attribute name=\"severity\"> high\n</attribute>")
        .replace("<attribute name=\"colour\">red</attribute>", "")
        .replace("</image>", r#"<box label="defect" xtl="3000" ytl="1" xbr="3005" ybr="5" z_order="0"><attribute name="colour">red</attribute></box></image>"#);
    let (_, report) = import(&app, p, "annotations.xml", export.as_bytes(), json!({"commit": true})).await;
    let list = annotations(&app, p, images[0]).await;
    assert_eq!(list[1]["attributes"]["severity"], json!("high"));
    let reasons: Vec<_> = report["skipped"].as_array().unwrap().iter().map(|s| s["reason"].as_str().unwrap().to_owned()).collect();
    assert_eq!(reasons.len(), 1, "only the shape outside the image: {reasons:?}");
}

#[tokio::test]
async fn export_images_missing_from_the_project_are_added_from_a_folder() {
    let app = test_app().await;
    // image2.jpg is in the project; image1.jpg only in photos/, next to a resized copy.
    let (p, _) = project(&app, &[("image2.jpg", 2048, 960)], &[]).await;
    for (path, width, height) in [("photos/front/image1.jpg", 2048, 941), ("photos/small/image1.jpg", 512, 235)] {
        let file = app.images.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(width, height, image::Rgb([7, 7, 7])).save(&file).unwrap();
    }
    let (_, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({})).await;
    assert_eq!((&report["matched"], &report["unmatched"]), (&json!(1), &json!(["image1.jpg"])));

    let plan = json!({"images_folder": "photos"});
    let (status, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), plan).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!((&report["matched"], &report["added"], &report["unmatched"]), (&json!(2), &json!(1), &json!([])));
    assert_eq!(report["annotations"], json!(65));
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    assert_eq!(page["total"], json!(1), "the preview adds nothing");

    let (status, report) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"images_folder": "photos", "commit": true})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    let paths: Vec<_> = page["images"].as_array().unwrap().iter().map(|image| image["path"].as_str().unwrap().to_owned()).collect();
    assert_eq!(paths, ["image2.jpg", "photos/front/image1.jpg"]);
    let added = page["images"][1]["id"].as_i64().unwrap();
    assert_eq!(annotations(&app, p, added).await.len(), 35);

    // A folder outside the image root needs the server's own machine (ADR 0003).
    let (status, _) = import(&app, p, "annotations.xml", DEFECT_DETECTION.as_bytes(), json!({"images_folder": "/tmp"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
