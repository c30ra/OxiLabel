//! The Annotation store: every change to an image's set of annotations goes through here, so its
//! rules hold for every writer. Each annotation must fit its label and the image, an image has at
//! most one copy of each tag, the stored order is the drawing order, and every change bumps the
//! image's version so an editor holding the old one reloads. Only an editor's save sets the
//! image's annotated status. The store works inside the caller's transaction.

use std::{collections::BTreeSet, sync::LazyLock};

use oxilabel_core::{
    Label, ShapeType,
    annotation::{Annotation, ImageAnnotations, counts_as_annotated},
    attribute::Attributes,
    import::SkipCount,
};
use sqlx::{
    Row, Sqlite, SqliteConnection,
    query::Query,
    sqlite::{SqliteArguments, SqliteRow},
};

use crate::{error::ApiError, labels, placeholders};

/// The image's annotations in drawing order, with its version and annotated status; `NotFound`
/// if the image is not in the project.
pub(crate) async fn load(conn: &mut SqliteConnection, project: i64, image: i64) -> Result<ImageAnnotations, ApiError> {
    let row = sqlx::query("SELECT version, annotated FROM images WHERE id = ? AND project_id = ?")
        .bind(image)
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApiError::NotFound)?;
    let rows = sqlx::query(&LOAD)
        .bind(image)
        .fetch_all(&mut *conn)
        .await?;
    let annotations = rows.iter().map(annotation_from_row).collect::<Result<_, ApiError>>()?;
    Ok(ImageAnnotations { version: row.get("version"), annotations, annotated: row.get("annotated") })
}

/// Replaces the image's annotations with an editor's, if the image is still at `base_version`
/// (else `Conflict`), and sets its annotated status from them. The whole set is refused if one
/// annotation does not fit its label or the image, or repeats a tag.
pub(crate) async fn save(conn: &mut SqliteConnection, project: i64, image: i64, base_version: i64, annotations: Vec<Annotation>) -> Result<ImageAnnotations, ApiError> {
    let (width, height, current) = image_row(conn, project, image).await?.ok_or(ApiError::NotFound)?;
    // A stale editor is told to reload first: that also resolves labels deleted meanwhile.
    if current != base_version {
        return Err(stale(current));
    }
    let labels = labels::project_labels(conn, project).await?;
    let (fitting, refused) = sort_out(&labels, width, height, BTreeSet::new(), annotations);
    if let Some((index, reason)) = refused.into_iter().next() {
        return Err(ApiError::BadRequest(format!("annotation {}: {reason}", index + 1)));
    }
    // Checking and bumping the version in one statement keeps concurrent saves safe. The status
    // follows the work: the first reviewed annotation marks the image, removing the last unmarks it.
    let annotated = counts_as_annotated(&fitting);
    let bumped = sqlx::query("UPDATE images SET version = version + 1, annotated = ? WHERE id = ? AND version = ?")
        .bind(annotated)
        .bind(image)
        .bind(base_version)
        .execute(&mut *conn)
        .await?;
    if bumped.rows_affected() == 0 {
        let (_, _, current) = image_row(conn, project, image).await?.ok_or(ApiError::NotFound)?;
        return Err(stale(current));
    }
    remove(conn, image, Replacing::All).await?;
    insert_annotations(conn, project, image, 0, &fitting).await?;
    Ok(ImageAnnotations { version: base_version + 1, annotations: fitting, annotated })
}

/// Which of the image's stored annotations a write takes the place of.
#[derive(Clone, Copy)]
pub(crate) enum Replacing<'a> {
    /// All of them.
    All,
    /// None: the write adds to them.
    Nothing,
    /// The unreviewed pre-annotations of the model named `source`.
    Pending { source: &'a str },
}

/// What a write stored, and what it left out by reason.
pub(crate) struct Written {
    pub stored: usize,
    pub skipped: Vec<SkipCount>,
    /// The positions, in the list given, of the annotations left out, in order.
    pub left_out: Vec<usize>,
}

/// Puts `annotations` on the image in place of `replacing`, on top of the annotations it keeps.
/// Annotations that do not fit their label or the image, and tags the image already has, are
/// left out and counted by reason. The annotated status stays as it is. `None` if the image is not
/// in the project.
pub(crate) async fn write(conn: &mut SqliteConnection, project: i64, image: i64, replacing: Replacing<'_>, annotations: Vec<Annotation>) -> Result<Option<Written>, ApiError> {
    let Some((width, height, _)) = image_row(conn, project, image).await? else {
        return Ok(None);
    };
    let tagged = kept_tags(conn, image, replacing).await?;
    let removed = remove(conn, image, replacing).await?;
    let first: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM annotations WHERE image_id = ?").bind(image).fetch_one(&mut *conn).await?;
    let labels = labels::project_labels(conn, project).await?;
    let (fitting, refused) = sort_out(&labels, width, height, tagged, annotations);
    insert_annotations(conn, project, image, first, &fitting).await?;
    if removed > 0 || !fitting.is_empty() {
        // An editor holding the old version is told to reload.
        bump_version(conn, image).await?;
    }
    Ok(Some(written(fitting.len(), refused)))
}

/// What `write` would store and leave out, writing nothing: for previews of imports whose labels
/// or images do not exist yet. `labels` stand for the project's and `width` × `height` for the
/// image's size; an `image` id that no image has stands for a new image.
pub(crate) async fn preview(
    conn: &mut SqliteConnection,
    image: i64,
    (width, height): (u32, u32),
    labels: &[Label],
    replacing: Replacing<'_>,
    annotations: Vec<Annotation>,
) -> Result<Written, ApiError> {
    let tagged = kept_tags(conn, image, replacing).await?;
    let (fitting, refused) = sort_out(labels, width, height, tagged, annotations);
    Ok(written(fitting.len(), refused))
}

fn written(stored: usize, refused: Vec<(usize, String)>) -> Written {
    let (mut skipped, mut left_out) = (Vec::new(), Vec::with_capacity(refused.len()));
    for (index, reason) in refused {
        SkipCount::tally(&mut skipped, reason, 1);
        left_out.push(index);
    }
    Written { stored, skipped, left_out }
}

/// The labels the image has as tags among the annotations a write in place of `replacing` keeps.
async fn kept_tags(conn: &mut SqliteConnection, image: i64, replacing: Replacing<'_>) -> Result<BTreeSet<i64>, ApiError> {
    const TAGS: &str = "SELECT label_id FROM annotations WHERE image_id = ? AND json_extract(shape, '$.type') = 'tag'";
    let tags: Vec<i64> = match replacing {
        Replacing::All => Vec::new(),
        Replacing::Nothing => sqlx::query_scalar(TAGS).bind(image).fetch_all(&mut *conn).await?,
        Replacing::Pending { source } => {
            sqlx::query_scalar(&format!("{TAGS} AND NOT (pending AND source IS ?)")).bind(image).bind(source).fetch_all(&mut *conn).await?
        }
    };
    Ok(tags.into_iter().collect())
}

/// Removes the annotations a write takes the place of; how many.
async fn remove(conn: &mut SqliteConnection, image: i64, replacing: Replacing<'_>) -> Result<u64, ApiError> {
    let removed = match replacing {
        Replacing::All => sqlx::query("DELETE FROM annotations WHERE image_id = ?").bind(image).execute(&mut *conn).await?,
        Replacing::Nothing => return Ok(0),
        Replacing::Pending { source } => {
            sqlx::query("DELETE FROM annotations WHERE image_id = ? AND pending AND source IS ?").bind(image).bind(source).execute(&mut *conn).await?
        }
    };
    Ok(removed.rows_affected())
}

async fn bump_version(conn: &mut SqliteConnection, image: i64) -> Result<(), ApiError> {
    sqlx::query("UPDATE images SET version = version + 1 WHERE id = ?").bind(image).execute(&mut *conn).await?;
    Ok(())
}

/// Makes the stored annotations of `label` fit its changed definition: values of removed
/// attributes go, values that no longer fit and missing ones become defaults, and the images whose
/// annotations change get a new version. Refused (`Conflict`, saying how many annotations are in
/// the way) while annotations have a shape type the label no longer allows.
pub(crate) async fn fit_to_label(conn: &mut SqliteConnection, project: i64, label: &Label) -> Result<(), ApiError> {
    let used: Vec<(String, i64)> = sqlx::query_as("SELECT json_extract(shape, '$.type'), COUNT(*) FROM annotations WHERE project_id = ? AND label_id = ? GROUP BY 1")
        .bind(project)
        .bind(label.id)
        .fetch_all(&mut *conn)
        .await?;
    let mut disallowed = Vec::new();
    for (name, count) in used {
        let shape_type: ShapeType = serde_json::from_value(serde_json::Value::String(name)).map_err(|err| ApiError::Internal(format!("stored shapes are unreadable: {err}")))?;
        if !label.shape_types.contains(&shape_type) {
            disallowed.push((shape_type, count));
        }
    }
    if !disallowed.is_empty() {
        disallowed.sort_by_key(|(shape_type, _)| ShapeType::ALL.iter().position(|other| other == shape_type));
        let uses: Vec<String> = disallowed
            .iter()
            .map(|(shape_type, count)| format!("{count} {} with {} shapes", if *count == 1 { "annotation" } else { "annotations" }, shape_type.name().to_lowercase()))
            .collect();
        let (last, rest) = uses.split_last().expect("not empty");
        let uses = if rest.is_empty() { last.clone() } else { format!("{} and {last}", rest.join(", ")) };
        let types = if disallowed.len() == 1 { "that shape type" } else { "those shape types" };
        return Err(ApiError::Conflict(format!("label \"{}\" still has {uses}; delete them before removing {types}", label.name)));
    }

    let rows = sqlx::query("SELECT id, image_id, attributes FROM annotations WHERE project_id = ? AND label_id = ?")
        .bind(project)
        .bind(label.id)
        .fetch_all(&mut *conn)
        .await?;
    let mut changed_images = BTreeSet::new();
    for row in rows {
        let stored: Attributes = serde_json::from_str(row.get("attributes")).unwrap_or_default();
        let conformed = label.conform_attributes(&stored);
        if conformed != stored {
            sqlx::query("UPDATE annotations SET attributes = ? WHERE id = ?")
                .bind(serde_json::to_string(&conformed).expect("attributes serialize"))
                .bind(row.get::<i64, _>("id"))
                .execute(&mut *conn)
                .await?;
            changed_images.insert(row.get::<i64, _>("image_id"));
        }
    }
    // Editors holding the old version reload.
    for image in changed_images {
        bump_version(conn, image).await?;
    }
    Ok(())
}

/// Removes every annotation of `label`, which is being deleted. The images they were on get a new
/// version, so editors holding the old one cannot save them back; the annotated status stays.
pub(crate) async fn remove_label_annotations(conn: &mut SqliteConnection, project: i64, label: i64) -> Result<(), ApiError> {
    sqlx::query("UPDATE images SET version = version + 1 WHERE id IN (SELECT image_id FROM annotations WHERE project_id = ? AND label_id = ?)")
        .bind(project)
        .bind(label)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM annotations WHERE project_id = ? AND label_id = ?").bind(project).bind(label).execute(&mut *conn).await?;
    Ok(())
}

/// The image's size and version, if it is in the project.
async fn image_row(conn: &mut SqliteConnection, project: i64, image: i64) -> Result<Option<(u32, u32, i64)>, ApiError> {
    let row = sqlx::query("SELECT width, height, version FROM images WHERE id = ? AND project_id = ?")
        .bind(image)
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|row| (row.get("width"), row.get("height"), row.get("version"))))
}

/// Splits `annotations` into those that fit and, for the others, their index and why not. Each
/// must fit its label and the `width` × `height` image, and a tag must be neither in `tagged`
/// (labels the image already has as tags) nor earlier in the list.
fn sort_out(labels: &[Label], width: u32, height: u32, mut tagged: BTreeSet<i64>, annotations: Vec<Annotation>) -> (Vec<Annotation>, Vec<(usize, String)>) {
    let mut fitting = Vec::with_capacity(annotations.len());
    let mut refused = Vec::new();
    for (index, annotation) in annotations.into_iter().enumerate() {
        let check = match labels.iter().find(|label| label.id == annotation.label) {
            None => Err(format!("there is no label with id {}", annotation.label)),
            Some(label) => check_annotation(label, &annotation, width, height).and_then(|()| {
                if annotation.shape.is_tag() && !tagged.insert(label.id) {
                    Err(format!("the image already has the tag \"{}\"", label.name))
                } else {
                    Ok(())
                }
            }),
        };
        match check {
            Ok(()) => fitting.push(annotation),
            Err(reason) => refused.push((index, reason)),
        }
    }
    (fitting, refused)
}

/// The columns of an annotation row, in the order `bind_annotation` binds them; `annotation_from_row`
/// reads them.
pub(crate) const ANNOTATION_COLUMNS: &str = "label_id, shape, source, confidence, attributes, hidden, locked, pending";

static LOAD: LazyLock<String> = LazyLock::new(|| format!("SELECT {ANNOTATION_COLUMNS} FROM annotations WHERE image_id = ? ORDER BY position"));
static INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO annotations (image_id, project_id, position, {ANNOTATION_COLUMNS}) VALUES (?, ?, ?, {})", placeholders(ANNOTATION_COLUMNS))
});

/// Reads an annotation from a row with the `ANNOTATION_COLUMNS`.
pub(crate) fn annotation_from_row(row: &SqliteRow) -> Result<Annotation, ApiError> {
    let unreadable = |what: &str, err: serde_json::Error| ApiError::Internal(format!("stored {what} are unreadable: {err}"));
    let shape = serde_json::from_str(row.get("shape")).map_err(|err| unreadable("shapes", err))?;
    let attributes = serde_json::from_str(row.get("attributes")).map_err(|err| unreadable("attributes", err))?;
    Ok(Annotation {
        label: row.get("label_id"),
        shape,
        source: row.get("source"),
        confidence: row.get("confidence"),
        attributes,
        hidden: row.get("hidden"),
        locked: row.get("locked"),
        pending: row.get("pending"),
    })
}

/// Binds the annotation's `ANNOTATION_COLUMNS`.
fn bind_annotation<'q>(query: Query<'q, Sqlite, SqliteArguments<'q>>, annotation: &'q Annotation) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    query
        .bind(annotation.label)
        .bind(serde_json::to_string(&annotation.shape).expect("shapes serialize"))
        .bind(&annotation.source)
        .bind(annotation.confidence)
        .bind(serde_json::to_string(&annotation.attributes).expect("attributes serialize"))
        .bind(annotation.hidden)
        .bind(annotation.locked)
        .bind(annotation.pending)
}

/// Checks that `label` allows the annotation's shape, the shape lies in a `width` × `height`
/// image, and its attribute values suit the label.
fn check_annotation(label: &Label, annotation: &Annotation, width: u32, height: u32) -> Result<(), String> {
    let shape_type = annotation.shape.shape_type();
    if !label.shape_types.contains(&shape_type) {
        let name = serde_json::to_value(shape_type).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        return Err(format!("label \"{}\" does not allow {name} shapes", label.name));
    }
    annotation.shape.validate(width, height)?;
    label.check_attributes(&annotation.attributes)
}

/// Stores `annotations` on the image in order, the first at drawing position `first`.
async fn insert_annotations(conn: &mut SqliteConnection, project: i64, image: i64, first: i64, annotations: &[Annotation]) -> Result<(), ApiError> {
    for (offset, annotation) in annotations.iter().enumerate() {
        let insert = sqlx::query(&INSERT).bind(image).bind(project).bind(first + offset as i64);
        bind_annotation(insert, annotation).execute(&mut *conn).await?;
    }
    Ok(())
}

fn stale(current: i64) -> ApiError {
    ApiError::Conflict(format!(
        "these annotations were changed elsewhere since you opened the image (now version {current}); reload to see them"
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use oxilabel_core::{
        annotation::Shape,
        geometry::{Point, Rect},
    };
    use serde_json::json;
    use sqlx::Connection;

    use super::*;

    const CRACK: i64 = 1;
    const DEFECT: i64 = 2;

    /// A database with project 1, its labels "Crack" (boxes and polygons) and "Defect" (tags),
    /// and one 20 × 10 image; the connection and the image's id.
    async fn image() -> (SqliteConnection, i64) {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON").execute(&mut conn).await.unwrap();
        sqlx::migrate!("./migrations").run(&mut conn).await.unwrap();
        let project: i64 = sqlx::query_scalar("INSERT INTO projects (name) VALUES ('p') RETURNING id").fetch_one(&mut conn).await.unwrap();
        assert_eq!(project, 1);
        for definition in [
            json!({"name": "Crack", "color": "#ff0000", "shape_types": ["rectangle", "polygon"]}),
            json!({"name": "Defect", "color": "#00ff00", "shape_types": ["tag"]}),
        ] {
            ok(labels::insert_label(&mut conn, project, serde_json::from_value(definition).unwrap()).await);
        }
        let image = sqlx::query_scalar("INSERT INTO images (project_id, path, hash, width, height) VALUES (1, 'a.png', 'h', 20, 10) RETURNING id")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        (conn, image)
    }

    fn ok<T>(result: Result<T, ApiError>) -> T {
        result.unwrap_or_else(|err| panic!("{}", err.message()))
    }

    fn refused<T>(result: Result<T, ApiError>) -> (StatusCode, String) {
        match result {
            Ok(_) => panic!("expected a refusal"),
            Err(err) => err.public(),
        }
    }

    fn crack(min_x: f64) -> Annotation {
        Annotation::manual(CRACK, Shape::Rectangle(Rect::new(min_x, 1.0, min_x + 2.0, 3.0)))
    }

    fn tag() -> Annotation {
        Annotation::manual(DEFECT, Shape::Tag)
    }

    #[tokio::test]
    async fn a_save_replaces_the_set_in_order_bumps_the_version_and_sets_the_status() {
        let (mut conn, image) = image().await;
        let saved = ok(save(&mut conn, 1, image, 0, vec![crack(1.0), tag(), crack(5.0)]).await);
        assert_eq!((saved.version, saved.annotated), (1, true));
        assert_eq!(ok(load(&mut conn, 1, image).await), ImageAnnotations { version: 1, annotations: vec![crack(1.0), tag(), crack(5.0)], annotated: true });

        let saved = ok(save(&mut conn, 1, image, 1, vec![crack(7.0)]).await);
        assert_eq!(ok(load(&mut conn, 1, image).await), saved);
        assert_eq!(saved, ImageAnnotations { version: 2, annotations: vec![crack(7.0)], annotated: true });

        // Removing the last reviewed annotation unmarks the image.
        let pending = Annotation { pending: true, source: Some("model".into()), ..crack(1.0) };
        let saved = ok(save(&mut conn, 1, image, 2, vec![pending.clone()]).await);
        assert_eq!(saved, ImageAnnotations { version: 3, annotations: vec![pending], annotated: false });
    }

    #[tokio::test]
    async fn a_save_from_a_stale_version_is_refused_and_changes_nothing() {
        let (mut conn, image) = image().await;
        ok(save(&mut conn, 1, image, 0, vec![crack(1.0)]).await);
        let (status, message) = refused(save(&mut conn, 1, image, 0, vec![]).await);
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(message.contains("now version 1"), "{message}");
        assert_eq!(ok(load(&mut conn, 1, image).await).annotations, [crack(1.0)]);
    }

    #[tokio::test]
    async fn a_save_with_an_annotation_its_label_or_the_image_refuses_is_refused_whole() {
        let (mut conn, image) = image().await;
        let outside = Annotation::manual(CRACK, Shape::Polygon { points: vec![Point::new(0.0, 0.0), Point::new(500.0, 0.0), Point::new(0.0, 5.0)] });
        let cases = [
            (Annotation::manual(DEFECT, Shape::Rectangle(Rect::new(1.0, 1.0, 2.0, 2.0))), "annotation 2: label \"Defect\" does not allow rectangle shapes"),
            (outside, "annotation 2: the shape must lie inside the 20 × 10 image"),
            (Annotation::manual(9, Shape::Tag), "annotation 2: there is no label with id 9"),
            (tag(), "annotation 3: the image already has the tag \"Defect\""),
        ];
        for (bad, reason) in cases {
            let annotations = if bad == tag() { vec![tag(), crack(1.0), bad] } else { vec![crack(1.0), bad] };
            assert_eq!(refused(save(&mut conn, 1, image, 0, annotations).await), (StatusCode::BAD_REQUEST, reason.to_owned()));
        }
        assert_eq!(ok(load(&mut conn, 1, image).await), ImageAnnotations { version: 0, annotations: vec![], annotated: false });
    }

    #[tokio::test]
    async fn an_image_outside_the_project_is_not_found() {
        let (mut conn, image) = image().await;
        assert_eq!(refused(save(&mut conn, 2, image, 0, vec![]).await).0, StatusCode::NOT_FOUND);
        assert_eq!(refused(load(&mut conn, 2, image).await).0, StatusCode::NOT_FOUND);
    }

    fn predicted(model: &str, annotation: Annotation) -> Annotation {
        Annotation { source: Some(model.into()), confidence: Some(0.9), pending: true, ..annotation }
    }

    fn skips(written: &Written) -> Vec<(&str, usize)> {
        written.skipped.iter().map(|skip| (skip.reason.as_str(), skip.count)).collect()
    }

    #[tokio::test]
    async fn a_pre_annotation_write_replaces_only_that_models_pending_ones_and_goes_on_top() {
        let (mut conn, image) = image().await;
        let kept = vec![predicted("a", crack(1.0)), crack(3.0), predicted("b", crack(5.0)), Annotation { source: Some("a".into()), ..crack(7.0) }];
        ok(save(&mut conn, 1, image, 0, kept).await);

        let written = ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, vec![predicted("a", crack(9.0)), predicted("a", crack(11.0))]).await).unwrap();
        assert_eq!((written.stored, skips(&written)), (2, vec![]));
        let stored = ok(load(&mut conn, 1, image).await);
        assert_eq!(stored.version, 2, "an editor holding version 1 reloads");
        // Model a's reviewed annotation and the others stay below the new ones.
        assert_eq!(stored.annotations, [crack(3.0), predicted("b", crack(5.0)), Annotation { source: Some("a".into()), ..crack(7.0) }, predicted("a", crack(9.0)), predicted("a", crack(11.0))]);
    }

    #[tokio::test]
    async fn a_pre_annotation_write_leaves_the_annotated_status_as_it_is() {
        let (mut conn, image) = image().await;
        ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, vec![predicted("a", crack(1.0))]).await);
        assert!(!ok(load(&mut conn, 1, image).await).annotated);

        ok(save(&mut conn, 1, image, 1, vec![crack(3.0)]).await);
        // Even a write that removes every annotation does not unmark the image.
        ok(write(&mut conn, 1, image, Replacing::All, vec![]).await);
        assert!(ok(load(&mut conn, 1, image).await).annotated);
    }

    #[tokio::test]
    async fn a_pre_annotation_write_skips_what_the_labels_or_the_image_refuse() {
        let (mut conn, image) = image().await;
        let outside = Annotation::manual(CRACK, Shape::Polygon { points: vec![Point::new(0.0, 0.0), Point::new(500.0, 0.0), Point::new(0.0, 5.0)] });
        let made = vec![
            predicted("a", outside.clone()),
            predicted("a", crack(1.0)),
            predicted("a", Annotation::manual(DEFECT, Shape::Rectangle(Rect::new(1.0, 1.0, 2.0, 2.0)))),
            predicted("a", outside),
            predicted("a", Annotation::manual(9, Shape::Tag)),
        ];
        let written = ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, made).await).unwrap();
        assert_eq!(written.stored, 1);
        assert_eq!(skips(&written), [
            ("the shape must lie inside the 20 × 10 image", 2),
            ("label \"Defect\" does not allow rectangle shapes", 1),
            ("there is no label with id 9", 1),
        ]);
        assert_eq!(ok(load(&mut conn, 1, image).await).annotations, [predicted("a", crack(1.0))]);
    }

    #[tokio::test]
    async fn a_pre_annotation_write_skips_a_tag_the_image_already_has() {
        let (mut conn, image) = image().await;
        ok(save(&mut conn, 1, image, 0, vec![tag()]).await);
        let written = ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, vec![predicted("a", tag()), predicted("a", crack(1.0))]).await).unwrap();
        assert_eq!((written.stored, skips(&written)), (1, vec![("the image already has the tag \"Defect\"", 1)]));
        assert_eq!(ok(load(&mut conn, 1, image).await).annotations, [tag(), predicted("a", crack(1.0))]);

        // A model's own earlier tag is replaced, and a second copy in one write is skipped.
        ok(save(&mut conn, 1, image, 2, vec![predicted("a", tag())]).await);
        let written = ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, vec![predicted("a", tag()), predicted("a", tag())]).await).unwrap();
        assert_eq!((written.stored, skips(&written)), (1, vec![("the image already has the tag \"Defect\"", 1)]));
        assert_eq!(ok(load(&mut conn, 1, image).await).annotations, [predicted("a", tag())]);
    }

    #[tokio::test]
    async fn an_append_goes_on_top_of_every_stored_annotation_and_skips_tags_the_image_has() {
        let (mut conn, image) = image().await;
        ok(save(&mut conn, 1, image, 0, vec![crack(1.0), tag()]).await);
        let written = ok(write(&mut conn, 1, image, Replacing::Nothing, vec![tag(), crack(3.0), crack(5.0)]).await).unwrap();
        assert_eq!((written.stored, skips(&written)), (2, vec![("the image already has the tag \"Defect\"", 1)]));
        let stored = ok(load(&mut conn, 1, image).await);
        assert_eq!((stored.version, stored.annotations), (2, vec![crack(1.0), tag(), crack(3.0), crack(5.0)]));
    }

    #[tokio::test]
    async fn a_write_that_changes_nothing_keeps_the_version() {
        let (mut conn, image) = image().await;
        ok(save(&mut conn, 1, image, 0, vec![tag()]).await);
        let written = ok(write(&mut conn, 1, image, Replacing::Nothing, vec![tag()]).await).unwrap();
        assert_eq!(written.stored, 0);
        ok(write(&mut conn, 1, image, Replacing::Pending { source: "a" }, vec![]).await);
        assert_eq!(ok(load(&mut conn, 1, image).await).version, 1, "an editor holding version 1 keeps working");
    }

    #[tokio::test]
    async fn a_write_to_an_image_outside_the_project_stores_nothing() {
        let (mut conn, image) = image().await;
        assert!(ok(write(&mut conn, 2, image, Replacing::All, vec![crack(1.0)]).await).is_none());
        assert!(ok(write(&mut conn, 1, image + 1, Replacing::All, vec![crack(1.0)]).await).is_none());
        assert_eq!(ok(load(&mut conn, 1, image).await).version, 0);
    }
}
