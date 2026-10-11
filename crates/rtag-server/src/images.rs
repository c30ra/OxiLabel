use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, Query, State},
    routing::{get, post},
};
use rtag_core::{Image, ImagePage, ImportFailure, ImportReport, ImportRequest};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};

use crate::{AppState, error::ApiError, projects};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];
const MAX_PAGE_SIZE: i64 = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{project}/imports", post(import_folder))
        .route("/api/projects/{project}/images", get(list_images))
}

async fn import_folder(
    State(state): State<AppState>,
    UrlPath(project): UrlPath<i64>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<ImportReport>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let root = state.images_root.canonicalize().map_err(|_| ApiError::BadRequest("image root is not available".into()))?;
    let folder = resolve_folder(&root, &request.folder)?;

    let mut known = Known::default();
    for row in sqlx::query("SELECT path, hash FROM images WHERE project_id = ?")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?
    {
        known.paths.insert(row.get("path"));
        known.hashes.insert(row.get("hash"));
    }
    // Don't hold a database connection through a scan that may take minutes.
    drop(conn);

    let (report, new_images) = tokio::task::spawn_blocking(move || scan(&root, &folder, known))
        .await
        .expect("folder scan panicked");

    let mut tx = state.pool.begin().await?;
    for image in &new_images {
        sqlx::query("INSERT INTO images (project_id, path, hash, width, height) VALUES (?, ?, ?, ?, ?)")
            .bind(project)
            .bind(&image.path)
            .bind(&image.hash)
            .bind(image.width)
            .bind(image.height)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(report))
}

/// Resolves a folder given relative to the image root, refusing anything that leaves the root.
fn resolve_folder(root: &Path, folder: &str) -> Result<PathBuf, ApiError> {
    let relative = Path::new(folder.trim());
    if relative.components().any(|part| !matches!(part, Component::Normal(_) | Component::CurDir)) {
        return Err(ApiError::BadRequest("folder must be a path inside the image root".into()));
    }
    let folder = root
        .join(relative)
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
        .ok_or_else(|| ApiError::BadRequest(format!("folder \"{}\" not found", relative.display())))?;
    if !folder.starts_with(root) {
        return Err(ApiError::BadRequest("folder must be a path inside the image root".into()));
    }
    Ok(folder)
}

#[derive(Default)]
struct Known {
    paths: HashSet<String>,
    hashes: HashSet<String>,
}

struct NewImage {
    path: String,
    hash: String,
    width: u32,
    height: u32,
}

/// Walks `folder` recursively in path order and reads every image not already known.
fn scan(root: &Path, folder: &Path, mut known: Known) -> (ImportReport, Vec<NewImage>) {
    let mut report = ImportReport::default();
    let mut new_images = Vec::new();
    let mut files = Vec::new();
    collect_files(folder, &mut files);
    files.sort();

    for file in files {
        let path = relative_path(root, &file);
        let supported = file
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
        if !supported {
            report.unsupported += 1;
            continue;
        }
        if known.paths.contains(&path) {
            report.skipped += 1;
            continue;
        }
        match read_image(&file) {
            Ok((hash, _, _)) if known.hashes.contains(&hash) => report.skipped += 1,
            Ok((hash, width, height)) => {
                known.hashes.insert(hash.clone());
                report.added += 1;
                new_images.push(NewImage { path, hash, width, height });
            }
            Err(reason) => report.failed.push(ImportFailure { path, reason }),
        }
    }
    (report, new_images)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            collect_files(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
}

fn relative_path(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// Content hash and dimensions of an image file. Only the header is decoded.
fn read_image(file: &Path) -> Result<(String, u32, u32), String> {
    let bytes = fs::read(file).map_err(|err| err.to_string())?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let (width, height) = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?
        .into_dimensions()
        .map_err(|err| err.to_string())?;
    Ok((hash, width, height))
}

#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    offset: i64,
    #[serde(default = "default_page_size")]
    limit: i64,
}

fn default_page_size() -> i64 {
    100
}

async fn list_images(
    State(pool): State<SqlitePool>,
    UrlPath(project): UrlPath<i64>,
    Query(page): Query<PageQuery>,
) -> Result<Json<ImagePage>, ApiError> {
    let mut conn = pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let total: i64 = sqlx::query("SELECT COUNT(*) AS n FROM images WHERE project_id = ?")
        .bind(project)
        .fetch_one(&mut *conn)
        .await?
        .get("n");
    let rows = sqlx::query(
        "SELECT id, path, width, height, annotated FROM images WHERE project_id = ? ORDER BY path LIMIT ? OFFSET ?",
    )
    .bind(project)
    .bind(page.limit.clamp(1, MAX_PAGE_SIZE))
    .bind(page.offset.max(0))
    .fetch_all(&mut *conn)
    .await?;
    let images = rows
        .iter()
        .map(|row| Image {
            id: row.get("id"),
            path: row.get("path"),
            width: row.get("width"),
            height: row.get("height"),
            annotated: row.get("annotated"),
        })
        .collect();
    Ok(Json(ImagePage { total, images }))
}
