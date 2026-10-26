//! Loading and saving an image's annotations as one batch, guarded by the image's version number.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use oxilabel_core::annotation::{ImageAnnotations, SaveAnnotations};
use sqlx::SqlitePool;

use crate::{AppState, access_scope::AccessScope, annotation_store, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/projects/{project}/images/{image}/annotations",
        get(load_annotations).put(save_annotations),
    )
}

async fn load_annotations(
    scope: AccessScope,
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut conn = pool.acquire().await?;
    scope.project_image(&mut conn, project, image).await?;
    let annotations = annotation_store::load(&mut conn, project, image).await?;
    Ok(Json(annotations))
}

async fn save_annotations(
    scope: AccessScope,
    State(pool): State<SqlitePool>,
    Path((project, image)): Path<(i64, i64)>,
    Json(request): Json<SaveAnnotations>,
) -> Result<Json<ImageAnnotations>, ApiError> {
    let mut tx = pool.begin().await?;
    scope.project_image(&mut tx, project, image).await?;
    let saved = annotation_store::save(&mut tx, project, image, request.base_version, request.annotations).await?;
    tx.commit().await?;
    Ok(Json(saved))
}
