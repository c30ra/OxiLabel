//! Deleting a project, and with it, if asked, its stored copies: image files the server keeps in
//! the upload folder (ADR 0003) that no other project uses. Originals are never touched. Cached
//! thumbnails and tiles whose content no remaining image has go too.

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use axum::{
    Json,
    extract::{Path as UrlPath, Query, State},
};
use oxilabel_core::{FailedFile, KeptFile, ProjectDeletionPreview, ProjectDeletionReport};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection};

use crate::{
    AppState,
    auth::Admin,
    error::ApiError,
    images::{relative_path, source_file},
    projects,
};

#[derive(Deserialize)]
pub(crate) struct DeletionOptions {
    #[serde(default)]
    delete_stored_copies: bool,
}

/// One of the project's image files, sorted by who may delete it.
enum ImageFile {
    Original,
    StoredCopy { file: PathBuf, shared_with: Option<String> },
}

pub(crate) async fn preview(_: Admin, State(state): State<AppState>, UrlPath(project): UrlPath<i64>) -> Result<Json<ProjectDeletionPreview>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let mut preview = ProjectDeletionPreview::default();
    for (file, _) in image_files(&mut conn, &state, project).await? {
        match file {
            ImageFile::Original => preview.originals += 1,
            ImageFile::StoredCopy { shared_with: None, .. } => preview.stored_copies += 1,
            ImageFile::StoredCopy { shared_with: Some(_), .. } => preview.shared_copies += 1,
        }
    }
    Ok(Json(preview))
}

pub(crate) async fn delete(
    _: Admin,
    State(state): State<AppState>,
    UrlPath(project): UrlPath<i64>,
    Query(options): Query<DeletionOptions>,
) -> Result<Json<ProjectDeletionReport>, ApiError> {
    let mut tx = state.pool.begin().await?;
    // Read before the rows go; an unknown project simply has none.
    let files = image_files(&mut tx, &state, project).await?;
    let deleted = sqlx::query("DELETE FROM projects WHERE id = ?").bind(project).execute(&mut *tx).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    tx.commit().await?;

    let hashes: BTreeSet<String> = files.iter().map(|(_, hash)| hash.clone()).collect();
    let mut orphaned = Vec::new();
    for hash in hashes {
        let used: bool = sqlx::query("SELECT EXISTS (SELECT 1 FROM images WHERE hash = ?) AS used")
            .bind(&hash)
            .fetch_one(&state.pool)
            .await?
            .get("used");
        if !used {
            orphaned.push(hash);
        }
    }
    let copies: Vec<(PathBuf, Option<String>)> = files
        .into_iter()
        .filter_map(|(file, _)| match file {
            ImageFile::StoredCopy { file, shared_with } if options.delete_stored_copies => Some((file, shared_with)),
            _ => None,
        })
        .collect();
    let uploads = absolute(&state.upload_dir);
    let cache = state.cache_dir.clone();
    let report = tokio::task::spawn_blocking(move || {
        forget_cached(&cache, &orphaned);
        delete_copies(&uploads, copies)
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(Json(report))
}

/// The project's image files with their content hashes.
async fn image_files(conn: &mut SqliteConnection, state: &AppState, project: i64) -> Result<Vec<(ImageFile, String)>, ApiError> {
    // A missing image root only means no relative path names a stored copy.
    let root = absolute(&state.images_root);
    let uploads = absolute(&state.upload_dir);
    let rows = sqlx::query("SELECT path, hash FROM images WHERE project_id = ?").bind(project).fetch_all(&mut *conn).await?;
    let mut files = Vec::with_capacity(rows.len());
    for row in rows {
        let stored: String = row.get("path");
        let file = absolute(&source_file(&root, &stored));
        let image = if file.starts_with(&uploads) {
            // Another project may name the same file by either form of its path.
            let shared_with = sqlx::query(
                "SELECT projects.name FROM images JOIN projects ON projects.id = images.project_id \
                 WHERE images.project_id <> ? AND images.path IN (?, ?) ORDER BY projects.name LIMIT 1",
            )
            .bind(project)
            .bind(file.to_string_lossy())
            .bind(relative_path(&root, &file))
            .fetch_optional(&mut *conn)
            .await?
            .map(|row| row.get("name"));
            ImageFile::StoredCopy { file, shared_with }
        } else {
            ImageFile::Original
        };
        files.push((image, row.get("hash")));
    }
    Ok(files)
}

/// `path` with symbolic links resolved when it exists, otherwise just made absolute.
fn absolute(path: &Path) -> PathBuf {
    path.canonicalize().or_else(|_| std::path::absolute(path)).unwrap_or_else(|_| path.to_owned())
}

/// Deletes the copies no other project uses, then the folders under `uploads` they leave empty.
fn delete_copies(uploads: &Path, copies: Vec<(PathBuf, Option<String>)>) -> ProjectDeletionReport {
    let mut report = ProjectDeletionReport::default();
    let mut folders = BTreeSet::new();
    for (file, shared_with) in copies {
        let path = file.to_string_lossy().into_owned();
        if let Some(project) = shared_with {
            report.kept.push(KeptFile { path, project });
            continue;
        }
        match fs::remove_file(&file) {
            Ok(()) => report.deleted.push(path),
            Err(err) => report.failed.push(FailedFile { path, reason: reason(&err) }),
        }
        folders.extend(file.ancestors().skip(1).take_while(|folder| *folder != uploads && folder.starts_with(uploads)).map(Path::to_owned));
    }
    // Deepest first, so a folder emptied by removing its subfolders goes too; non-empty ones fail.
    for folder in folders.iter().rev() {
        fs::remove_dir(folder).ok();
    }
    report.deleted.sort();
    report.kept.sort_by(|a, b| a.path.cmp(&b.path));
    report.failed.sort_by(|a, b| a.path.cmp(&b.path));
    report
}

fn reason(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::NotFound => "already missing".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => err.to_string(),
    }
}

/// Removes the cached thumbnails and tiles of content no image has any more.
fn forget_cached(cache: &Path, hashes: &[String]) {
    for hash in hashes {
        fs::remove_file(cache.join("thumbnails").join(format!("{hash}.jpg"))).ok();
        fs::remove_dir_all(cache.join("tiles").join(hash)).ok();
    }
}
