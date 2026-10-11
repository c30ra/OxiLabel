mod common;

use std::{fs, io::Read, path::PathBuf};

use axum::http::{Method, StatusCode};
use common::{TestApp, call, get_bytes, test_app};
use serde_json::{Value, json};

/// A project with labels "crack" (1) and "finger" (2) and three 200 × 100 images; `a.png` has two
/// boxes, `b.png` one, `c.png` none. Returns (project, image ids by name).
async fn setup(app: &TestApp) -> (i64, Vec<i64>) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    let project = project["id"].as_i64().unwrap();
    for name in ["crack", "finger"] {
        call(app, Method::POST, &format!("/api/projects/{project}/labels"), Some(json!({"name": name, "color": "#ffffff"}))).await;
    }
    for (index, name) in ["a", "b", "c"].iter().enumerate() {
        let path = app.images.path().join(format!("set/{name}.png"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(200, 100, image::Rgb([index as u8, 0, 0])).save(&path).unwrap();
    }
    call(app, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": ["set"]}))).await;
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    let ids: Vec<i64> = page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect();
    let rect = |label: i64, x0: f64, y0: f64, x1: f64, y1: f64| {
        json!({"label": label, "shape": {"type": "rectangle", "min": {"x": x0, "y": y0}, "max": {"x": x1, "y": y1}}})
    };
    let boxes = [
        (ids[0], json!([rect(1, 20.0, 10.0, 60.0, 50.0), rect(2, 100.0, 0.0, 200.0, 100.0)])),
        (ids[1], json!([rect(1, 0.0, 0.0, 10.0, 10.0)])),
    ];
    for (image, annotations) in boxes {
        let url = format!("/api/projects/{project}/images/{image}/annotations");
        call(app, Method::PUT, &url, Some(json!({"base_version": 0, "annotations": annotations}))).await;
    }
    (project, ids)
}

async fn export(app: &TestApp, project: i64, request: Value) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/exports"), Some(request)).await
}

fn all_train(images: Value, include: &str) -> Value {
    json!({"format": "yolo-detect", "images": images, "include_images": include, "split": {"train": 100, "val": 0, "test": 0}})
}

/// The export folder in the app's export directory.
fn folder(app: &TestApp, result: &Value) -> PathBuf {
    let path = PathBuf::from(result["path"].as_str().unwrap());
    assert!(path.starts_with(app.exports.path()), "{path:?}");
    path
}

#[tokio::test]
async fn the_available_formats_are_listed() {
    let app = test_app().await;
    let (status, formats) = call(&app, Method::GET, "/api/exporters", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(formats, json!([{"id": "yolo-detect", "name": "YOLO detect", "image_modes": ["symlink", "copy"]}]));
}

#[tokio::test]
async fn exporting_all_images_writes_labels_data_yaml_and_a_manifest() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let (status, result) = export(&app, project, all_train(json!({"kind": "all"}), "symlink")).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!((result["image_count"].clone(), result["annotation_count"].clone()), (json!(3), json!(3)));
    assert_eq!(result["subsets"], json!({"train": 3, "val": 0, "test": 0}));

    let dir = folder(&app, &result);
    assert_eq!(
        fs::read_to_string(dir.join("labels/train/set__a.txt")).unwrap(),
        "0 0.200000 0.300000 0.200000 0.400000\n1 0.750000 0.500000 0.500000 1.000000\n"
    );
    assert_eq!(fs::read_to_string(dir.join("labels/train/set__c.txt")).unwrap(), "");
    assert!(fs::read_to_string(dir.join("data.yaml")).unwrap().contains("names:\n  0: crack\n  1: finger\n"));
    let manifest: Value = serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["project"], "Cells");
    assert_eq!(manifest["image_count"], 3);
    assert!(manifest["created"].as_str().unwrap().ends_with('Z'));
    // Every label file has its image at the matching path, as Ultralytics expects.
    assert!(dir.join("images/train/set__a.png").exists());
}

#[tokio::test]
async fn yolo_needs_the_images_and_links_them_by_default() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let (status, body) = export(&app, project, all_train(json!({"kind": "all"}), "none")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "YOLO detect needs the image files next to the labels: include them as links or copies");

    let request = json!({"format": "yolo-detect", "images": {"kind": "all"}, "split": {"train": 100, "val": 0, "test": 0}});
    let (status, result) = export(&app, project, request).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let link = folder(&app, &result).join("images/train/set__a.png");
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    let manifest: Value = serde_json::from_str(&fs::read_to_string(folder(&app, &result).join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["images"], "symlink");
}

#[tokio::test]
async fn the_export_can_be_limited_to_annotated_or_chosen_images() {
    let app = test_app().await;
    let (project, ids) = setup(&app).await;
    let (_, annotated) = export(&app, project, all_train(json!({"kind": "with_annotations"}), "symlink")).await;
    assert_eq!(annotated["image_count"], 2);
    assert!(!folder(&app, &annotated).join("labels/train/set__c.txt").exists());
    assert!(!folder(&app, &annotated).join("images/train/set__c.png").exists());

    let (_, chosen) = export(&app, project, all_train(json!({"kind": "ids", "ids": [ids[2]]}), "symlink")).await;
    assert_eq!((chosen["image_count"].clone(), chosen["annotation_count"].clone()), (json!(1), json!(0)));
    // Each export gets its own folder.
    assert_ne!(annotated["path"], chosen["path"]);
}

#[tokio::test]
async fn images_can_be_linked_or_copied_into_the_export() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let (_, linked) = export(&app, project, all_train(json!({"kind": "all"}), "symlink")).await;
    let link = folder(&app, &linked).join("images/train/set__a.png");
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&link).unwrap(), fs::read(app.images.path().join("set/a.png")).unwrap());

    let (_, copied) = export(&app, project, all_train(json!({"kind": "all"}), "copy")).await;
    let copy = folder(&app, &copied).join("images/train/set__b.png");
    assert!(fs::symlink_metadata(&copy).unwrap().file_type().is_file());
}

#[tokio::test]
async fn the_default_split_puts_images_in_train_and_val() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let (status, result) = export(&app, project, json!({"format": "yolo-detect", "images": {"kind": "all"}})).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let subsets = &result["subsets"];
    assert_eq!(subsets["train"].as_u64().unwrap() + subsets["val"].as_u64().unwrap(), 3);
    assert_eq!(subsets["test"], 0);
}

#[tokio::test]
async fn an_export_downloads_as_a_zip() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let (_, result) = export(&app, project, all_train(json!({"kind": "all"}), "symlink")).await;
    let (status, content_type, bytes) = get_bytes(&app, result["download"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/zip");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "data.yaml",
            "images/train/set__a.png",
            "images/train/set__b.png",
            "images/train/set__c.png",
            "labels/train/set__a.txt",
            "labels/train/set__b.txt",
            "labels/train/set__c.txt",
            "manifest.json"
        ]
    );
    // Linked images are stored in the zip as the real files.
    let mut image = Vec::new();
    zip.by_name("images/train/set__a.png").unwrap().read_to_end(&mut image).unwrap();
    assert_eq!(image, fs::read(app.images.path().join("set/a.png")).unwrap());
}

#[tokio::test]
async fn invalid_exports_are_rejected() {
    let app = test_app().await;
    let (project, _) = setup(&app).await;
    let cases = [
        (json!({"format": "nope", "images": {"kind": "all"}}), "unknown export format \"nope\""),
        (
            json!({"format": "yolo-detect", "images": {"kind": "all"}, "split": {"train": 80, "val": 30, "test": 0}}),
            "train, val and test must add up to 100% (they add up to 110%)",
        ),
        (json!({"format": "yolo-detect", "images": {"kind": "ids", "ids": [999]}}), "there are no images to export"),
    ];
    for (request, message) in cases {
        let (status, body) = export(&app, project, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{message}");
        assert_eq!(body["error"], message);
    }
    let (status, _) = export(&app, 999, json!({"format": "yolo-detect", "images": {"kind": "all"}})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get_bytes(&app, "/api/exports/..%2Fsecret/zip").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
