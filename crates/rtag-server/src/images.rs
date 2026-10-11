use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, Query, State},
    routing::{get, post},
};
use rtag_core::{BrowseEntry, BrowseListing, EntryKind, Image, ImagePage, ImportFailure, ImportReport, ImportRequest};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{AppState, error::ApiError, projects};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];
const MAX_PAGE_SIZE: i64 = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/browse", get(browse))
        .route("/api/projects/{project}/imports", post(import_selection))
        .route("/api/projects/{project}/images", get(list_images))
}

/// Images in the project, as a set of paths and a set of content hashes.
#[derive(Default)]
struct Known {
    paths: HashSet<String>,
    hashes: HashSet<String>,
}

async fn known_images(conn: &mut SqliteConnection, project: i64) -> Result<Known, ApiError> {
    let mut known = Known::default();
    for row in sqlx::query("SELECT path, hash FROM images WHERE project_id = ?")
        .bind(project)
        .fetch_all(conn)
        .await?
    {
        known.paths.insert(row.get("path"));
        known.hashes.insert(row.get("hash"));
    }
    Ok(known)
}

#[derive(Deserialize)]
struct BrowseQuery {
    #[serde(default)]
    path: String,
    project: Option<i64>,
}

async fn browse(State(state): State<AppState>, Query(query): Query<BrowseQuery>) -> Result<Json<BrowseListing>, ApiError> {
    let root = image_root(&state)?;
    let folder = resolve(&root, &query.path)?;
    if !folder.is_dir() {
        return Err(ApiError::BadRequest(format!("\"{}\" is not a folder", query.path.trim())));
    }
    let imported = match query.project {
        Some(project) => {
            let mut conn = state.pool.acquire().await?;
            projects::ensure_exists(&mut conn, project).await?;
            Some(known_images(&mut conn, project).await?.paths)
        }
        None => None,
    };

    let mut entries = Vec::new();
    for entry in fs::read_dir(&folder).map_err(|err| ApiError::Internal(format!("cannot list {}: {err}", folder.display())))?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let path = relative_path(&root, &entry.path());
        if kind.is_dir() {
            entries.push(BrowseEntry { name, path, kind: EntryKind::Folder, imported: None });
        } else if kind.is_file() && is_supported(&entry.path()) {
            let imported = imported.as_ref().map(|known| known.contains(&path));
            entries.push(BrowseEntry { name, path, kind: EntryKind::Image, imported });
        }
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Folder, a.name.to_lowercase()).cmp(&(b.kind != EntryKind::Folder, b.name.to_lowercase()))
    });
    Ok(Json(BrowseListing { path: relative_path(&root, &folder), entries }))
}

async fn import_selection(
    State(state): State<AppState>,
    UrlPath(project): UrlPath<i64>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<ImportReport>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    if request.paths.is_empty() {
        return Err(ApiError::BadRequest("select at least one folder or image".into()));
    }
    let root = image_root(&state)?;
    let selection = request.paths.iter().map(|path| resolve(&root, path)).collect::<Result<Vec<_>, _>>()?;
    let known = known_images(&mut conn, project).await?;
    // Don't hold a database connection through a scan that may take minutes.
    drop(conn);

    let (report, new_images) = tokio::task::spawn_blocking(move || scan(&root, &selection, known))
        .await
        .expect("image scan panicked");

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

fn image_root(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .images_root
        .canonicalize()
        .map_err(|err| ApiError::Internal(format!("image root {} is not available: {err}", state.images_root.display())))
}

/// Resolves a folder or file given relative to the image root, refusing anything that leaves it.
fn resolve(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    let relative = relative.trim();
    let outside = || ApiError::BadRequest("path must be inside the image root".into());
    if Path::new(relative).components().any(|part| !matches!(part, Component::Normal(_) | Component::CurDir)) {
        return Err(outside());
    }
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| ApiError::BadRequest(format!("\"{relative}\" not found")))?;
    // A symlink inside the root may still point outside it.
    if !path.starts_with(root) {
        return Err(outside());
    }
    Ok(path)
}

struct NewImage {
    path: String,
    hash: String,
    width: u32,
    height: u32,
}

/// Reads every image in the selection that is not already known. Folders are walked recursively;
/// a file reached twice (selected on its own and through its folder) is handled once.
fn scan(root: &Path, selection: &[PathBuf], mut known: Known) -> (ImportReport, Vec<NewImage>) {
    let mut files = BTreeSet::new();
    for path in selection {
        if path.is_dir() {
            collect_files(path, &mut files);
        } else {
            files.insert(path.clone());
        }
    }

    let mut report = ImportReport::default();
    let mut new_images = Vec::new();
    for file in files {
        let path = relative_path(root, &file);
        if !is_supported(&file) {
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

fn collect_files(dir: &Path, files: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            collect_files(&entry.path(), files);
        } else if kind.is_file() {
            files.insert(entry.path());
        }
    }
}

fn is_supported(file: &Path) -> bool {
    file.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
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
