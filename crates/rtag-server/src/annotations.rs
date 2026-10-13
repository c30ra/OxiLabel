//! Loading and saving an image's annotations as one batch, guarded by the image's version number.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use rtag_core::annotation::{Annotation, ImageAnnotations, SaveAnnotations};
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{AppState, error::ApiError, labels};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/projects/{project}/images/{image}/annotations",
        get(load_annotations).put(save_annotations),
    )
}

/// The image's size and current version, or `NotFound` if it is not in the project.
async fn image_in_project(conn: &mut SqliteConnection, project: i64, image: i64) -> Result<(u32, u32, i64), ApiError> {
    let row = sqlx::query("SELECT width, height, version FROM images WHERE id = ? AND project_id = ?")
        .bind(image)
        .bind(project)
        .fetch_optional(conn)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok((row.get("width"), row.get("height"), row.get("version")))
}

async fn load_annotations(
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut conn = pool.acquire().await?;
    let (_, _, version) = image_in_project(&mut conn, project, image).await?;
    let rows = sqlx::query("SELECT label_id, shape, source, confidence, attributes, hidden, locked FROM annotations WHERE image_id = ? ORDER BY position")
        .bind(image)
        .fetch_all(&mut *conn)
        .await?;
    let annotations = rows.iter().map(annotation_from_row).collect::<Result<_, ApiError>>()?;
    Ok(Json(ImageAnnotations { version, annotations }))
}

/// Reads an annotation from a row with `label_id`, `shape`, `source`, `confidence`,
/// `attributes`, `hidden` and `locked`.
pub(crate) fn annotation_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Annotation, ApiError> {
    let unreadable = |what: &str, err: serde_json::Error| ApiError::Internal(format!("stored {what} are unreadable: {err}"));
    let shape = serde_json::from_str(row.get("shape")).map_err(|err| unreadable("shapes", err))?;
    let attributes = serde_json::from_str(row.get("attributes")).map_err(|err| unreadable("attributes", err))?;
    Ok(Annotation {
        label: row.get("label_id"),
        shape,
        source: row.get("source"),
        confidence: row.get("confidence"),
        attributes,
        hidden: row.get("hidden"),
        locked: row.get("locked"),
    })
}

async fn save_annotations(
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
    Json(request): Json<SaveAnnotations>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut tx = pool.begin().await?;
    let (width, height, current) = image_in_project(&mut tx, project, image).await?;
    // A stale editor is told to reload first: that also resolves labels deleted meanwhile.
    if current != request.base_version {
        return Err(stale(current));
    }
    let labels = labels::project_labels(&mut tx, project).await?;
    for (index, annotation) in request.annotations.iter().enumerate() {
        let invalid = |reason: String| ApiError::BadRequest(format!("annotation {}: {reason}", index + 1));
        let label = labels
            .iter()
            .find(|label| label.id == annotation.label)
            .ok_or_else(|| invalid(format!("there is no label with id {}", annotation.label)))?;
        let shape_type = annotation.shape.shape_type();
        if !label.shape_types.contains(&shape_type) {
            let name = serde_json::to_value(shape_type).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            return Err(invalid(format!("label \"{}\" does not allow {name} shapes", label.name)));
        }
        annotation.shape.validate(width, height).map_err(invalid)?;
        label.check_attributes(&annotation.attributes).map_err(invalid)?;
        let repeated = annotation.shape.is_tag()
            && request.annotations[..index].iter().any(|earlier| earlier.shape.is_tag() && earlier.label == annotation.label);
        if repeated {
            return Err(invalid(format!("the image already has the tag \"{}\"", label.name)));
        }
    }

    // Checking and bumping the version in one statement keeps concurrent saves safe.
    let bumped = sqlx::query("UPDATE images SET version = version + 1 WHERE id = ? AND version = ?")
        .bind(image)
        .bind(request.base_version)
        .execute(&mut *tx)
        .await?;
    if bumped.rows_affected() == 0 {
        let (_, _, current) = image_in_project(&mut tx, project, image).await?;
        return Err(stale(current));
    }
    sqlx::query("DELETE FROM annotations WHERE image_id = ?").bind(image).execute(&mut *tx).await?;
    for (position, annotation) in request.annotations.iter().enumerate() {
        let shape = serde_json::to_string(&annotation.shape).expect("shapes serialize");
        sqlx::query(
            "INSERT INTO annotations (image_id, project_id, label_id, position, shape, source, confidence, attributes, hidden, locked) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(image)
        .bind(project)
        .bind(annotation.label)
        .bind(position as i64)
        .bind(shape)
        .bind(&annotation.source)
        .bind(annotation.confidence)
        .bind(serde_json::to_string(&annotation.attributes).expect("attributes serialize"))
        .bind(annotation.hidden)
        .bind(annotation.locked)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(ImageAnnotations { version: request.base_version + 1, annotations: request.annotations }))
}

fn stale(current: i64) -> ApiError {
    ApiError::Conflict(format!(
        "these annotations were changed elsewhere since you opened the image (now version {current}); reload to see them"
    ))
}
