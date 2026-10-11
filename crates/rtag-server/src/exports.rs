//! Exports: a chosen exporter writes a project's images and annotations into a new folder under
//! the export directory, which can also be downloaded as a zip.

use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as UrlPath, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
};
use rtag_core::{
    annotation::Annotation,
    export::{ExportFormat, ExportRequest, ExportResult, ImageSelection},
};
use rtag_formats::{ExportImage, ExportInput, FolderSink, run_export};
use sqlx::Row;
use tokio_util::io::ReaderStream;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{AppState, annotations::annotation_from_row, error::ApiError, labels, write_atomically_to};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/exporters", get(list_exporters))
        .route("/api/projects/{project}/exports", post(create_export))
        .route("/api/exports/{name}/zip", get(download_zip))
}

async fn list_exporters() -> Json<Vec<ExportFormat>> {
    Json(
        rtag_formats::exporters()
            .iter()
            .map(|exporter| ExportFormat { id: exporter.id().to_owned(), name: exporter.name().to_owned() })
            .collect(),
    )
}

async fn create_export(
    State(state): State<AppState>,
    UrlPath(project): UrlPath<i64>,
    Json(request): Json<ExportRequest>,
) -> Result<Json<ExportResult>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let project_name: String = sqlx::query("SELECT name FROM projects WHERE id = ?")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApiError::NotFound)?
        .get("name");
    let exporter = rtag_formats::exporter(&request.format)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown export format \"{}\"", request.format)))?;
    request.split.validate().map_err(ApiError::BadRequest)?;
    let labels = labels::project_labels(&mut conn, project).await?;

    let mut annotations: HashMap<i64, Vec<Annotation>> = HashMap::new();
    for row in sqlx::query(
        "SELECT image_id, label_id, shape, source, confidence FROM annotations WHERE project_id = ? ORDER BY image_id, position",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?
    {
        annotations.entry(row.get("image_id")).or_default().push(annotation_from_row(&row)?);
    }
    let rows = sqlx::query("SELECT id, path, hash, width, height FROM images WHERE project_id = ? ORDER BY path")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    drop(conn);
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| {
            let id: i64 = row.get("id");
            match &request.images {
                ImageSelection::All => true,
                ImageSelection::WithAnnotations => annotations.get(&id).is_some_and(|list| !list.is_empty()),
                ImageSelection::Ids { ids } => ids.contains(&id),
            }
        })
        .collect();
    let images: Vec<ExportImage> = selected
        .into_iter()
        .map(|row| {
            let path: String = row.get("path");
            ExportImage {
                source: state.images_root.join(&path),
                path,
                hash: row.get("hash"),
                width: row.get("width"),
                height: row.get("height"),
                annotations: annotations.remove(&row.get::<i64, _>("id")).unwrap_or_default(),
            }
        })
        .collect();
    if images.is_empty() {
        return Err(ApiError::BadRequest("there are no images to export".into()));
    }

    let (created, stamp) = utc_now();
    let input = ExportInput { project: project_name.clone(), labels, images, split: request.split };
    let export_dir = state.export_dir.clone();
    let format = request.format.clone();
    let mode = request.include_images;
    let (name, dir, manifest) = tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
        let (name, dir) = new_folder(&export_dir, &format!("{}-{format}-{stamp}", slug(&project_name)))
            .map_err(|err| ApiError::Internal(format!("cannot create export folder: {err}")))?;
        let mut sink = FolderSink::new(&dir, mode);
        match run_export(exporter.as_ref(), &input, &mut sink, &created, mode) {
            Ok(manifest) => Ok((name, dir, manifest)),
            Err(err) => {
                let _ = fs::remove_dir_all(&dir);
                Err(ApiError::Internal(format!("export failed: {err}")))
            }
        }
    })
    .await
    .expect("export task panicked")?;

    Ok(Json(ExportResult {
        download: format!("/api/exports/{name}/zip"),
        path: dir.display().to_string(),
        name,
        image_count: manifest.image_count,
        annotation_count: manifest.annotation_count,
        subsets: manifest.subsets,
    }))
}

/// Creates `<base>`, or `<base>-2`, `<base>-3`, … if it exists, in `parent`.
fn new_folder(parent: &Path, base: &str) -> io::Result<(String, std::path::PathBuf)> {
    fs::create_dir_all(parent)?;
    for attempt in 1.. {
        let name = if attempt == 1 { base.to_owned() } else { format!("{base}-{attempt}") };
        let dir = parent.join(&name);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((name, dir)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    unreachable!("the loop only ends by returning")
}

/// Lowercase letters, digits and single dashes, for folder names.
fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() { "project".to_owned() } else { slug }
}

/// The current UTC time as RFC 3339 (`2026-10-11T12:34:56Z`) and compact (`20261011-123456`).
fn utc_now() -> (String, String) {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |age| age.as_secs());
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let (hour, minute, second) = (seconds % 86_400 / 3600, seconds % 3600 / 60, seconds % 60);
    (
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"),
        format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}"),
    )
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

async fn download_zip(State(state): State<AppState>, UrlPath(name): UrlPath<String>) -> Result<impl IntoResponse, ApiError> {
    // Only names this server generates; anything else (e.g. `..`) is not an export.
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let dir = state.export_dir.join(&name);
    if !valid || !dir.is_dir() {
        return Err(ApiError::NotFound);
    }
    // Export folders never change, so the zip is built once and kept next to the folder.
    let zip_path = state.export_dir.join(format!("{name}.zip"));
    if !zip_path.exists() {
        let (dir, zip_path) = (dir.clone(), zip_path.clone());
        tokio::task::spawn_blocking(move || write_atomically_to(&zip_path, |file| write_zip(&dir, file)))
            .await
            .expect("zip task panicked")
            .map_err(|err| ApiError::Internal(format!("cannot build zip: {err}")))?;
    }
    let file = tokio::fs::File::open(&zip_path)
        .await
        .map_err(|err| ApiError::Internal(format!("cannot open zip: {err}")))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}.zip\"")),
        ],
        Body::from_stream(ReaderStream::new(file)),
    ))
}

/// Zips every file under `dir` (following symbolic links, so linked images are stored as files).
fn write_zip(dir: &Path, out: &mut fs::File) -> io::Result<()> {
    let mut zip = ZipWriter::new(out);
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    for relative in files {
        let source = dir.join(&relative);
        let size = fs::metadata(&source)?.len();
        // Images are compressed already; text compresses well.
        let text = matches!(Path::new(&relative).extension().and_then(|e| e.to_str()), Some("txt" | "yaml" | "json"));
        let options = SimpleFileOptions::default()
            .compression_method(if text { CompressionMethod::Deflated } else { CompressionMethod::Stored })
            .large_file(size >= u64::from(u32::MAX));
        zip.start_file(relative, options).map_err(io::Error::other)?;
        io::copy(&mut fs::File::open(&source)?, &mut zip)?;
    }
    zip.finish().map_err(io::Error::other)?.flush()
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if fs::metadata(&path)?.is_dir() {
            collect(root, &path, files)?;
        } else {
            let relative = path.strip_prefix(root).expect("inside the root");
            files.push(relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_1970_become_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn project_names_become_folder_friendly() {
        assert_eq!(slug("EL defects"), "el-defects");
        assert_eq!(slug("  Cell broken - FRONT!! "), "cell-broken-front");
        assert_eq!(slug("…"), "project");
    }
}
