//! Importing a CVAT "for images 1.1" export (the XML, or the zip CVAT downloads) into a project.
//! Export images are matched to the project's by file name (folders ignored) and size; CVAT labels
//! map to project labels, new labels, or nothing, as the plan says. The same request previews the
//! import, writing nothing, and with `commit` carries it out in one transaction.

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
    Label, LabelDefinition, ShapeType,
    annotation::Annotation,
    attribute::{AttributeKind, AttributeValue, Attributes},
    import::{CvatImportPlan, CvatImportReport, CvatLabelReport, LabelChoice, SkipCount},
};
use oxilabel_formats::cvat::{CvatAnnotation, CvatExport, CvatLabel, parse_cvat};
use sqlx::{Row, SqliteConnection};

use crate::{
    AppState,
    annotation_store::{self, Replacing},
    auth::Admin,
    error::ApiError,
    host::Caller,
    images::{self, NewImage},
    labels, projects,
};

/// CVAT exports of a few thousand images run to tens of megabytes of XML.
const EXPORT_LIMIT: usize = 200 * 1024 * 1024;
/// The color of labels used in the export's shapes but missing from its label list.
const UNLISTED_COLOR: &str = "#7c8a99";

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
        let report = run(&mut tx, project, &export, &plan, found).await?;
        tx.commit().await?;
        Ok(Json(report))
    } else {
        Ok(Json(run(&mut conn, project, &export, &plan, found).await?))
    }
}

/// Export images the project lacks that the images folder has, by lower-case file name and size:
/// read and ready to add when committing, otherwise only found.
type Found = BTreeMap<(String, u32, u32), Option<NewImage>>;

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

/// The project's images by lower-case file name.
async fn project_images(conn: &mut SqliteConnection, project: i64) -> Result<BTreeMap<String, Vec<ProjectImage>>, ApiError> {
    let mut by_name: BTreeMap<String, Vec<ProjectImage>> = BTreeMap::new();
    let rows = sqlx::query("SELECT id, path, width, height, (SELECT COUNT(*) FROM annotations WHERE image_id = images.id) AS annotations FROM images WHERE project_id = ?")
        .bind(project)
        .fetch_all(&mut *conn)
        .await?;
    for row in rows {
        let path: String = row.get("path");
        let image = ProjectImage { id: row.get("id"), width: row.get("width"), height: row.get("height"), annotations: row.get("annotations") };
        by_name.entry(file_name(&path).to_lowercase()).or_default().push(image);
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

/// A project image an export image may match.
struct ProjectImage {
    id: i64,
    width: u32,
    height: u32,
    annotations: i64,
}

async fn run(conn: &mut SqliteConnection, project: i64, export: &CvatExport, plan: &CvatImportPlan, mut found: Found) -> Result<CvatImportReport, ApiError> {
    let existing = labels::project_labels(conn, project).await?;

    // Every CVAT label: the listed ones, then any the shapes use without the list naming them.
    let mut cvat_labels: Vec<CvatLabel> = export.labels.clone();
    for annotation in export.images.iter().flat_map(|image| &image.annotations) {
        if !cvat_labels.iter().any(|label| label.name == annotation.label) {
            cvat_labels.push(CvatLabel { name: annotation.label.clone(), color: None, shape_types: ShapeType::ALL.to_vec(), attributes: Vec::new() });
        }
    }

    // What each CVAT label becomes. New labels are checked against the project's and each other.
    let mut reports = Vec::new();
    let mut targets: BTreeMap<String, Option<Label>> = BTreeMap::new();
    let mut taken = existing.clone();
    for (index, cvat_label) in cvat_labels.iter().enumerate() {
        let definition = definition_of(cvat_label);
        // Previews use the definition as it will be stored (trimmed names and options).
        let (definition, problem) = match definition.clone().validated(&taken) {
            Ok(valid) => (valid, None),
            Err(problem) => (definition, Some(problem)),
        };
        let same_name = existing.iter().find(|label| label.name.eq_ignore_ascii_case(&cvat_label.name));
        let suggestion = match (same_name, &problem) {
            (Some(label), _) => LabelChoice::Map { label: label.id },
            (None, None) => LabelChoice::Create,
            (None, Some(_)) => LabelChoice::Skip,
        };
        let choice = plan.labels.get(&cvat_label.name).copied().unwrap_or(suggestion);
        let target = match choice {
            LabelChoice::Map { label } => Some(
                existing
                    .iter()
                    .find(|existing| existing.id == label)
                    .cloned()
                    .ok_or_else(|| ApiError::BadRequest(format!("this project has no label {label} for \"{}\"", cvat_label.name)))?,
            ),
            LabelChoice::Create if plan.commit => Some(labels::insert_label(conn, project, definition).await.map_err(|err| match err {
                ApiError::BadRequest(reason) => ApiError::BadRequest(format!("cannot create label \"{}\": {reason}", cvat_label.name)),
                other => other,
            })?),
            // Previewed, it stands for the label it would become, under an id no label has.
            LabelChoice::Create => Some(Label::new(-1 - index as i64, definition)),
            LabelChoice::Skip => None,
        };
        if let (LabelChoice::Create, Some(label)) = (choice, &target) {
            taken.push(label.clone());
        }
        let count = export.images.iter().flat_map(|image| &image.annotations).filter(|a| a.label == cvat_label.name).count();
        // A problem matters only for a label to be created.
        let problem = problem.filter(|_| choice == LabelChoice::Create);
        reports.push(CvatLabelReport { name: cvat_label.name.clone(), color: cvat_label.color.clone(), annotations: count, choice, problem });
        targets.insert(cvat_label.name.clone(), target);
    }

    let by_name = project_images(conn, project).await?;

    let mut report = CvatImportReport {
        labels: reports,
        images: export.images.len(),
        matched: 0,
        added: 0,
        unmatched: Vec::new(),
        ambiguous: Vec::new(),
        size_mismatch: Vec::new(),
        annotated: 0,
        annotations: 0,
        skipped: Vec::new(),
        committed: plan.commit,
    };
    let mut skipped = Vec::new();
    for cvat_image in &export.images {
        // Same file name (ignoring case), then same size; failing that, an image found in the
        // images folder, added now.
        let key = (file_name(&cvat_image.name).to_lowercase(), cvat_image.width, cvat_image.height);
        let named = by_name.get(&key.0).map(Vec::as_slice).unwrap_or_default();
        let sized: Vec<&ProjectImage> = named.iter().filter(|image| (image.width, image.height) == (cvat_image.width, cvat_image.height)).collect();
        let added;
        let image = match (named, &sized[..]) {
            (_, []) if let Some(new) = found.remove(&key) => {
                let id = match new {
                    Some(new) => images::add_images(conn, project, &[new]).await?[0],
                    // Previewed, it stands for the image it would become, under an id no image has.
                    None => -1,
                };
                report.added += 1;
                added = ProjectImage { id, width: cvat_image.width, height: cvat_image.height, annotations: 0 };
                &added
            }
            ([], _) => {
                report.unmatched.push(cvat_image.name.clone());
                continue;
            }
            (_, [image]) => *image,
            (_, []) => {
                report.size_mismatch.push(cvat_image.name.clone());
                continue;
            }
            _ => {
                report.ambiguous.push(cvat_image.name.clone());
                continue;
            }
        };
        report.matched += 1;
        if image.annotations > 0 {
            report.annotated += 1;
        }
        for kind in &cvat_image.unsupported {
            SkipCount::tally(&mut skipped, format!("{kind} shapes are not supported"), 1);
        }
        // Each annotation with the attribute values it leaves out.
        let (mut added, mut dropped_values) = (Vec::new(), Vec::new());
        for cvat_annotation in &cvat_image.annotations {
            let Some(label) = targets.get(&cvat_annotation.label).and_then(Option::as_ref) else {
                SkipCount::tally(&mut skipped, format!("label \"{}\" is skipped", cvat_annotation.label), 1);
                continue;
            };
            let (attributes, dropped) = attributes_of(cvat_annotation, label);
            added.push(Annotation { attributes, ..Annotation::manual(label.id, cvat_annotation.shape.clone()) });
            dropped_values.push(dropped);
        }
        // The store leaves out what does not fit, and tags already on the image.
        let replacing = if plan.replace { Replacing::All } else { Replacing::Nothing };
        let written = if plan.commit {
            annotation_store::write(conn, project, image.id, replacing, added)
                .await?
                .ok_or_else(|| ApiError::Internal("a matched image is not in its project".into()))?
        } else {
            annotation_store::preview(conn, image.id, (image.width, image.height), &taken, replacing, added).await?
        };
        report.annotations += written.stored;
        // A shape left out does not also count its attribute values.
        for (index, dropped) in dropped_values.into_iter().enumerate() {
            if written.left_out.binary_search(&index).is_err() {
                for reason in dropped {
                    SkipCount::tally(&mut skipped, reason.to_owned(), 1);
                }
            }
        }
        for skip in written.skipped {
            SkipCount::tally(&mut skipped, skip.reason, skip.count);
        }
    }
    report.skipped = skipped;
    Ok(report)
}

/// The OxiLabel label a CVAT label becomes: its name, color, allowed shapes and attributes.
fn definition_of(label: &CvatLabel) -> LabelDefinition {
    LabelDefinition {
        name: label.name.clone(),
        color: label.color.clone().unwrap_or_else(|| UNLISTED_COLOR.to_owned()),
        hotkey: None,
        shape_types: label.shape_types.clone(),
        ignore: false,
        attributes: label.attributes.clone(),
        footprint: oxilabel_core::Footprint::default(),
        show_footprint: true,
        constraints: Default::default(),
    }
}

/// The CVAT attribute values that `label` has an attribute for and that fit it, and why each of
/// the others is left out.
fn attributes_of(annotation: &CvatAnnotation, label: &Label) -> (Attributes, Vec<&'static str>) {
    let (mut values, mut dropped) = (Attributes::new(), Vec::new());
    for (name, text) in &annotation.attributes {
        let Some(definition) = label.attributes.iter().find(|definition| &definition.name == name) else {
            dropped.push("attribute values with no matching attribute");
            continue;
        };
        let value = match &definition.kind {
            AttributeKind::Checkbox { .. } => match text.trim().to_ascii_lowercase().as_str() {
                "true" => Some(AttributeValue::Bool(true)),
                "false" => Some(AttributeValue::Bool(false)),
                _ => None,
            },
            AttributeKind::Number { .. } => text.trim().parse().ok().map(AttributeValue::Number),
            AttributeKind::Select { .. } => Some(AttributeValue::Text(text.trim().to_owned())),
            AttributeKind::Text { .. } => Some(AttributeValue::Text(text.clone())),
        };
        match value.filter(|value| definition.check(value).is_ok()) {
            Some(value) => {
                values.insert(name.clone(), value);
            }
            None => dropped.push("attribute values that do not fit their attribute"),
        }
    }
    (values, dropped)
}
