//! Loading and saving an image's annotations as one batch, guarded by the image's version number.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use oxilabel_core::annotation::{ImageAnnotations, SaveAnnotations};
use sqlx::SqlitePool;

use crate::{AppState, annotation_store, auth::CurrentUser, error::ApiError, jobs};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/projects/{project}/images/{image}/annotations",
        get(load_annotations).put(save_annotations),
    )
}

async fn load_annotations(
    user: CurrentUser,
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut conn = pool.acquire().await?;
    let annotations = annotation_store::load(&mut conn, project, image).await?;
    jobs::ensure_image_access(&mut conn, &user, image).await?;
    Ok(Json(annotations))
}

async fn save_annotations(
    user: CurrentUser,
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
    Json(request): Json<SaveAnnotations>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut tx = pool.begin().await?;
    let in_project: Option<i64> = sqlx::query_scalar("SELECT id FROM images WHERE id = ? AND project_id = ?").bind(image).bind(project).fetch_optional(&mut *tx).await?;
    in_project.ok_or(ApiError::NotFound)?;
    // Annotators save only in their own jobs.
    jobs::ensure_image_access(&mut tx, &user, image).await?;
    let saved = annotation_store::save(&mut tx, project, image, request.base_version, request.annotations).await?;
    tx.commit().await?;
    Ok(Json(saved))
}
