mod common;

use std::path::Path;

use axum::{
    Router,
    http::{Method, StatusCode},
};
use common::{TestApp, call, get_bytes, test_app};
use serde_json::json;

/// Writes an image whose left half is red and right half blue.
fn write_halves(root: &Path, relative: &str, width: u32, height: u32) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(width, height, |x, _| if x < width / 2 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) })
        .save(&path)
        .unwrap();
}

/// A project with one imported 1200 × 700 image; returns (project, image id).
async fn one_image(app: &TestApp) -> (i64, i64) {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Tiles"}))).await;
    let project = project["id"].as_i64().unwrap();
    write_halves(app.images.path(), "big.png", 1200, 700);
    call(app, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": ["big.png"]}))).await;
    let (_, page) = call(app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    (project, page["images"][0]["id"].as_i64().unwrap())
}

async fn tile(app: &Router, image: i64, level: u32, x: u32, y: u32) -> (StatusCode, Option<image::RgbImage>) {
    let (status, content_type, bytes) = get_bytes(app, &format!("/api/images/{image}/tiles/{level}/{x}/{y}")).await;
    if status != StatusCode::OK {
        return (status, None);
    }
    assert_eq!(content_type, "image/jpeg");
    (status, Some(image::load_from_memory(&bytes).unwrap().to_rgb8()))
}

fn is_red(pixel: &image::Rgb<u8>) -> bool {
    pixel[0] > 200 && pixel[2] < 60
}

fn is_blue(pixel: &image::Rgb<u8>) -> bool {
    pixel[2] > 200 && pixel[0] < 60
}

#[tokio::test]
async fn an_image_can_be_fetched_by_id_within_its_project() {
    let app = test_app().await;
    let (project, image) = one_image(&app).await;
    let (status, body) = call(&app, Method::GET, &format!("/api/projects/{project}/images/{image}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((body["path"].clone(), body["width"].clone(), body["height"].clone()), (json!("big.png"), json!(1200), json!(700)));

    let (_, other) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "Other"}))).await;
    let (status, _) = call(&app, Method::GET, &format!("/api/projects/{}/images/{image}", other["id"]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn full_resolution_tiles_are_512_pixels_and_cropped_at_the_edges() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;

    let (status, first) = tile(&app, image, 0, 0, 0).await;
    assert_eq!(status, StatusCode::OK);
    let first = first.unwrap();
    assert_eq!(first.dimensions(), (512, 512));
    assert!(is_red(first.get_pixel(10, 10)));

    // Columns: 0..512, 512..1024, 1024..1200; rows: 0..512, 512..700.
    let (_, corner) = tile(&app, image, 0, 2, 1).await;
    let corner = corner.unwrap();
    assert_eq!(corner.dimensions(), (176, 188));
    assert!(is_blue(corner.get_pixel(100, 100)));
}

#[tokio::test]
async fn coarser_levels_halve_the_image_down_to_a_single_tile() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;
    // 1200×700 → 600×350 → 300×175, which fits in one tile.
    let (_, level1) = tile(&app, image, 1, 1, 0).await;
    assert_eq!(level1.unwrap().dimensions(), (88, 350));
    let (_, whole) = tile(&app, image, 2, 0, 0).await;
    let whole = whole.unwrap();
    assert_eq!(whole.dimensions(), (300, 175));
    assert!(is_red(whole.get_pixel(20, 80)) && is_blue(whole.get_pixel(280, 80)));
}

#[tokio::test]
async fn the_whole_pyramid_is_cached_after_the_first_tile() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;
    let (status, _) = tile(&app, image, 2, 0, 0).await;
    assert_eq!(status, StatusCode::OK);

    // With the source gone, every tile must come from the cache.
    std::fs::remove_file(app.images.path().join("big.png")).unwrap();
    for (level, x, y) in [(0, 0, 0), (0, 2, 1), (1, 1, 0), (2, 0, 0)] {
        let (status, _) = tile(&app, image, level, x, y).await;
        assert_eq!(status, StatusCode::OK, "tile {level}/{x}/{y}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_requests_all_get_their_tile() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;
    let mut requests = tokio::task::JoinSet::new();
    for (level, x, y) in [(0, 0, 0), (0, 1, 0), (0, 2, 0), (0, 0, 1), (0, 1, 1), (0, 2, 1), (1, 0, 0), (1, 1, 0), (2, 0, 0)] {
        let router = Router::clone(&app);
        requests.spawn(async move { tile(&router, image, level, x, y).await });
    }
    while let Some(result) = requests.join_next().await {
        let (status, tile) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(tile.is_some());
    }
}

#[tokio::test]
async fn tiles_outside_the_pyramid_are_not_found() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;
    for (level, x, y) in [(3, 0, 0), (0, 3, 0), (0, 0, 2), (2, 1, 0)] {
        let (status, _) = tile(&app, image, level, x, y).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "tile {level}/{x}/{y}");
    }
    let (status, _) = tile(&app, 999, 0, 0, 0).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_missing_source_file_is_not_found() {
    let app = test_app().await;
    let (_, image) = one_image(&app).await;
    std::fs::remove_file(app.images.path().join("big.png")).unwrap();
    let (status, _) = tile(&app, image, 0, 0, 0).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
