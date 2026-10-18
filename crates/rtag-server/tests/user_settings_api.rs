mod common;

use axum::http::{Method, StatusCode};
use common::{call, login, test_app};
use serde_json::json;

#[tokio::test]
async fn each_user_keeps_their_own_settings_across_sessions() {
    let app = test_app().await;
    let (status, settings) = call(&app, Method::GET, "/api/me/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings, json!({"shortcuts": {}, "flick_distance": 150.0, "label_style": "hotkeys", "theme": "dark"}));

    let mine = json!({"shortcuts": {"rectangle": [{"key": "w"}], "undo": [{"key": "u", "ctrl": true}]}, "flick_distance": 900, "label_style": "options_ring", "theme": "light"});
    let (status, saved) = call(&app, Method::PUT, "/api/me/settings", Some(mine)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["flick_distance"], json!(400.0), "kept within its range");

    // Another session of the same user sees them; another user has the defaults.
    let again = login(&app, "admin", "admin password").await;
    assert_eq!(call(&again, Method::GET, "/api/me/settings", None).await.1, saved);
    call(&app, Method::POST, "/api/users", Some(json!({"username": "anna", "password": "anna's password", "role": "annotator"}))).await;
    let anna = login(&app, "anna", "anna's password").await;
    assert_eq!(call(&anna, Method::GET, "/api/me/settings", None).await.1["theme"], json!("dark"));
    let (status, _) = call(&anna, Method::PUT, "/api/me/settings", Some(json!({"theme": "system"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(call(&app, Method::GET, "/api/me/settings", None).await.1["theme"], json!("light"));

    let (status, _) = call(&app, Method::PUT, "/api/me/settings", Some(json!({"shortcuts": {"undo": [{"key": " "}]}}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app.anonymous, Method::GET, "/api/me/settings", None).await.0, StatusCode::UNAUTHORIZED);
}
