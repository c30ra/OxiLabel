use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use rtag_core::{Label, LabelDefinition};
use sqlx::{Row, SqliteConnection, SqlitePool, sqlite::SqliteRow};

use crate::error::ApiError;

pub fn routes() -> Router<SqlitePool> {
    Router::new()
        .route("/api/projects/{project}/labels", get(list_labels).post(create_label))
        .route("/api/projects/{project}/labels/{id}", put(update_label).delete(delete_label))
}

async fn create_label(
    State(pool): State<SqlitePool>,
    Path(project): Path<i64>,
    Json(definition): Json<LabelDefinition>,
) -> Result<(StatusCode, Json<Label>), ApiError> {
    let mut tx = pool.begin().await?;
    let others = project_labels(&mut tx, project).await?;
    let definition = definition.validated(&others).map_err(ApiError::BadRequest)?;
    // Ids come from a per-project counter, so a deleted label's id is never handed out again.
    let id: i64 = sqlx::query(
        "UPDATE projects SET next_label_id = next_label_id + 1 WHERE id = ? RETURNING next_label_id - 1 AS id",
    )
    .bind(project)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?
    .get("id");
    let label = Label::new(id, definition);
    sqlx::query(
        "INSERT INTO labels (project_id, id, name, color, hotkey, shape_types, is_ignore) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(label.id)
    .bind(&label.name)
    .bind(&label.color)
    .bind(&label.hotkey)
    .bind(serde_json::to_string(&label.shape_types).expect("shape types serialize"))
    .bind(label.ignore)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(label)))
}

async fn list_labels(State(pool): State<SqlitePool>, Path(project): Path<i64>) -> Result<Json<Vec<Label>>, ApiError> {
    let mut conn = pool.acquire().await?;
    Ok(Json(project_labels(&mut conn, project).await?))
}

/// All labels of a project, or `NotFound` if the project does not exist.
async fn project_labels(conn: &mut SqliteConnection, project: i64) -> Result<Vec<Label>, ApiError> {
    let exists = sqlx::query("SELECT 1 FROM projects WHERE id = ?")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?
        .is_some();
    if !exists {
        return Err(ApiError::NotFound);
    }
    let rows = sqlx::query("SELECT * FROM labels WHERE project_id = ? ORDER BY id")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(label_from_row).collect())
}

async fn update_label(
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
    Json(definition): Json<LabelDefinition>,
) -> Result<Json<Label>, ApiError> {
    let mut conn = pool.acquire().await?;
    let (current, others): (Vec<Label>, Vec<Label>) =
        project_labels(&mut conn, project).await?.into_iter().partition(|label| label.id == id);
    if current.is_empty() {
        return Err(ApiError::NotFound);
    }
    let label = Label::new(id, definition.validated(&others).map_err(ApiError::BadRequest)?);
    sqlx::query(
        "UPDATE labels SET name = ?, color = ?, hotkey = ?, shape_types = ?, is_ignore = ? WHERE project_id = ? AND id = ?",
    )
    .bind(&label.name)
    .bind(&label.color)
    .bind(&label.hotkey)
    .bind(serde_json::to_string(&label.shape_types).expect("shape types serialize"))
    .bind(label.ignore)
    .bind(project)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(Json(label))
}

async fn delete_label(
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let result = sqlx::query("DELETE FROM labels WHERE project_id = ? AND id = ?")
        .bind(project)
        .bind(id)
        .execute(&pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn label_from_row(row: &SqliteRow) -> Label {
    Label {
        id: row.get("id"),
        name: row.get("name"),
        color: row.get("color"),
        hotkey: row.get("hotkey"),
        shape_types: serde_json::from_str(row.get("shape_types")).unwrap_or_default(),
        ignore: row.get("is_ignore"),
    }
}
