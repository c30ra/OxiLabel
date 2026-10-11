use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use rtag_core::{NewProject, Project};
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{AppState, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", get(get_project).delete(delete_project))
}

async fn create_project(
    State(pool): State<SqlitePool>,
    Json(new): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project name must not be blank".into()));
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

async fn get_project(State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<Json<Project>, ApiError> {
    let row = sqlx::query("SELECT id, name FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(Project { id: row.get("id"), name: row.get("name") }))
}

async fn delete_project(State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let result = sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(&pool).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Fails with `NotFound` unless the project exists.
pub async fn ensure_exists(conn: &mut SqliteConnection, project: i64) -> Result<(), ApiError> {
    sqlx::query("SELECT 1 FROM projects WHERE id = ?")
        .bind(project)
        .fetch_optional(conn)
        .await?
        .map(|_| ())
        .ok_or(ApiError::NotFound)
}
