//! Importing a CVAT "for images 1.1" export (the XML, or the zip CVAT downloads) into a project.
//! Export images are matched to the project's by file name (folders ignored) and size; CVAT labels
//! map to project labels, new labels, or nothing, as the plan says. The plan is pure (see
//! `cvat_plan`): the same request previews it, writing nothing, and with `commit` applies it in one
//! transaction.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    routing::post,
};
use oxilabel_core::{
    annotation::Annotation,
    import::{CvatImportPlan, CvatImportReport},
};
use oxilabel_formats::cvat::{CvatExport, parse_cvat};
use sqlx::{Row, SqliteConnection};

use crate::{
    AppState,
    annotation_store::{self, Replacing},
    auth::Admin,
    cvat_plan::{self, CvatPlan, Found, ImageTarget, LabelRef, ProjectImage},
    error::ApiError,
    host::Caller,
    images, labels, projects,
};

/// CVAT exports of a few thousand images run to tens of megabytes of XML.
const EXPORT_LIMIT: usize = 200 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/projects/{project}/cvat-imports", post(import).layer(DefaultBodyLimit::max(EXPORT_LIMIT)))
}

/// Reads the `file` part (the export) and the `plan` part (JSON, optional) and previews or runs
/// the import.
async fn import(
    _: Admin,
    State(state): State<AppState>,
    caller: Caller,
    Path(project): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<CvatImportReport>, ApiError> {
    let pool = &state.pool;
    let unreadable = |err: axum::extract::multipart::MultipartError| ApiError::BadRequest(format!("unreadable upload: {err}"));
    let (mut file, mut plan) = (None, CvatImportPlan::default());
    while let Some(field) = multipart.next_field().await.map_err(unreadable)? {
        match field.name() {
            Some("file") => file = Some(field.bytes().await.map_err(unreadable)?),
            Some("plan") => {
                let text = field.text().await.map_err(unreadable)?;
                plan = serde_json::from_str(&text).map_err(|err| ApiError::BadRequest(format!("invalid plan: {err}")))?;
            }
            _ => {}
        }
    }
    let file = file.ok_or_else(|| ApiError::BadRequest("choose a CVAT export to import".into()))?;
    let export = parse_cvat(&export_xml(&file)?).map_err(ApiError::BadRequest)?;
    let mut conn = pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let found = match plan.images_folder.as_deref().map(str::trim).filter(|folder| !folder.is_empty()) {
        Some(folder) => find_missing(&state, caller, &mut conn, project, &export, folder, plan.commit).await?,
        None => Found::new(),
    };
    if plan.commit {
        drop(conn);
        // Takes the write lock at once, so a save meanwhile waits instead of failing midway.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let planned = make_plan(&mut tx, project, &export, &plan, found).await?;
        let report = commit(&mut tx, project, planned).await?;
        tx.commit().await?;
        Ok(Json(report))
    } else {
        Ok(Json(make_plan(&mut conn, project, &export, &plan, found).await?.report))
    }
}

/// Plans the import against the project as it is in `conn`: the labels, the images (with their
/// tags, which appends must not repeat) and the export.
async fn make_plan(conn: &mut SqliteConnection, project: i64, export: &CvatExport, choices: &CvatImportPlan, found: Found) -> Result<CvatPlan, ApiError> {
    let existing = labels::project_labels(conn, project).await?;
    let images = project_images(conn, project).await?;
    cvat_plan::plan(export, &existing, &images, found, choices).map_err(ApiError::BadRequest)
}

/// Applies a plan: new labels, then new images, then every image's planned annotations through the
/// annotation store. The store must store exactly what the plan says, or the transaction fails.
async fn commit(conn: &mut SqliteConnection, project: i64, planned: CvatPlan) -> Result<CvatImportReport, ApiError> {
    let CvatPlan { report, new_labels, images: planned_images, replace } = planned;
    let mut label_ids = Vec::with_capacity(new_labels.len());
    for definition in new_labels {
        let name = definition.name.clone();
        let label = labels::insert_label(conn, project, definition).await.map_err(|err| match err {
            ApiError::BadRequest(reason) => ApiError::BadRequest(format!("cannot create label \"{name}\": {reason}")),
            other => other,
        })?;
        label_ids.push(label.id);
    }
    let mut image_ids = Vec::with_capacity(planned_images.len());
    for planned in &planned_images {
        match &planned.target {
            ImageTarget::Existing { id } => image_ids.push(*id),
            ImageTarget::New { image: Some(image), .. } => image_ids.push(images::add_images(conn, project, std::slice::from_ref(image)).await?[0]),
            ImageTarget::New { key, image: None } => return Err(ApiError::Internal(format!("a committing CVAT plan has an unread image: {}", key.0))),
        }
    }
    for (planned, image) in planned_images.into_iter().zip(image_ids) {
        let mut annotations = Vec::with_capacity(planned.annotations.len());
        for planned in planned.annotations {
            let label = match planned.label {
                LabelRef::Existing(id) => id,
                LabelRef::New(index) => label_ids[index],
            };
            let mut annotation = Annotation::manual(label, planned.shape);
            annotation.attributes = planned.attributes;
            annotations.push(annotation);
        }
        let replacing = if replace { Replacing::All } else { Replacing::Nothing };
        let expected = annotations.len();
        let written = annotation_store::write(conn, project, image, replacing, annotations)
            .await?
            .ok_or_else(|| ApiError::Internal("a matched image is not in its project".into()))?;
        if written.stored != expected {
            return Err(ApiError::Internal("the CVAT import plan and annotation store disagree".into()));
        }
    }
    Ok(report)
}

/// Looks in `folder` for the export images that match no project image. This happens before the
/// import's transaction, so annotators' saves never wait for files being read.
async fn find_missing(state: &AppState, caller: Caller, conn: &mut SqliteConnection, project: i64, export: &CvatExport, folder: &str, read: bool) -> Result<Found, ApiError> {
    let root = images::image_root(state)?;
    let folder = images::resolve_for(caller, &root, folder)?;
    if !folder.is_dir() {
        return Err(ApiError::BadRequest("the images folder is not a folder".into()));
    }
    let present = project_images(conn, project).await?;
    let missing: BTreeSet<(String, u32, u32)> = export
        .images
        .iter()
        .map(|image| (file_name(&image.name).to_lowercase(), image.width, image.height))
        .filter(|(name, width, height)| !present.get(name).is_some_and(|images| images.iter().any(|image| (image.width, image.height) == (*width, *height))))
        .collect();
    let names = missing.iter().map(|(name, _, _)| name.clone()).collect();
    tokio::task::spawn_blocking(move || {
        let files = images::images_named(&folder, &names);
        let mut found = Found::new();
        for key in missing {
            // Copies with the same name and size are taken to be the same image: the first, by path.
            let Some(file) = files.get(&key).and_then(|files| files.first()) else { continue };
            let image = if read { Some(images::read_new_image(&root, file).map_err(|reason| ApiError::BadRequest(format!("cannot read {}: {reason}", file.display())))?) } else { None };
            found.insert(key, image);
        }
        Ok(found)
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
}

/// The project's images by lower-case file name, each with the labels it has as tags.
async fn project_images(conn: &mut SqliteConnection, project: i64) -> Result<BTreeMap<String, Vec<ProjectImage>>, ApiError> {
    let mut by_name: BTreeMap<String, Vec<ProjectImage>> = BTreeMap::new();
    let rows = sqlx::query("SELECT id, path, width, height, (SELECT COUNT(*) FROM annotations WHERE image_id = images.id) AS annotations FROM images WHERE project_id = ?")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    for row in rows {
        let path: String = row.get("path");
        let image = ProjectImage { id: row.get("id"), width: row.get("width"), height: row.get("height"), annotations: row.get("annotations"), tags: BTreeSet::new() };
        by_name.entry(file_name(&path).to_lowercase()).or_default().push(image);
    }
    let tags = sqlx::query("SELECT image_id, label_id FROM annotations WHERE project_id = ? AND json_extract(shape, '$.type') = 'tag'")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    for row in tags {
        let image: i64 = row.get("image_id");
        if let Some(image) = by_name.values_mut().flatten().find(|candidate| candidate.id == image) {
            image.tags.insert(row.get("label_id"));
        }
    }
    Ok(by_name)
}

/// The XML of an export: the file itself, or the XML file inside a zip (its `annotations.xml`,
/// in whatever folder, if it has one).
fn export_xml(file: &[u8]) -> Result<Cow<'_, str>, ApiError> {
    let not_text = || ApiError::BadRequest("the export is not UTF-8 text".into());
    if !file.starts_with(b"PK") {
        return std::str::from_utf8(file).map(Cow::Borrowed).map_err(|_| not_text());
    }
    let unreadable = |err: &dyn std::fmt::Display| ApiError::BadRequest(format!("unreadable zip: {err}"));
    let mut zip = zip::ZipArchive::new(Cursor::new(file)).map_err(|err| unreadable(&err))?;
    let name = zip
        .file_names()
        // macOS archivers add resource forks under __MACOSX.
        .filter(|name| name.to_ascii_lowercase().ends_with(".xml") && !name.starts_with("__MACOSX/"))
        .min_by_key(|name| !file_name(name).eq_ignore_ascii_case("annotations.xml"))
        .map(str::to_owned)
        .ok_or_else(|| ApiError::BadRequest("the zip holds no XML file".into()))?;
    // Read no more than an uploaded XML may be, whatever the zip claims.
    let mut bytes = Vec::new();
    let entry = zip.by_name(&name).map_err(|err| unreadable(&err))?;
    entry.take(EXPORT_LIMIT as u64 + 1).read_to_end(&mut bytes).map_err(|err| unreadable(&err))?;
    if bytes.len() > EXPORT_LIMIT {
        return Err(ApiError::BadRequest(format!("{name} in the zip is larger than {} MB", EXPORT_LIMIT / (1024 * 1024))));
    }
    String::from_utf8(bytes).map(Cow::Owned).map_err(|_| not_text())
}

/// The file name of a path with `/` or `\` separators.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}