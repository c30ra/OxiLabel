use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, Query, State},
    routing::{get, post, put},
};
use rtag_core::{
    BrowseEntry, BrowseListing, EntryKind, FolderCount, Image, ImageFilter, ImageNeighbours, ImagePage, ImageStatus, ImportFailure, ImportReport, ImportRequest,
    RemovalReport, RemovalRequest,
};
use serde::Deserialize;
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection, SqlitePool};

use crate::{AppState, auth::Admin, error::ApiError, host::Caller, projects};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];
const MAX_PAGE_SIZE: i64 = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/browse", get(browse))
        .route("/api/browse/count", get(count_folder))
        .route(
            "/api/projects/{project}/uploads",
            post(upload_images).layer(axum::extract::DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .route("/api/projects/{project}/imports", post(import_selection))
        .route("/api/projects/{project}/images", get(list_images))
        .route("/api/projects/{project}/images/{image}", get(get_image))
        .route("/api/projects/{project}/images/{image}/status", put(set_status))
        .route("/api/projects/{project}/images/{image}/neighbours", get(neighbours))
        .route("/api/projects/{project}/image-ids", get(list_image_ids))
        .route("/api/projects/{project}/image-removals", post(remove_images))
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

async fn browse(_: Admin, State(state): State<AppState>, Query(query): Query<BrowseQuery>) -> Result<Json<BrowseListing>, ApiError> {
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

/// How many images a folder holds, subfolders included (what importing it would read).
async fn count_folder(_: Admin, State(state): State<AppState>, caller: Caller, Query(query): Query<BrowseQuery>) -> Result<Json<FolderCount>, ApiError> {
    let root = image_root(&state)?;
    let folder = resolve_for(caller, &root, &query.path)?;
    if !folder.is_dir() {
        return Err(ApiError::BadRequest(format!("\"{}\" is not a folder", query.path.trim())));
    }
    let images = tokio::task::spawn_blocking(move || {
        let mut files = BTreeSet::new();
        collect_files(&folder, &mut files);
        files.iter().filter(|file| is_supported(file)).count() as u64
    })
    .await
    .expect("counting panicked");
    Ok(Json(FolderCount { images }))
}

async fn import_selection(
    _: Admin,
    State(state): State<AppState>,
    caller: Caller,
    UrlPath(project): UrlPath<i64>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<ImportReport>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    if request.paths.is_empty() {
        return Err(ApiError::BadRequest("select at least one folder or image".into()));
    }
    let root = image_root(&state)?;
    let selection = request.paths.iter().map(|path| resolve_for(caller, &root, path)).collect::<Result<Vec<_>, _>>()?;
    let known = known_images(&mut conn, project).await?;
    // Don't hold a database connection through a scan that may take minutes.
    drop(conn);

    let (report, new_images) = tokio::task::spawn_blocking(move || scan(&root, &selection, known))
        .await
        .expect("image scan panicked");

    add_images(&state.pool, project, &new_images).await?;
    Ok(Json(report))
}

async fn add_images(pool: &SqlitePool, project: i64, new_images: &[NewImage]) -> Result<(), ApiError> {
    let mut tx = pool.begin().await?;
    for image in new_images {
        sqlx::query(
            "INSERT INTO images (project_id, path, hash, width, height, size, modified_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project)
        .bind(&image.path)
        .bind(&image.hash)
        .bind(image.width)
        .bind(image.height)
        .bind(image.file.size)
        .bind(image.file.modified_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn remove_images(
    _: Admin,
    State(pool): State<SqlitePool>,
    UrlPath(project): UrlPath<i64>,
    Json(request): Json<RemovalRequest>,
) -> Result<Json<RemovalReport>, ApiError> {
    let mut tx = pool.begin().await?;
    projects::ensure_exists(&mut tx, project).await?;
    if request.ids.is_empty() {
        return Err(ApiError::BadRequest("select at least one image".into()));
    }
    let mut removed = 0;
    for id in request.ids {
        removed += sqlx::query("DELETE FROM images WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(project)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    tx.commit().await?;
    Ok(Json(RemovalReport { removed }))
}

fn image_root(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .images_root
        .canonicalize()
        .map_err(|err| ApiError::Internal(format!("image root {} is not available: {err}", state.images_root.display())))
}

/// Resolves a folder or file: an absolute path as it is, for requests from the server's own
/// machine only (ADR 0003), otherwise a path relative to the image root.
fn resolve_for(caller: Caller, root: &Path, path: &str) -> Result<PathBuf, ApiError> {
    let path = path.trim();
    if !Path::new(path).is_absolute() {
        return resolve(root, path);
    }
    caller.require_local("a folder outside the image root")?;
    Path::new(path).canonicalize().map_err(|_| ApiError::BadRequest(format!("\"{path}\" not found")))
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
    file: FileInfo,
}

/// Size in bytes and modification time in Unix seconds, as stored in the `images` table.
#[derive(Clone, Copy)]
struct FileInfo {
    size: i64,
    modified_at: Option<i64>,
}

impl FileInfo {
    fn of(metadata: &fs::Metadata) -> Self {
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|age| age.as_secs() as i64);
        FileInfo { size: metadata.len() as i64, modified_at }
    }
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
            Ok(image) if known.hashes.contains(&image.hash) => report.skipped += 1,
            Ok(image) => {
                known.hashes.insert(image.hash.clone());
                report.added += 1;
                new_images.push(NewImage { path, ..image });
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

/// How an image's path is stored: relative to the image root (with `/`) when it is inside it,
/// otherwise absolute.
fn relative_path(root: &Path, file: &Path) -> String {
    match file.strip_prefix(root) {
        Ok(relative) => relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => file.to_string_lossy().into_owned(),
    }
}

/// The largest upload request; the browser sends a big folder in several.
const UPLOAD_LIMIT: usize = 1 << 30;

/// Images from another machine's folder (ADR 0003): each part is a file named by its path
/// inside the picked folder. They are kept under the upload folder and imported; copies of
/// duplicates and of files that are not images are removed again.
async fn upload_images(
    _: Admin,
    State(state): State<AppState>,
    UrlPath(project): UrlPath<i64>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<ImportReport>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let known = known_images(&mut conn, project).await?;
    drop(conn);
    let folder = state.upload_dir.join(project.to_string());
    let cannot = |err: std::io::Error| ApiError::Internal(format!("cannot keep an uploaded image: {err}"));
    let mut stored = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::BadRequest(err.to_string()))? {
        let name = field.file_name().unwrap_or_default().to_owned();
        let inside = Path::new(&name);
        if name.is_empty() || inside.components().any(|part| !matches!(part, Component::Normal(_))) {
            return Err(ApiError::BadRequest(format!("\"{name}\" is not a file name inside the picked folder")));
        }
        let bytes = field.bytes().await.map_err(|err| ApiError::BadRequest(err.to_string()))?;
        let target = free_name(&folder.join(inside));
        fs::create_dir_all(target.parent().expect("an uploaded file has a folder")).map_err(cannot)?;
        fs::write(&target, &bytes).map_err(cannot)?;
        stored.push(target.canonicalize().map_err(cannot)?);
    }
    let root = image_root(&state)?;
    let (report, new_images) = tokio::task::spawn_blocking({
        let stored = stored.clone();
        move || scan(&root, &stored, known)
    })
    .await
    .expect("image scan panicked");
    let kept: BTreeSet<&str> = new_images.iter().map(|image| image.path.as_str()).collect();
    for file in &stored {
        if !kept.contains(file.to_string_lossy().as_ref()) {
            fs::remove_file(file).ok();
        }
    }
    add_images(&state.pool, project, &new_images).await?;
    Ok(Json(report))
}

/// `path`, or `name-2.ext`, `name-3.ext`… if it is taken, so an upload never overwrites a kept image.
fn free_name(path: &Path) -> PathBuf {
    let (stem, extension) = (path.file_stem().unwrap_or_default().to_string_lossy(), path.extension().map(|e| e.to_string_lossy()));
    (1..)
        .map(|n| match (n, &extension) {
            (1, _) => path.to_owned(),
            (n, Some(ext)) => path.with_file_name(format!("{stem}-{n}.{ext}")),
            (n, None) => path.with_file_name(format!("{stem}-{n}")),
        })
        .find(|candidate| !candidate.exists())
        .expect("some name is free")
}

/// Content hash, dimensions and file info of an image file (without its path). Only the header is decoded.
fn read_image(file: &Path) -> Result<NewImage, String> {
    let info = FileInfo::of(&fs::metadata(file).map_err(|err| err.to_string())?);
    let bytes = fs::read(file).map_err(|err| err.to_string())?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let (width, height) = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?
        .into_dimensions()
        .map_err(|err| err.to_string())?;
    Ok(NewImage { path: String::new(), hash, width, height, file: info })
}

/// Fills in size and modification time for images recorded before they were stored.
/// Files that are missing stay unknown.
pub async fn backfill_file_info(pool: SqlitePool, root: PathBuf) {
    let Ok(rows) = sqlx::query("SELECT id, path FROM images WHERE size IS NULL").fetch_all(&pool).await else { return };
    if rows.is_empty() {
        return;
    }
    let rows: Vec<(i64, String)> = rows.iter().map(|row| (row.get("id"), row.get("path"))).collect();
    let found = tokio::task::spawn_blocking(move || {
        rows.into_iter()
            .filter_map(|(id, path)| fs::metadata(root.join(&path)).ok().map(|meta| (id, FileInfo::of(&meta))))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let Ok(mut tx) = pool.begin().await else { return };
    for (id, info) in found {
        let updated = sqlx::query("UPDATE images SET size = ?, modified_at = ? WHERE id = ?")
            .bind(info.size)
            .bind(info.modified_at)
            .bind(id)
            .execute(&mut *tx)
            .await;
        if updated.is_err() {
            return;
        }
    }
    let _ = tx.commit().await;
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

/// Appends the conditions selecting the project's images that match `filter`.
fn push_filter(query: &mut QueryBuilder<'_, Sqlite>, project: i64, filter: &ImageFilter) -> Result<(), ApiError> {
    query.push(" WHERE project_id = ").push_bind(project);
    if let Some(name) = filter.name.as_deref().filter(|name| !name.is_empty()) {
        if let Err(err) = regex::RegexBuilder::new(name).case_insensitive(true).build() {
            // Syntax errors span several lines pointing at the pattern; the last one says what is wrong.
            let message = err.to_string();
            let reason = message.lines().last().unwrap_or_default().trim_start_matches("error: ");
            return Err(ApiError::BadRequest(format!("invalid name filter: {reason}")));
        }
        query.push(" AND path REGEXP ").push_bind(format!("(?i){name}"));
    }
    let bounds = [
        ("width >= ", filter.min_width.map(i64::from)),
        ("width <= ", filter.max_width.map(i64::from)),
        ("height >= ", filter.min_height.map(i64::from)),
        ("height <= ", filter.max_height.map(i64::from)),
        ("size >= ", filter.min_size.map(|v| v as i64)),
        ("size <= ", filter.max_size.map(|v| v as i64)),
    ];
    for (condition, value) in bounds {
        if let Some(value) = value {
            query.push(" AND ").push(condition).push_bind(value);
        }
    }
    let dates = [(">= ", &filter.modified_from), ("<= ", &filter.modified_to)];
    for (comparison, date) in dates {
        if let Some(date) = date.as_deref().filter(|date| !date.is_empty()) {
            if !is_date(date) {
                return Err(ApiError::BadRequest("dates must look like 2024-12-31".into()));
            }
            query.push(" AND date(modified_at, 'unixepoch', 'localtime') ").push(comparison).push_bind(date.to_owned());
        }
    }
    Ok(())
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() })
}

async fn list_images(
    State(pool): State<SqlitePool>,
    UrlPath(project): UrlPath<i64>,
    Query(page): Query<PageQuery>,
    Query(filter): Query<ImageFilter>,
) -> Result<Json<ImagePage>, ApiError> {
    let mut conn = pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let mut count = QueryBuilder::new("SELECT COUNT(*) AS n FROM images");
    push_filter(&mut count, project, &filter)?;
    let total: i64 = count.build().fetch_one(&mut *conn).await?.get("n");

    let mut select = QueryBuilder::new(IMAGE_COLUMNS);
    push_filter(&mut select, project, &filter)?;
    select
        .push(" ORDER BY path LIMIT ")
        .push_bind(page.limit.clamp(1, MAX_PAGE_SIZE))
        .push(" OFFSET ")
        .push_bind(page.offset.max(0));
    let rows = select.build().fetch_all(&mut *conn).await?;
    Ok(Json(ImagePage { total, images: rows.iter().map(image_from_row).collect() }))
}

/// Selects the columns `image_from_row` reads; callers append the conditions.
const IMAGE_COLUMNS: &str = "SELECT id, path, width, height, annotated, size, \
     strftime('%Y-%m-%d %H:%M', modified_at, 'unixepoch', 'localtime') AS modified, \
     (SELECT COUNT(*) FROM annotations WHERE annotations.image_id = images.id) AS annotation_count FROM images";

fn image_from_row(row: &sqlx::sqlite::SqliteRow) -> Image {
    Image {
        id: row.get("id"),
        path: row.get("path"),
        width: row.get("width"),
        height: row.get("height"),
        annotated: row.get("annotated"),
        annotations: row.get("annotation_count"),
        size: row.get::<Option<i64>, _>("size").map(|size| size as u64),
        modified: row.get("modified"),
    }
}

async fn get_image(
    State(pool): State<SqlitePool>,
    UrlPath((project, image)): UrlPath<(i64, i64)>,
) -> Result<Json<Image>, ApiError> {
    let row = sqlx::query(&format!("{IMAGE_COLUMNS} WHERE id = ? AND project_id = ?"))
        .bind(image)
        .bind(project)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(image_from_row(&row)))
}

/// Marks an image annotated or not annotated.
async fn set_status(
    State(pool): State<SqlitePool>,
    UrlPath((project, image)): UrlPath<(i64, i64)>,
    Json(status): Json<ImageStatus>,
) -> Result<Json<Image>, ApiError> {
    let updated = sqlx::query("UPDATE images SET annotated = ? WHERE id = ? AND project_id = ?")
        .bind(status.annotated)
        .bind(image)
        .bind(project)
        .execute(&pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    get_image(State(pool), UrlPath((project, image))).await
}

#[derive(Deserialize)]
struct NeighboursQuery {
    /// How many images to give on each side.
    #[serde(default = "default_neighbours")]
    count: i64,
}

fn default_neighbours() -> i64 {
    6
}

/// An image's place in its project (by path) and up to `count` images on each side, for the
/// filmstrip.
async fn neighbours(
    State(pool): State<SqlitePool>,
    UrlPath((project, image)): UrlPath<(i64, i64)>,
    Query(query): Query<NeighboursQuery>,
) -> Result<Json<ImageNeighbours>, ApiError> {
    let path: String = sqlx::query("SELECT path FROM images WHERE id = ? AND project_id = ?")
        .bind(image)
        .bind(project)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?
        .get("path");
    let count = query.count.clamp(0, 50);
    let before: i64 = sqlx::query("SELECT COUNT(*) AS n FROM images WHERE project_id = ? AND path < ?")
        .bind(project)
        .bind(&path)
        .fetch_one(&pool)
        .await?
        .get("n");
    let total: i64 = sqlx::query("SELECT COUNT(*) AS n FROM images WHERE project_id = ?").bind(project).fetch_one(&pool).await?.get("n");
    let rows = sqlx::query(&format!("{IMAGE_COLUMNS} WHERE project_id = ? ORDER BY path LIMIT ? OFFSET ?"))
        .bind(project)
        .bind(2 * count + 1)
        .bind((before - count).max(0))
        .fetch_all(&pool)
        .await?;
    // When fewer than `count` images come before, the window starts at the first and is shorter.
    let images: Vec<Image> = rows.iter().map(image_from_row).take((count + 1 + before.min(count)) as usize).collect();
    Ok(Json(ImageNeighbours { position: before + 1, total, images }))
}

/// Ids of every image matching the filter, in path order; used to select all of them at once.
async fn list_image_ids(
    State(pool): State<SqlitePool>,
    UrlPath(project): UrlPath<i64>,
    Query(filter): Query<ImageFilter>,
) -> Result<Json<Vec<i64>>, ApiError> {
    let mut conn = pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let mut select = QueryBuilder::new("SELECT id FROM images");
    push_filter(&mut select, project, &filter)?;
    select.push(" ORDER BY path");
    let rows = select.build().fetch_all(&mut *conn).await?;
    Ok(Json(rows.iter().map(|row| row.get("id")).collect()))
}
