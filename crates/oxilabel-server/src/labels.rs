use std::sync::LazyLock;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, put},
};
use oxilabel_core::{Footprint, Label, LabelDefinition, PointShape, annotation::LabelUsage};
use serde::Deserialize;
use sqlx::{
    Row, Sqlite, SqliteConnection, SqlitePool,
    query::Query as SqlQuery,
    sqlite::{SqliteArguments, SqliteRow},
};

use crate::{
    AppState, access_scope::AccessScope, annotation_store,
    auth::Admin,
    error::ApiError,
    placeholders, projects,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{project}/labels", get(list_labels).post(create_label))
        .route("/api/projects/{project}/labels/{id}", put(update_label).delete(delete_label))
        .route("/api/projects/{project}/labels/{id}/usage", get(label_usage))
}

async fn create_label(
    _: Admin,
    State(pool): State<SqlitePool>,
    Path(project): Path<i64>,
    Json(definition): Json<LabelDefinition>,
) -> Result<(StatusCode, Json<Label>), ApiError> {
    let mut tx = pool.begin().await?;
    let label = insert_label(&mut tx, project, definition).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(label)))
}

/// Checks `definition` against the project's labels and adds it with the next id.
pub(crate) async fn insert_label(conn: &mut SqliteConnection, project: i64, definition: LabelDefinition) -> Result<Label, ApiError> {
    let others = project_labels(conn, project).await?;
    let definition = definition.validated(&others).map_err(ApiError::BadRequest)?;
    // Ids come from a per-project counter, so a deleted label's id is never handed out again.
    let id: i64 = sqlx::query(
        "UPDATE projects SET next_label_id = next_label_id + 1 WHERE id = ? RETURNING next_label_id - 1 AS id",
    )
    .bind(project)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound)?
    .get("id");
    let label = Label::new(id, definition);
    store_label(conn, project, &label).await?;
    Ok(label)
}

/// Writes `label` into the project as it is, id included.
pub(crate) async fn store_label(conn: &mut SqliteConnection, project: i64, label: &Label) -> Result<(), ApiError> {
    bind_label(sqlx::query(&INSERT).bind(project).bind(label.id), label).execute(&mut *conn).await?;
    Ok(())
}

async fn list_labels(scope: AccessScope, State(pool): State<SqlitePool>, Path(project): Path<i64>) -> Result<Json<Vec<Label>>, ApiError> {
    let mut conn = pool.acquire().await?;
    scope.project(&mut conn, project).await?;
    Ok(Json(project_labels(&mut conn, project).await?))
}

/// All labels of a project, or `NotFound` if the project does not exist.
pub(crate) async fn project_labels(conn: &mut SqliteConnection, project: i64) -> Result<Vec<Label>, ApiError> {
    projects::ensure_exists(conn, project).await?;
    let rows = sqlx::query(&SELECT)
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(label_from_row).collect())
}

async fn update_label(
    _: Admin,
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
    bind_label(sqlx::query(&UPDATE), &label).bind(project).bind(id).execute(&mut *tx).await?;
    annotation_store::fit_to_label(&mut tx, project, &label).await?;
    tx.commit().await?;
    Ok(Json(label))
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    confirm: bool,
}

/// Deletes a label. If annotations use it they are deleted too, but only with `?confirm=true`.
async fn delete_label(
    _: Admin,
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
    annotation_store::remove_label_annotations(&mut tx, project, id).await?;
    sqlx::query("DELETE FROM labels WHERE project_id = ? AND id = ?").bind(project).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn label_usage(
    scope: AccessScope,
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
) -> Result<Json<LabelUsage>, ApiError> {
    let mut conn = pool.acquire().await?;
    scope.project(&mut conn, project).await?;
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

/// The columns of a label row besides `project_id` and `id`, in the order `bind_label` binds them;
/// `label_from_row` reads them.
const LABEL_FIELDS: &str = "name, color, hotkey, shape_types, is_ignore, attributes, line_width, point_radius, point_shape, show_footprint, constraints";

static SELECT: LazyLock<String> = LazyLock::new(|| format!("SELECT id, {LABEL_FIELDS} FROM labels WHERE project_id = ? ORDER BY id"));
static INSERT: LazyLock<String> =
    LazyLock::new(|| format!("INSERT INTO labels (project_id, id, {LABEL_FIELDS}) VALUES (?, ?, {})", placeholders(LABEL_FIELDS)));
static UPDATE: LazyLock<String> =
    LazyLock::new(|| format!("UPDATE labels SET ({LABEL_FIELDS}) = ({}) WHERE project_id = ? AND id = ?", placeholders(LABEL_FIELDS)));

/// Binds the label's `LABEL_FIELDS`.
fn bind_label<'q>(query: SqlQuery<'q, Sqlite, SqliteArguments<'q>>, label: &'q Label) -> SqlQuery<'q, Sqlite, SqliteArguments<'q>> {
    query
        .bind(&label.name)
        .bind(&label.color)
        .bind(&label.hotkey)
        .bind(serde_json::to_string(&label.shape_types).expect("shape types serialize"))
        .bind(label.ignore)
        .bind(serde_json::to_string(&label.attributes).expect("attributes serialize"))
        .bind(label.footprint.line_width)
        .bind(label.footprint.point_radius)
        .bind(label.footprint.point_shape.name())
        .bind(label.show_footprint)
        .bind(serde_json::to_string(&label.constraints).expect("constraints serialize"))
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
        footprint: Footprint {
            line_width: row.get("line_width"),
            point_radius: row.get("point_radius"),
            point_shape: PointShape::named(row.get("point_shape")),
        },
        show_footprint: row.get("show_footprint"),
        constraints: serde_json::from_str(row.get("constraints")).unwrap_or_default(),
    }
}
