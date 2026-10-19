mod common;

use axum::http::{Method, StatusCode};
use common::{ADMIN, call, login, sign_in, test_app, with_cookie};
use serde_json::json;

#[tokio::test]
async fn the_session_names_the_signed_in_user() {
    let app = test_app().await;
    let (status, session) = call(&app, Method::GET, "/api/session", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["setup"], json!(false));
    assert_eq!(session["user"]["username"], json!("admin"));
    assert_eq!(session["user"]["role"], json!("admin"));
}

#[tokio::test]
async fn without_a_session_nobody_is_signed_in_and_api_requests_are_rejected() {
    let app = test_app().await;
    let (status, session) = call(&app.anonymous, Method::GET, "/api/session", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session, json!({"setup": false, "user": null}));
    for (method, uri) in [
        (Method::GET, "/api/projects"),
        (Method::POST, "/api/projects"),
        (Method::GET, "/api/images/1/thumbnail"),
        (Method::GET, "/api/users"),
        (Method::GET, "/api/exporters"),
    ] {
        let (status, _) = call(&app.anonymous, method.clone(), uri, Some(json!({"name": "x"}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn an_unknown_cookie_is_not_a_session() {
    let app = test_app().await;
    let forged = with_cookie(app.anonymous.clone(), "oxilabel_session=0123456789abcdef");
    let (status, _) = call(&forged, Method::GET, "/api/projects", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_first_account_can_be_created_only_once() {
    let app = test_app().await;
    let (status, cookie) = sign_in(&app.anonymous, "/api/setup", "intruder", "long enough password").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(cookie, None);
}

#[tokio::test]
async fn login_needs_the_right_password() {
    let app = test_app().await;
    let (status, cookie) = sign_in(&app.anonymous, "/api/login", ADMIN.0, "wrong password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cookie, None);
    let (status, _) = sign_in(&app.anonymous, "/api/login", "nobody", ADMIN.1).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn usernames_are_matched_case_insensitively() {
    let app = test_app().await;
    let (status, _) = sign_in(&app.anonymous, "/api/login", "ADMIN", ADMIN.1).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_logged_in_session_works_until_logout() {
    let app = test_app().await;
    let session = login(&app, ADMIN.0, ADMIN.1).await;
    assert_eq!(call(&session, Method::GET, "/api/projects", None).await.0, StatusCode::OK);
    assert_eq!(call(&session, Method::POST, "/api/logout", None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&session, Method::GET, "/api/projects", None).await.0, StatusCode::UNAUTHORIZED);
    // Other sessions of the same user stay signed in.
    assert_eq!(call(&app, Method::GET, "/api/projects", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn admins_create_users_who_can_log_in() {
    let app = test_app().await;
    let (status, user) = call(&app, Method::POST, "/api/users", Some(json!({"username": " anna ", "password": "anna's password", "role": "annotator"}))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(user["username"], json!("anna"));
    assert_eq!(user["role"], json!("annotator"));
    let (_, users) = call(&app, Method::GET, "/api/users", None).await;
    let names: Vec<_> = users.as_array().unwrap().iter().map(|u| (u["username"].clone(), u["role"].clone())).collect();
    assert_eq!(names, vec![(json!("admin"), json!("admin")), (json!("anna"), json!("annotator"))]);
    assert!(users[0].get("password_hash").is_none());

    let anna = login(&app, "anna", "anna's password").await;
    let (_, session) = call(&anna, Method::GET, "/api/session", None).await;
    assert_eq!(session["user"]["username"], json!("anna"));
}

#[tokio::test]
async fn new_users_need_a_unique_name_a_long_enough_password_and_a_known_role() {
    let app = test_app().await;
    for (body, expected) in [
        (json!({"username": "Admin", "password": "long enough password", "role": "annotator"}), StatusCode::CONFLICT),
        (json!({"username": "  ", "password": "long enough password", "role": "annotator"}), StatusCode::BAD_REQUEST),
        (json!({"username": "bob", "password": "short", "role": "annotator"}), StatusCode::BAD_REQUEST),
        (json!({"username": "bob", "password": "long enough password", "role": "boss"}), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let (status, _) = call(&app, Method::POST, "/api/users", Some(body.clone())).await;
        assert_eq!(status, expected, "{body}");
    }
}

#[tokio::test]
async fn a_password_reset_replaces_the_password_and_ends_that_users_sessions() {
    let app = test_app().await;
    let (_, anna) = call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "first password", "role": "annotator"}))).await;
    let old_session = login(&app, "anna", "first password").await;
    let uri = format!("/api/users/{}", anna["id"]);
    let (status, _) = call(&app, Method::PUT, &uri, Some(json!({"password": "second password"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(call(&old_session, Method::GET, "/api/projects", None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(sign_in(&app.anonymous, "/api/login", "anna", "first password").await.0, StatusCode::UNAUTHORIZED);
    let _ = login(&app, "anna", "second password").await;
}

#[tokio::test]
async fn resetting_ones_own_password_keeps_the_current_session() {
    let app = test_app().await;
    let (_, session) = call(&app, Method::GET, "/api/session", None).await;
    let uri = format!("/api/users/{}", session["user"]["id"]);
    assert_eq!(call(&app, Method::PUT, &uri, Some(json!({"password": "a new admin password"}))).await.0, StatusCode::OK);
    assert_eq!(call(&app, Method::GET, "/api/projects", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn roles_can_be_changed_but_the_last_admin_stays_admin() {
    let app = test_app().await;
    let (_, me) = call(&app, Method::GET, "/api/session", None).await;
    let me = format!("/api/users/{}", me["user"]["id"]);
    let (status, _) = call(&app, Method::PUT, &me, Some(json!({"role": "annotator"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (_, anna) = call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let (status, user) = call(&app, Method::PUT, &format!("/api/users/{}", anna["id"]), Some(json!({"role": "admin"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["role"], json!("admin"));
    // With another admin, the first may step down.
    assert_eq!(call(&app, Method::PUT, &me, Some(json!({"role": "annotator"}))).await.0, StatusCode::OK);
    assert_eq!(call(&app, Method::GET, "/api/users", None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn changing_an_unknown_user_is_not_found() {
    let app = test_app().await;
    let (status, _) = call(&app, Method::PUT, "/api/users/999", Some(json!({"role": "admin"}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn annotators_annotate_but_cannot_reach_admin_actions() {
    let app = test_app().await;
    let (_, project) = call(&app, Method::POST, "/api/projects", Some(json!({"name": "P"}))).await;
    let p = project["id"].as_i64().unwrap();
    call(&app, Method::POST, &format!("/api/projects/{p}/labels"), Some(json!({"name": "crack", "color": "#ff0000", "hotkey": "1", "shape_types": ["rectangle"]}))).await;
    std::fs::write(app.images.path().join("a.png"), common_png()).unwrap();
    call(&app, Method::POST, &format!("/api/projects/{p}/imports"), Some(json!({"paths": ["a.png"]}))).await;
    let (_, page) = call(&app, Method::GET, &format!("/api/projects/{p}/images"), None).await;
    let image = page["images"][0]["id"].as_i64().unwrap();

    let (_, user) = call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    // Annotators see the images of their jobs.
    call(&app, Method::POST, &format!("/api/projects/{p}/jobs"), Some(json!({"image_ids": [image], "assignee": user["id"]}))).await;

    let annotations = format!("/api/projects/{p}/images/{image}/annotations");
    for (method, uri, body) in [
        (Method::GET, "/api/projects".to_owned(), None),
        (Method::GET, format!("/api/projects/{p}"), None),
        (Method::GET, format!("/api/projects/{p}/labels"), None),
        (Method::GET, format!("/api/projects/{p}/images"), None),
        (Method::GET, annotations.clone(), None),
        (Method::PUT, annotations.clone(), Some(json!({"base_version": 0, "annotations": [{"label": 1, "shape": {"type": "rectangle", "min": {"x": 1.0, "y": 1.0}, "max": {"x": 5.0, "y": 5.0}}}]}))),
        (Method::PUT, format!("/api/projects/{p}/images/{image}/status"), Some(json!({"annotated": true}))),
    ] {
        let (status, body) = call(&anna, method.clone(), &uri, body).await;
        assert_eq!(status, StatusCode::OK, "{method} {uri}: {body}");
    }

    for (method, uri, body) in [
        (Method::POST, "/api/projects".to_owned(), Some(json!({"name": "Q"}))),
        (Method::DELETE, format!("/api/projects/{p}"), None),
        (Method::POST, format!("/api/projects/{p}/labels"), Some(json!({"name": "dent", "color": "#00ff00", "hotkey": "2", "shape_types": ["rectangle"]}))),
        (Method::PUT, format!("/api/projects/{p}/labels/1"), Some(json!({"name": "dent", "color": "#00ff00", "hotkey": "2", "shape_types": ["rectangle"]}))),
        (Method::DELETE, format!("/api/projects/{p}/labels/1"), None),
        (Method::POST, format!("/api/projects/{p}/imports"), Some(json!({"paths": ["a.png"]}))),
        (Method::POST, format!("/api/projects/{p}/image-removals"), Some(json!({"ids": [image]}))),
        (Method::POST, format!("/api/projects/{p}/uploads"), None),
        (Method::GET, "/api/browse".to_owned(), None),
        (Method::GET, "/api/browse/count?path=".to_owned(), None),
        (Method::POST, "/api/host/folder-dialog".to_owned(), None),
        (Method::GET, "/api/exports/0123abcd/zip".to_owned(), None),
        (Method::POST, format!("/api/projects/{p}/exports"), Some(json!({"exporter": "yolo-detect"}))),
        (Method::GET, "/api/exporters".to_owned(), None),
        (Method::GET, "/api/users".to_owned(), None),
        (Method::POST, "/api/users".to_owned(), Some(json!({"username": "eve", "password": "eve's password", "role": "admin"}))),
        (Method::PUT, "/api/users/1".to_owned(), Some(json!({"password": "taken over!"}))),
    ] {
        let (status, body) = call(&anna, method.clone(), &uri, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
    }
    // Nothing the refused requests asked for happened.
    let (_, labels) = call(&app, Method::GET, &format!("/api/projects/{p}/labels"), None).await;
    assert_eq!(labels.as_array().unwrap().len(), 1);
    let (_, users) = call(&app, Method::GET, "/api/users", None).await;
    assert_eq!(users.as_array().unwrap().len(), 2);
}

/// A tiny valid PNG.
fn common_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::new(8, 8).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}
