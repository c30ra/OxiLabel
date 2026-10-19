mod common;

use std::path::Path;

use axum::http::{Method, StatusCode};
use common::{TestApp, call, get_bytes, test_app};
use serde_json::{Value, json};

/// Writes a solid-color PNG of the given size; `shade` makes otherwise equal images differ in content.
fn write_png(root: &Path, relative: &str, width: u32, height: u32, shade: u8) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(width, height, image::Rgb([shade, 0, 0])).save(&path).unwrap();
}

async fn new_project(app: &TestApp) -> i64 {
    let (_, project) = call(app, Method::POST, "/api/projects", Some(json!({"name": "Cells"}))).await;
    project["id"].as_i64().unwrap()
}

async fn import_selection(app: &TestApp, project: i64, paths: &[&str]) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/imports"), Some(json!({"paths": paths}))).await
}

async fn import(app: &TestApp, project: i64, folder: &str) -> (StatusCode, Value) {
    import_selection(app, project, &[folder]).await
}

async fn browse(app: &TestApp, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, &format!("/api/browse?path={path}"), None).await
}

async fn list(app: &TestApp, project: i64, query: &str) -> Value {
    let (status, page) = call(app, Method::GET, &format!("/api/projects/{project}/images{query}"), None).await;
    assert_eq!(status, StatusCode::OK);
    page
}

fn paths(page: &Value) -> Vec<&str> {
    page["images"].as_array().unwrap().iter().map(|image| image["path"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn importing_a_folder_adds_its_images_recursively() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "batch1/a.png", 40, 30, 1);
    write_png(app.images.path(), "batch1/nested/b.png", 20, 10, 2);
    write_png(app.images.path(), "other/c.png", 5, 5, 3);

    let (status, report) = import(&app, project, "batch1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"added": 2, "skipped": 0, "unsupported": 0, "failed": []}));

    let page = list(&app, project, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(paths(&page), vec!["batch1/a.png", "batch1/nested/b.png"]);
    let first = &page["images"][0];
    assert_eq!((first["width"].clone(), first["height"].clone()), (json!(40), json!(30)));
    assert_eq!(first["annotated"], false);
}

#[tokio::test]
async fn importing_again_skips_known_images_and_adds_new_ones() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 4, 4, 1);
    import(&app, project, "").await;
    write_png(app.images.path(), "b.png", 4, 4, 2);

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report, json!({"added": 1, "skipped": 1, "unsupported": 0, "failed": []}));
    assert_eq!(list(&app, project, "").await["total"], 2);
}

#[tokio::test]
async fn a_copy_of_a_known_image_under_another_name_is_skipped() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 4, 4, 1);
    write_png(app.images.path(), "copy/a-again.png", 4, 4, 1);

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report, json!({"added": 1, "skipped": 1, "unsupported": 0, "failed": []}));
}

#[tokio::test]
async fn unreadable_images_are_reported_and_other_files_counted() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "good.png", 4, 4, 1);
    std::fs::write(app.images.path().join("broken.jpg"), b"not really a jpeg").unwrap();
    std::fs::write(app.images.path().join("notes.txt"), b"hello").unwrap();

    let (_, report) = import(&app, project, "").await;
    assert_eq!(report["added"], 1);
    assert_eq!(report["unsupported"], 1);
    assert_eq!(report["failed"][0]["path"], "broken.jpg");
    assert!(report["failed"][0]["reason"].as_str().is_some_and(|reason| !reason.is_empty()));
    assert_eq!(paths(&list(&app, project, "").await), vec!["good.png"]);
}

#[tokio::test]
async fn paths_outside_the_image_root_are_refused() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for path in ["..", "a/../../etc"] {
        let (status, body) = import(&app, project, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["error"], "path must be inside the image root", "{path}");
        let (status, _) = browse(&app, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "browse {path}");
    }
    // An absolute path is a host folder: only for the server's own machine (see host_api.rs).
    let (status, _) = import(&app, project, "/etc").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_missing_path_is_reported() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, body) = import(&app, project, "nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "\"nope\" not found");
}

#[tokio::test]
async fn an_empty_selection_is_rejected() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, body) = import_selection(&app, project, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "select at least one folder or image");
}

#[tokio::test]
async fn a_selection_can_mix_single_images_and_folders() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "loose/one.png", 2, 2, 1);
    write_png(app.images.path(), "loose/two.png", 2, 2, 2);
    write_png(app.images.path(), "batch/three.png", 2, 2, 3);
    write_png(app.images.path(), "batch/deep/four.png", 2, 2, 4);

    let (status, report) = import_selection(&app, project, &["loose/two.png", "batch"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"added": 3, "skipped": 0, "unsupported": 0, "failed": []}));
    assert_eq!(
        paths(&list(&app, project, "").await),
        vec!["batch/deep/four.png", "batch/three.png", "loose/two.png"]
    );
}

#[tokio::test]
async fn an_image_selected_together_with_its_folder_is_counted_once() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "batch/a.png", 2, 2, 1);

    let (_, report) = import_selection(&app, project, &["batch", "batch/a.png"]).await;
    assert_eq!(report, json!({"added": 1, "skipped": 0, "unsupported": 0, "failed": []}));
}

#[tokio::test]
async fn browsing_lists_folders_first_then_images_and_hides_other_files() {
    let app = test_app().await;
    write_png(app.images.path(), "set/b.png", 2, 2, 1);
    write_png(app.images.path(), "set/A.png", 2, 2, 2);
    write_png(app.images.path(), "set/zeta/c.png", 2, 2, 3);
    std::fs::create_dir_all(app.images.path().join("set/alpha")).unwrap();
    std::fs::write(app.images.path().join("set/readme.txt"), b"hi").unwrap();
    std::fs::write(app.images.path().join("set/.hidden.png"), b"hi").unwrap();

    let (status, listing) = browse(&app, "set").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!({
            "path": "set",
            "entries": [
                {"name": "alpha", "path": "set/alpha", "kind": "folder"},
                {"name": "zeta", "path": "set/zeta", "kind": "folder"},
                {"name": "A.png", "path": "set/A.png", "kind": "image"},
                {"name": "b.png", "path": "set/b.png", "kind": "image"}
            ]
        })
    );
}

#[tokio::test]
async fn browsing_without_a_path_lists_the_image_root() {
    let app = test_app().await;
    write_png(app.images.path(), "set/b.png", 2, 2, 1);
    let (_, listing) = call(&app, Method::GET, "/api/browse", None).await;
    assert_eq!(listing, json!({"path": "", "entries": [{"name": "set", "path": "set", "kind": "folder"}]}));
}

#[tokio::test]
async fn images_are_listed_in_pages() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for i in 0..5u8 {
        write_png(app.images.path(), &format!("img{i}.png"), 2, 2, i);
    }
    import(&app, project, "").await;

    let page = list(&app, project, "?offset=2&limit=2").await;
    assert_eq!(page["total"], 5);
    assert_eq!(paths(&page), vec!["img2.png", "img3.png"]);
}

#[tokio::test]
async fn images_of_an_unknown_or_deleted_project_are_not_found() {
    let app = test_app().await;
    let (status, _) = import(&app, 999, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 2, 2, 1);
    import(&app, project, "").await;
    call(&app, Method::DELETE, &format!("/api/projects/{project}"), None).await;
    let (status, _) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn first_image_id(app: &TestApp, project: i64) -> i64 {
    list(app, project, "").await["images"][0]["id"].as_i64().unwrap()
}

async fn thumbnail_size(app: &TestApp, image: i64) -> (u32, u32) {
    let (status, content_type, bytes) = get_bytes(app, &format!("/api/images/{image}/thumbnail")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "image/jpeg");
    let thumbnail = image::load_from_memory(&bytes).unwrap();
    (thumbnail.width(), thumbnail.height())
}

#[tokio::test]
async fn thumbnails_fit_in_256_pixels_keeping_the_aspect_ratio() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "wide.png", 1024, 512, 1);
    import(&app, project, "").await;

    assert_eq!(thumbnail_size(&app, first_image_id(&app, project).await).await, (256, 128));
}

#[tokio::test]
async fn small_images_are_not_upscaled_for_thumbnails() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "small.png", 40, 30, 1);
    import(&app, project, "").await;

    assert_eq!(thumbnail_size(&app, first_image_id(&app, project).await).await, (40, 30));
}

#[tokio::test]
async fn thumbnails_are_cached_after_the_first_request() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 300, 300, 1);
    import(&app, project, "").await;
    let image = first_image_id(&app, project).await;
    thumbnail_size(&app, image).await;

    std::fs::remove_file(app.images.path().join("a.png")).unwrap();
    assert_eq!(thumbnail_size(&app, image).await, (256, 256));
}

#[tokio::test]
async fn thumbnail_of_an_unknown_image_is_not_found() {
    let app = test_app().await;
    let (status, _, _) = get_bytes(&app, "/api/images/999/thumbnail").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn browsing_for_a_project_marks_images_already_imported() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "set/old.png", 2, 2, 1);
    import(&app, project, "set/old.png").await;
    write_png(app.images.path(), "set/new.png", 2, 2, 2);

    let (_, listing) = call(&app, Method::GET, &format!("/api/browse?path=set&project={project}"), None).await;
    assert_eq!(
        listing["entries"],
        json!([
            {"name": "new.png", "path": "set/new.png", "kind": "image", "imported": false},
            {"name": "old.png", "path": "set/old.png", "kind": "image", "imported": true}
        ])
    );
}

async fn remove(app: &TestApp, project: i64, ids: &[i64]) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/projects/{project}/image-removals"), Some(json!({"ids": ids}))).await
}

fn ids(page: &Value) -> Vec<i64> {
    page["images"].as_array().unwrap().iter().map(|image| image["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn removing_images_takes_them_out_of_the_project_but_keeps_the_files() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for (index, name) in ["a", "b", "c"].iter().enumerate() {
        write_png(app.images.path(), &format!("set/{name}.png"), 2, 2, index as u8);
    }
    import(&app, project, "set").await;
    let all = ids(&list(&app, project, "").await);

    let (status, report) = remove(&app, project, &[all[0], all[2]]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"removed": 2}));

    let page = list(&app, project, "").await;
    assert_eq!(page["total"], 1);
    assert_eq!(paths(&page), vec!["set/b.png"]);
    assert!(app.images.path().join("set/a.png").exists());
}

#[tokio::test]
async fn removed_images_can_be_imported_again() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "set/a.png", 2, 2, 1);
    import(&app, project, "set").await;
    remove(&app, project, &ids(&list(&app, project, "").await)).await;

    let (_, listing) = call(&app, Method::GET, &format!("/api/browse?path=set&project={project}"), None).await;
    assert_eq!(listing["entries"][0]["imported"], false);
    let (_, report) = import(&app, project, "set").await;
    assert_eq!(report["added"], 1);
}

#[tokio::test]
async fn images_of_another_project_are_not_removed() {
    let app = test_app().await;
    let first = new_project(&app).await;
    let second = new_project(&app).await;
    write_png(app.images.path(), "set/a.png", 2, 2, 1);
    import(&app, first, "set").await;
    let theirs = ids(&list(&app, first, "").await);

    let (status, report) = remove(&app, second, &theirs).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report, json!({"removed": 0}));
    assert_eq!(list(&app, first, "").await["total"], 1);
}

#[tokio::test]
async fn removing_nothing_is_rejected_and_unknown_projects_are_not_found() {
    let app = test_app().await;
    let project = new_project(&app).await;
    let (status, body) = remove(&app, project, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "select at least one image");

    let (status, _) = remove(&app, 999, &[1]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Sets a file's modification time to midday UTC of the given day count since 1970-01-01.
fn set_modified_day(root: &Path, relative: &str, days: u64) {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(days * 86_400 + 43_200);
    std::fs::File::options().write(true).open(root.join(relative)).unwrap().set_modified(time).unwrap();
}

async fn filtered(app: &TestApp, project: i64, query: &str) -> Vec<String> {
    let page = list(app, project, &format!("?{query}")).await;
    let names: Vec<String> = paths(&page).into_iter().map(str::to_owned).collect();
    assert_eq!(page["total"], names.len());
    names
}

#[tokio::test]
async fn images_can_be_filtered_by_a_case_insensitive_path_regex() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "front/Crack_01.png", 2, 2, 1);
    write_png(app.images.path(), "front/ok_02.png", 2, 2, 2);
    write_png(app.images.path(), "back/crack_03.png", 2, 2, 3);
    import_selection(&app, project, &["front", "back"]).await;

    assert_eq!(filtered(&app, project, "name=crack").await, vec!["back/crack_03.png", "front/Crack_01.png"]);
    assert_eq!(filtered(&app, project, "name=%5Efront%2F.*_0%5B12%5D").await, vec!["front/Crack_01.png", "front/ok_02.png"]);

    let (status, body) = call(&app, Method::GET, &format!("/api/projects/{project}/images?name=%5B"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid name filter: unclosed character class");
}

#[tokio::test]
async fn images_can_be_filtered_by_dimensions() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "set/small.png", 10, 10, 1);
    write_png(app.images.path(), "set/wide.png", 100, 10, 2);
    write_png(app.images.path(), "set/big.png", 100, 100, 3);
    import(&app, project, "set").await;

    assert_eq!(filtered(&app, project, "min_width=50").await, vec!["set/big.png", "set/wide.png"]);
    assert_eq!(filtered(&app, project, "min_width=50&max_height=50").await, vec!["set/wide.png"]);
    assert_eq!(filtered(&app, project, "max_width=10&min_height=10").await, vec!["set/small.png"]);
}

#[tokio::test]
async fn images_record_file_size_and_date_and_can_be_filtered_by_them() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "set/old.png", 2, 2, 1);
    write_png(app.images.path(), "set/new.png", 400, 400, 2);
    set_modified_day(app.images.path(), "set/old.png", 19_000); // 2022-01-08
    set_modified_day(app.images.path(), "set/new.png", 20_000); // 2024-10-04
    import(&app, project, "set").await;

    let page = list(&app, project, "").await;
    let new = &page["images"][0];
    let size = std::fs::metadata(app.images.path().join("set/new.png")).unwrap().len();
    assert_eq!(new["size"], size);
    assert!(new["modified"].as_str().unwrap().starts_with("2024-10-0"), "{new}");

    assert_eq!(filtered(&app, project, &format!("min_size={size}")).await, vec!["set/new.png"]);
    assert_eq!(filtered(&app, project, &format!("max_size={}", size - 1)).await, vec!["set/old.png"]);
    assert_eq!(filtered(&app, project, "modified_from=2023-01-01").await, vec!["set/new.png"]);
    assert_eq!(filtered(&app, project, "modified_to=2023-01-01").await, vec!["set/old.png"]);
    assert_eq!(filtered(&app, project, "modified_from=2022-01-01&modified_to=2024-12-31").await.len(), 2);

    let (status, body) = call(&app, Method::GET, &format!("/api/projects/{project}/images?modified_from=yesterday"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "dates must look like 2024-12-31");
}

#[tokio::test]
async fn ids_of_all_matching_images_are_listed_across_pages() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for index in 0..5 {
        write_png(app.images.path(), &format!("set/{index}.png"), 2 + index, 2, index as u8);
    }
    import(&app, project, "set").await;
    let all = ids(&list(&app, project, "").await);

    let (status, body) = call(&app, Method::GET, &format!("/api/projects/{project}/image-ids"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(all));
    let (_, body) = call(&app, Method::GET, &format!("/api/projects/{project}/image-ids?min_width=5"), None).await;
    assert_eq!(body, json!(all[3..]));
}

#[tokio::test]
async fn a_folders_images_can_be_counted_before_importing() {
    let app = test_app().await;
    write_png(app.images.path(), "cells/a.png", 4, 4, 1);
    write_png(app.images.path(), "cells/deep/b.png", 4, 4, 2);
    std::fs::write(app.images.path().join("cells/notes.txt"), "not an image").unwrap();
    let (status, count) = call(&app, Method::GET, "/api/browse/count?path=cells", None).await;
    assert_eq!((status, count), (StatusCode::OK, json!({"images": 2})));
    let (status, _) = call(&app, Method::GET, "/api/browse/count?path=../outside", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}


#[tokio::test]
async fn an_images_status_can_be_set_and_shows_in_the_list() {
    let app = test_app().await;
    let project = new_project(&app).await;
    write_png(app.images.path(), "a.png", 4, 4, 1);
    import(&app, project, "a.png").await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    let id = page["images"][0]["id"].as_i64().unwrap();
    let status = format!("/api/projects/{project}/images/{id}/status");
    let (code, image) = call(&app, Method::PUT, &status, Some(json!({"annotated": true}))).await;
    assert_eq!((code, image["annotated"].clone()), (StatusCode::OK, json!(true)));
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    assert_eq!(page["images"][0]["annotated"], true);
    let (_, image) = call(&app, Method::PUT, &status, Some(json!({"annotated": false}))).await;
    assert_eq!(image["annotated"], false);
    let (code, _) = call(&app, Method::PUT, &format!("/api/projects/{project}/images/9999/status"), Some(json!({"annotated": true}))).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_images_neighbours_come_in_path_order_with_its_position() {
    let app = test_app().await;
    let project = new_project(&app).await;
    for (i, name) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        write_png(app.images.path(), &format!("set/{name}.png"), 4, 4, i as u8);
    }
    import(&app, project, "set").await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{project}/images"), None).await;
    let ids: Vec<i64> = page["images"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    let (code, around) = call(&app, Method::GET, &format!("/api/projects/{project}/images/{}/neighbours?count=2", ids[1]), None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!((around["position"].clone(), around["total"].clone()), (json!(2), json!(5)), "1-based position");
    let shown: Vec<i64> = around["images"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    assert_eq!(shown, ids[0..4], "up to two before and two after, the image itself in between");
}
