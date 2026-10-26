use std::{
    fs,
    io::Cursor,
    path::Path,
};

use axum::{
    Router,
    extract::{Path as UrlPath, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use image::{DynamicImage, ImageFormat};

use crate::{AppState, access_scope::AccessScope, error::ApiError, write_atomically};

/// Thumbnails fit in a square of this many pixels.
const THUMBNAIL_SIZE: u32 = 256;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/images/{id}/thumbnail", get(thumbnail))
}

async fn thumbnail(scope: AccessScope, State(state): State<AppState>, UrlPath(id): UrlPath<i64>) -> Result<impl IntoResponse, ApiError> {
    let visible = scope.image(&mut *state.pool.acquire().await?, id).await?;
    let source = visible.source;
    // Named by content hash, so identical images share one cached thumbnail.
    let cached = state.cache_dir.join("thumbnails").join(format!("{}.jpg", visible.hash));

    let bytes = tokio::task::spawn_blocking(move || cached_thumbnail(&source, &cached))
        .await
        .expect("thumbnail task panicked")?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=86400")], bytes))
}

fn cached_thumbnail(source: &Path, cached: &Path) -> Result<Vec<u8>, ApiError> {
    if let Ok(bytes) = fs::read(cached) {
        return Ok(bytes);
    }
    let image = open_image(source)?;
    let bytes = encode_thumbnail(&image).map_err(|err| ApiError::Internal(format!("cannot encode thumbnail: {err}")))?;
    write_atomically(cached, &bytes).map_err(|err| ApiError::Internal(format!("cannot cache thumbnail: {err}")))?;
    Ok(bytes)
}

/// Decodes a source image; a file that has gone missing is reported as not found.
pub(crate) fn open_image(source: &Path) -> Result<DynamicImage, ApiError> {
    image::open(source).map_err(|err| match err {
        image::ImageError::IoError(io) if io.kind() == std::io::ErrorKind::NotFound => ApiError::NotFound,
        err => ApiError::Internal(format!("cannot decode {}: {err}", source.display())),
    })
}

fn encode_thumbnail(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    let fits = image.width() <= THUMBNAIL_SIZE && image.height() <= THUMBNAIL_SIZE;
    let thumbnail = if fits { image.clone() } else { image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE) };
    let mut bytes = Vec::new();
    // JPEG has no alpha channel, so flatten to RGB first.
    DynamicImage::ImageRgb8(thumbnail.to_rgb8()).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)?;
    Ok(bytes)
}
