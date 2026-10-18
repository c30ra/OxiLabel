use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use rtag_core::{NewProject, Project, ProjectSummary};
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{
    AppState,
    auth::{Admin, CurrentUser},
    error::ApiError,
    jobs,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", get(get_project).delete(delete_project))
}

async fn create_project(
    _: Admin,
    State(pool): State<SqlitePool>,
    Json(new): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project name must not be blank".into()));
    }
    let mut tx = pool.begin().await?;
    let id = sqlx::query("INSERT INTO projects (name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(&mut *tx)
        .await?
        .get::<i64, _>("id");
    if let Some(source) = new.copy_labels_from {
        copy_labels(&mut tx, source, id).await?;
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(Project { id, name: name.to_owned() })))
}

/// Gives project `target` the labels of `source` with the same ids, and the same next id, so ids
/// mean the same in both and a deleted id stays unused.
async fn copy_labels(conn: &mut SqliteConnection, source: i64, target: i64) -> Result<(), ApiError> {
    let next: i64 = sqlx::query("SELECT next_label_id FROM projects WHERE id = ?")
        .bind(source)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("there is no project {source} to copy labels from")))?
        .get("next_label_id");
    sqlx::query(
        "INSERT INTO labels (project_id, id, name, color, hotkey, shape_types, is_ignore, attributes, line_width, point_radius, point_shape, show_footprint, constraints) \
         SELECT ?, id, name, color, hotkey, shape_types, is_ignore, attributes, line_width, point_radius, point_shape, show_footprint, constraints \
         FROM labels WHERE project_id = ?",
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE projects SET next_label_id = ? WHERE id = ?").bind(next).bind(target).execute(&mut *conn).await?;
    Ok(())
}

/// Every project with its counts; an annotator gets only the projects with jobs of theirs, counting
/// only those jobs.
async fn list_projects(user: CurrentUser, State(pool): State<SqlitePool>) -> Result<Json<Vec<ProjectSummary>>, ApiError> {
    // ?1 is the annotator, or null for an admin.
    let rows = sqlx::query(
        "SELECT p.id, p.name, \
           (SELECT COUNT(*) FROM images i WHERE i.project_id = p.id AND (?1 IS NULL OR i.job_id IN (SELECT id FROM jobs WHERE assignee_id = ?1))) AS images, \
           (SELECT COUNT(*) FROM images i WHERE i.project_id = p.id AND annotated AND (?1 IS NULL OR i.job_id IN (SELECT id FROM jobs WHERE assignee_id = ?1))) AS annotated, \
           (SELECT COUNT(*) FROM jobs WHERE project_id = p.id AND (?1 IS NULL OR assignee_id = ?1)) AS jobs, \
           (SELECT json_group_array(color) FROM (SELECT color FROM labels WHERE project_id = p.id ORDER BY id)) AS colors \
         FROM projects p WHERE ?1 IS NULL OR p.id IN (SELECT project_id FROM jobs WHERE assignee_id = ?1) ORDER BY p.id",
    )
    .bind(user.job_owner())
    .fetch_all(&pool)
    .await?;
    Ok(Json(
        rows.iter()
            .map(|row| ProjectSummary {
                id: row.get("id"),
                name: row.get("name"),
                images: row.get("images"),
                annotated: row.get("annotated"),
                jobs: row.get("jobs"),
                label_colors: serde_json::from_str(row.get("colors")).unwrap_or_default(),
            })
            .collect(),
    ))
}

async fn get_project(user: CurrentUser, State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<Json<Project>, ApiError> {
    jobs::ensure_project_visible(&mut *pool.acquire().await?, &user, id).await?;
    let row = sqlx::query("SELECT id, name FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(Project { id: row.get("id"), name: row.get("name") }))
}

async fn delete_project(_: Admin, State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
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
