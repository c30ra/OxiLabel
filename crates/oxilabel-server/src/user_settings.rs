//! Each user's own settings (shortcuts, pie sensitivity, label style, theme), stored on the server
//! so they follow the user from machine to machine.

use axum::{Json, Router, extract::State, routing::get};
use oxilabel_core::settings::UserSettings;
use sqlx::SqlitePool;

use crate::{AppState, auth::CurrentUser, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/me/settings", get(my_settings).put(change_my_settings))
}

/// The user's settings; the defaults until they change any (or if what is stored no longer reads).
async fn my_settings(user: CurrentUser, State(pool): State<SqlitePool>) -> Result<Json<UserSettings>, ApiError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM user_settings WHERE user_id = ?").bind(user.user.id).fetch_optional(&pool).await?;
    Ok(Json(value.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or_default()))
}

async fn change_my_settings(user: CurrentUser, State(pool): State<SqlitePool>, Json(settings): Json<UserSettings>) -> Result<Json<UserSettings>, ApiError> {
    let settings = settings.validated().map_err(ApiError::BadRequest)?;
    sqlx::query("INSERT INTO user_settings (user_id, value) VALUES (?, ?) ON CONFLICT (user_id) DO UPDATE SET value = excluded.value")
        .bind(user.user.id)
        .bind(serde_json::to_string(&settings).expect("settings serialise"))
        .execute(&pool)
        .await?;
    Ok(Json(settings))
}
