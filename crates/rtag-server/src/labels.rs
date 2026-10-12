use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, put},
};
use rtag_core::{Label, LabelDefinition, annotation::LabelUsage, attribute::Attributes};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection, SqlitePool, sqlite::SqliteRow};

use crate::{AppState, error::ApiError, projects};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{project}/labels", get(list_labels).post(create_label))
        .route("/api/projects/{project}/labels/{id}", put(update_label).delete(delete_label))
        .route("/api/projects/{project}/labels/{id}/usage", get(label_usage))
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
        "INSERT INTO labels (project_id, id, name, color, hotkey, shape_types, is_ignore, attributes) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(label.id)
    .bind(&label.name)
    .bind(&label.color)
    .bind(&label.hotkey)
    .bind(serde_json::to_string(&label.shape_types).expect("shape types serialize"))
    .bind(label.ignore)
    .bind(serde_json::to_string(&label.attributes).expect("attributes serialize"))
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
pub(crate) async fn project_labels(conn: &mut SqliteConnection, project: i64) -> Result<Vec<Label>, ApiError> {
    projects::ensure_exists(conn, project).await?;
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
    let mut tx = pool.begin().await?;
    let (current, others): (Vec<Label>, Vec<Label>) =
        project_labels(&mut tx, project).await?.into_iter().partition(|label| label.id == id);
    if current.is_empty() {
        return Err(ApiError::NotFound);
    }
    let label = Label::new(id, definition.validated(&others).map_err(ApiError::BadRequest)?);
    sqlx::query(
        "UPDATE labels SET name = ?, color = ?, hotkey = ?, shape_types = ?, is_ignore = ?, attributes = ? WHERE project_id = ? AND id = ?",
    )
    .bind(&label.name)
    .bind(&label.color)
    .bind(&label.hotkey)
    .bind(serde_json::to_string(&label.shape_types).expect("shape types serialize"))
    .bind(label.ignore)
    .bind(serde_json::to_string(&label.attributes).expect("attributes serialize"))
    .bind(project)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    conform_attribute_values(&mut tx, project, &label).await?;
    tx.commit().await?;
    Ok(Json(label))
}

/// Makes the stored attribute values of `label`'s annotations fit its (changed) definitions:
/// values of removed attributes go, values that no longer fit and missing ones become defaults.
/// Images whose annotations change get a new version, so editors holding the old one reload.
async fn conform_attribute_values(conn: &mut SqliteConnection, project: i64, label: &Label) -> Result<(), ApiError> {
    let rows = sqlx::query("SELECT id, image_id, attributes FROM annotations WHERE project_id = ? AND label_id = ?")
        .bind(project)
        .bind(label.id)
        .fetch_all(&mut *conn)
        .await?;
    let mut changed_images = std::collections::BTreeSet::new();
    for row in rows {
        let stored: Attributes = serde_json::from_str(row.get("attributes")).unwrap_or_default();
        let conformed = label.conform_attributes(&stored);
        if conformed != stored {
            sqlx::query("UPDATE annotations SET attributes = ? WHERE id = ?")
                .bind(serde_json::to_string(&conformed).expect("attributes serialize"))
                .bind(row.get::<i64, _>("id"))
                .execute(&mut *conn)
                .await?;
            changed_images.insert(row.get::<i64, _>("image_id"));
        }
    }
    for image in changed_images {
        sqlx::query("UPDATE images SET version = version + 1 WHERE id = ?").bind(image).execute(&mut *conn).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    confirm: bool,
}

/// Deletes a label. If annotations use it they are deleted too, but only with `?confirm=true`.
async fn delete_label(
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, ApiError> {
    let mut tx = pool.begin().await?;
    let name: String = sqlx::query("SELECT name FROM labels WHERE project_id = ? AND id = ?")
        .bind(project)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?
        .get("name");
    let used = count_annotations(&mut tx, project, id).await?;
    if used > 0 && !query.confirm {
        let noun = if used == 1 { "annotation" } else { "annotations" };
        return Err(ApiError::Conflict(format!(
            "label \"{name}\" is used by {used} {noun}; confirm to delete them too"
        )));
    }
    // The affected images change, so editors holding their old version must not save over them.
    sqlx::query(
        "UPDATE images SET version = version + 1 \
         WHERE id IN (SELECT image_id FROM annotations WHERE project_id = ? AND label_id = ?)",
    )
    .bind(project)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Its annotations go with it (foreign key cascade).
    sqlx::query("DELETE FROM labels WHERE project_id = ? AND id = ?").bind(project).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn label_usage(
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
) -> Result<Json<LabelUsage>, ApiError> {
    let mut conn = pool.acquire().await?;
    let exists = sqlx::query("SELECT 1 FROM labels WHERE project_id = ? AND id = ?")
        .bind(project)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    exists.ok_or(ApiError::NotFound)?;
    Ok(Json(LabelUsage { annotations: count_annotations(&mut conn, project, id).await? }))
}

async fn count_annotations(conn: &mut SqliteConnection, project: i64, label: i64) -> Result<i64, ApiError> {
    Ok(sqlx::query("SELECT COUNT(*) AS n FROM annotations WHERE project_id = ? AND label_id = ?")
        .bind(project)
        .bind(label)
        .fetch_one(conn)
        .await?
        .get("n"))
}

fn label_from_row(row: &SqliteRow) -> Label {
    Label {
        id: row.get("id"),
        name: row.get("name"),
        color: row.get("color"),
        hotkey: row.get("hotkey"),
        shape_types: serde_json::from_str(row.get("shape_types")).unwrap_or_default(),
        ignore: row.get("is_ignore"),
        attributes: serde_json::from_str(row.get("attributes")).unwrap_or_default(),
    }
}
