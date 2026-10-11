use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use rtag_core::{NewProject, Project};
use serde_json::json;
use std::{str::FromStr, time::Duration};

use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

pub async fn app(database_url: &str) -> Result<Router, sqlx::Error> {
    let mut options = SqlitePoolOptions::new();
    if database_url.contains(":memory:") {
        // Each connection to `sqlite::memory:` is a separate database, so tests must share one.
        options = options.max_connections(1);
    }
    let connect = SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = options.connect_with(connect).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", delete(delete_project))
        .with_state(pool))
}

enum ApiError {
    BadRequest(&'static str),
    NotFound,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Database(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            ApiError::Database(err) => {
                eprintln!("database error: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

async fn create_project(
    State(pool): State<SqlitePool>,
    Json(new): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project name must not be blank"));
    }
    let id = sqlx::query("INSERT INTO projects (name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(&pool)
        .await?
        .get::<i64, _>("id");
    Ok((StatusCode::CREATED, Json(Project { id, name: name.to_owned() })))
}

async fn list_projects(State(pool): State<SqlitePool>) -> Result<Json<Vec<Project>>, ApiError> {
    let rows = sqlx::query("SELECT id, name FROM projects ORDER BY id").fetch_all(&pool).await?;
    Ok(Json(
        rows.iter()
            .map(|row| Project { id: row.get("id"), name: row.get("name") })
            .collect(),
    ))
}

async fn delete_project(State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let result = sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(&pool).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
