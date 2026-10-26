//! Project archives (`oxilabel_formats::archive`): exporting a whole project, with or without its
//! image files, and importing one as a new project. Labels keep their ids, jobs their numbers
//! (assigned to the user of the same name, if there is one), images their status and annotations.
//!
//! An image the archive includes is kept in the upload folder (ADR 0003). One it only refers to is
//! looked for at its path (an absolute one only for requests from the server's own machine), then
//! by file name in the plan's images folder; either way its content hash must match.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use oxilabel_core::{
    Project,
    import::{ArchiveImportPlan, ArchiveImportReport, SkipCount},
};
use oxilabel_formats::archive::{ArchiveImage, ArchiveJob, ArchiveReader, ArchiveWriter, ProjectArchive};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};

use crate::{
    AppState,
    annotation_store::{self, Replacing, annotation_from_row},
    auth::Admin,
    error::ApiError,
    host::Caller,
    images::{self, NewImage},
    labels,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{project}/archive", get(export_archive))
        // Archives with images can be far larger than any other upload.
        .route("/api/project-archives", post(import_archive).layer(DefaultBodyLimit::disable()))
}

/// The largest image file taken out of an archive.
const IMAGE_LIMIT: u64 = 4 << 30;

fn internal(err: impl std::fmt::Display) -> ApiError {
    ApiError::Internal(err.to_string())
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    images: bool,
}

/// The project as an archive, read from the database.
async fn project_archive(pool: &SqlitePool, project: i64) -> Result<ProjectArchive, ApiError> {
    let mut conn = pool.acquire().await?;
    let row = sqlx::query("SELECT name, next_label_id, next_job_number FROM projects WHERE id = ?").bind(project).fetch_optional(&mut *conn).await?.ok_or(ApiError::NotFound)?;
    let labels = labels::project_labels(&mut conn, project).await?;
    let jobs = sqlx::query("SELECT jobs.number, users.username FROM jobs LEFT JOIN users ON users.id = jobs.assignee_id WHERE jobs.project_id = ? ORDER BY jobs.number")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|row| ArchiveJob { number: row.get("number"), assignee: row.get("username") })
        .collect();
    let mut annotations: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    for row in sqlx::query("SELECT image_id, label_id, shape, source, confidence, attributes, hidden, locked, pending FROM annotations WHERE project_id = ? ORDER BY image_id, position")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?
    {
        annotations.entry(row.get("image_id")).or_default().push(annotation_from_row(&row)?);
    }
    let images = sqlx::query("SELECT images.id, path, hash, width, height, annotated, jobs.number FROM images LEFT JOIN jobs ON jobs.id = images.job_id WHERE images.project_id = ? ORDER BY path")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|row| ArchiveImage {
            path: row.get("path"),
            hash: row.get("hash"),
            width: row.get("width"),
            height: row.get("height"),
            annotated: row.get("annotated"),
            job: row.get("number"),
            file: None,
            annotations: annotations.remove(&row.get::<i64, _>("id")).unwrap_or_default(),
        })
        .collect();
    Ok(ProjectArchive { name: row.get("name"), next_label_id: row.get("next_label_id"), next_job_number: row.get("next_job_number"), labels, jobs, images })
}

/// The project as an archive file to download, with the image files if asked. Images whose file
/// is gone are referred to only.
async fn export_archive(_: Admin, State(state): State<AppState>, UrlPath(project): UrlPath<i64>, Query(query): Query<ExportQuery>) -> Result<impl IntoResponse, ApiError> {
    let mut archive = project_archive(&state.pool, project).await?;
    let name = archive.name.clone();
    let root = images::image_root(&state)?;
    let folder = state.export_dir.clone();
    let file = tokio::task::spawn_blocking(move || -> Result<fs::File, ApiError> {
        fs::create_dir_all(&folder).map_err(internal)?;
        // A file with no name, gone once the download is sent.
        let mut writer = ArchiveWriter::new(tempfile::tempfile_in(&folder).map_err(internal)?);
        if query.images {
            // Copies with the same content share one file in the archive.
            let mut added: BTreeMap<String, String> = BTreeMap::new();
            for image in &mut archive.images {
                if let Some(name) = added.get(&image.hash) {
                    image.file = Some(name.clone());
                    continue;
                }
                let path = root.join(&image.path);
                let Ok(mut file) = fs::File::open(&path) else { continue };
                let extension = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "img".into());
                let name = writer.add_image(&image.hash, &extension, &mut file).map_err(internal)?;
                added.insert(image.hash.clone(), name.clone());
                image.file = Some(name);
            }
        }
        let mut file = writer.finish(&archive).map_err(internal)?;
        file.seek(SeekFrom::Start(0)).map_err(internal)?;
        Ok(file)
    })
    .await
    .map_err(internal)??;
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file)));
    Ok(([(header::CONTENT_TYPE, "application/zip".to_owned()), (header::CONTENT_DISPOSITION, attachment(&format!("{name}.oxilabel.zip")))], body))
}

/// A download's Content-Disposition: an ASCII name for every browser and the real one (RFC 5987)
/// for those that read it.
fn attachment(name: &str) -> String {
    let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect();
    let encoded: String = name
        .bytes()
        .map(|byte| if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') { (byte as char).to_string() } else { format!("%{byte:02X}") })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// An image to add, with the annotations and job the archive gives it.
struct Found {
    image: NewImage,
    archived: ArchiveImage,
}

/// Imports an archive (the `file` part, written to disk as it arrives) as a new project.
async fn import_archive(_: Admin, State(state): State<AppState>, caller: Caller, mut multipart: Multipart) -> Result<(StatusCode, Json<ArchiveImportReport>), ApiError> {
    let unreadable = |err: axum::extract::multipart::MultipartError| ApiError::BadRequest(format!("unreadable upload: {err}"));
    fs::create_dir_all(&state.upload_dir).map_err(internal)?;
    let (mut upload, mut plan) = (None, ArchiveImportPlan::default());
    while let Some(mut field) = multipart.next_field().await.map_err(unreadable)? {
        match field.name() {
            Some("file") => {
                let mut file = tokio::fs::File::from_std(tempfile::tempfile_in(&state.upload_dir).map_err(internal)?);
                while let Some(chunk) = field.chunk().await.map_err(unreadable)? {
                    tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await.map_err(internal)?;
                }
                upload = Some(file.into_std().await);
            }
            Some("plan") => {
                let text = field.text().await.map_err(unreadable)?;
                plan = serde_json::from_str(&text).map_err(|err| ApiError::BadRequest(format!("invalid plan: {err}")))?;
            }
            _ => {}
        }
    }
    let mut upload = upload.ok_or_else(|| ApiError::BadRequest("choose a project archive to import".into()))?;
    upload.seek(SeekFrom::Start(0)).map_err(internal)?;
    let root = images::image_root(&state)?;
    let folder = match plan.images_folder.as_deref().map(str::trim).filter(|folder| !folder.is_empty()) {
        Some(folder) => Some(images::resolve_for(caller, &root, folder)?),
        None => None,
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |age| age.as_millis());
    let kept = state.upload_dir.join(format!("archive-{stamp}"));
    let result = import_into(&state, caller, upload, plan, root, folder, &kept).await;
    // The archive's copies stay only with a project that uses them.
    if result.is_err() {
        fs::remove_dir_all(&kept).ok();
    }
    result
}

async fn import_into(
    state: &AppState,
    caller: Caller,
    upload: fs::File,
    plan: ArchiveImportPlan,
    root: PathBuf,
    folder: Option<PathBuf>,
    kept: &Path,
) -> Result<(StatusCode, Json<ArchiveImportReport>), ApiError> {
    let kept = kept.to_owned();

    // Reading the archive and the image files takes a while: before the transaction, so saves
    // elsewhere never wait for it.
    let (archive, found, missing) = tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
        let mut reader = ArchiveReader::open(upload).map_err(ApiError::BadRequest)?;
        let archive = reader.project().map_err(ApiError::BadRequest)?;
        let wanted: BTreeSet<String> = archive.images.iter().map(|image| file_name(&image.path).to_lowercase()).collect();
        let in_folder = folder.as_deref().map(|folder| images::images_named(folder, &wanted)).unwrap_or_default();
        let (mut found, mut missing) = (Vec::<Found>::new(), Vec::new());
        for archived in &archive.images {
            match locate(&mut reader, archived, &root, caller.local, &kept, &in_folder)? {
                // Two archived images found as one file are imported once.
                Some(image) if !found.iter().any(|other| other.image.path == image.path) => found.push(Found { image, archived: archived.clone() }),
                _ => missing.push(archived.path.clone()),
            }
        }
        Ok((archive, found, missing))
    })
    .await
    .map_err(internal)??;

    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let name = [plan.name.unwrap_or_default(), archive.name.clone()]
        .into_iter()
        .map(|name| name.trim().to_owned())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "Imported project".to_owned());
    // Never below an id in use, whatever the archive says.
    let next_label_id = archive.labels.iter().map(|label| label.id + 1).chain([archive.next_label_id]).max().unwrap_or(1);
    let next_job_number = archive.jobs.iter().map(|job| job.number + 1).chain([archive.next_job_number]).max().unwrap_or(1);
    let project: i64 = sqlx::query_scalar("INSERT INTO projects (name, next_label_id, next_job_number) VALUES (?, ?, ?) RETURNING id")
        .bind(&name)
        .bind(next_label_id)
        .bind(next_job_number)
        .fetch_one(&mut *tx)
        .await?;
    for label in &archive.labels {
        labels::store_label(&mut tx, project, label).await?;
    }
    let mut jobs = BTreeMap::new();
    let mut unassigned_jobs = Vec::new();
    for job in &archive.jobs {
        let assignee: Option<i64> = match &job.assignee {
            Some(username) => sqlx::query_scalar("SELECT id FROM users WHERE username = ?").bind(username).fetch_optional(&mut *tx).await?,
            None => None,
        };
        if job.assignee.is_some() && assignee.is_none() {
            unassigned_jobs.push(job.number);
        }
        let id: i64 = sqlx::query_scalar("INSERT INTO jobs (project_id, number, assignee_id) VALUES (?, ?, ?) RETURNING id")
            .bind(project)
            .bind(job.number)
            .bind(assignee)
            .fetch_one(&mut *tx)
            .await?;
        jobs.insert(job.number, id);
    }
    let (imported, mut annotations, mut skipped) = (found.len(), 0, Vec::<SkipCount>::new());
    for Found { image, archived } in found {
        let id = images::add_images(&mut tx, project, std::slice::from_ref(&image)).await?[0];
        sqlx::query("UPDATE images SET annotated = ?, job_id = ? WHERE id = ?")
            .bind(archived.annotated)
            .bind(archived.job.and_then(|number| jobs.get(&number)))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let written = annotation_store::write(&mut tx, project, id, Replacing::All, archived.annotations)
            .await?
            .ok_or_else(|| ApiError::Internal("an image just added is not in its project".into()))?;
        annotations += written.stored;
        for skip in written.skipped {
            SkipCount::tally(&mut skipped, skip.reason, skip.count);
        }
    }
    tx.commit().await?;
    let report = ArchiveImportReport { project: Project { id: project, name }, images: imported, missing, annotations, skipped, unassigned_jobs };
    Ok((StatusCode::CREATED, Json(report)))
}

/// The file name of a path with `/` or `\\` separators.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The image's file on this server, read, if its content is the archived one: the archive's copy
/// (kept under `kept`), the file at its path, or a file of that name in the images folder.
fn locate<R: std::io::Read + Seek>(
    reader: &mut ArchiveReader<R>,
    archived: &ArchiveImage,
    root: &Path,
    local: bool,
    kept: &Path,
    in_folder: &BTreeMap<(String, u32, u32), Vec<PathBuf>>,
) -> Result<Option<NewImage>, ApiError> {
    let matching = |file: &Path| images::read_new_image(root, file).ok().filter(|image| image.hash == archived.hash);
    if let Some(name) = &archived.file {
        let base = Some(file_name(&archived.path)).filter(|base| !base.is_empty() && *base != "." && *base != "..").unwrap_or("image");
        let target = images::free_name(&kept.join(base));
        fs::create_dir_all(kept).map_err(internal)?;
        let mut out = fs::File::create(&target).map_err(internal)?;
        let copied = std::io::copy(&mut reader.image(name).map_err(ApiError::BadRequest)?.take(IMAGE_LIMIT + 1), &mut out).map_err(internal)?;
        if copied > IMAGE_LIMIT {
            return Err(ApiError::BadRequest(format!("the archive's copy of {} is larger than any image may be", archived.path)));
        }
        let target = target.canonicalize().map_err(internal)?;
        return match matching(&target) {
            Some(image) => Ok(Some(image)),
            None => {
                fs::remove_file(&target).ok();
                Err(ApiError::BadRequest(format!("the archive's copy of {} is damaged", archived.path)))
            }
        };
    }
    // Absolute paths only from the server's own machine (ADR 0003).
    let at_path = Path::new(&archived.path);
    if !at_path.is_absolute() || local {
        let file = projects_path(root, at_path);
        if let Some(image) = file.and_then(|file| matching(&file)) {
            return Ok(Some(image));
        }
    }
    let key = (file_name(&archived.path).to_lowercase(), archived.width, archived.height);
    Ok(in_folder.get(&key).into_iter().flatten().find_map(|file| matching(file)))
}

/// Where a stored image path points: inside the image root for a relative one (never outside it).
fn projects_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let file = root.join(path).canonicalize().ok()?;
    (path.is_absolute() || file.starts_with(root)).then_some(file)
}
