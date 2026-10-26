//! The Project read model: a project as exporters and project archives read it. The project, its
//! labels, and its images in path order, each with its annotations in drawing order and the file
//! it is read from. Exports and archives map it to their own input; neither queries the project.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use oxilabel_core::{Label, annotation::Annotation};
use sqlx::{Row, SqliteConnection};

use crate::{
    annotation_store::{ANNOTATION_COLUMNS, annotation_from_row},
    error::ApiError,
    images::source_file,
    labels,
};

pub(crate) struct ProjectRead {
    pub name: String,
    /// The next label id and job number the project hands out.
    pub next_label_id: i64,
    pub next_job_number: i64,
    pub labels: Vec<Label>,
    /// In path order.
    pub images: Vec<ImageRead>,
}

pub(crate) struct ImageRead {
    pub id: i64,
    /// As the project stores it: relative to the image root, or absolute.
    pub path: String,
    /// The image's file.
    pub source: PathBuf,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub annotated: bool,
    /// The number of its job.
    pub job: Option<i64>,
    /// In drawing order.
    pub annotations: Vec<Annotation>,
}

/// Whether a read includes unreviewed pre-annotations.
#[derive(Clone, Copy)]
pub(crate) enum Pending {
    Include,
    Exclude,
}

/// The project's annotations by image and drawing order; the second parameter includes pending ones.
static ANNOTATIONS: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT image_id, {ANNOTATION_COLUMNS} FROM annotations WHERE project_id = ? AND (? OR NOT pending) ORDER BY image_id, position")
});

/// Reads `project`, or `NotFound` if there is none. Image files are resolved by
/// `images::source_file` from the canonical image root (`images::image_root`).
pub(crate) async fn read(conn: &mut SqliteConnection, root: &Path, project: i64, pending: Pending) -> Result<ProjectRead, ApiError> {
    let row = sqlx::query("SELECT name, next_label_id, next_job_number FROM projects WHERE id = ?")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApiError::NotFound)?;
    let labels = labels::project_labels(conn, project).await?;
    let mut annotations: HashMap<i64, Vec<Annotation>> = HashMap::new();
    for row in sqlx::query(&ANNOTATIONS).bind(project).bind(matches!(pending, Pending::Include)).fetch_all(&mut *conn).await? {
        annotations.entry(row.get("image_id")).or_default().push(annotation_from_row(&row)?);
    }
    let images = sqlx::query(
        "SELECT images.id, path, hash, width, height, annotated, jobs.number AS job FROM images LEFT JOIN jobs ON jobs.id = images.job_id \
         WHERE images.project_id = ? ORDER BY path",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(|row| {
        let (id, path): (i64, String) = (row.get("id"), row.get("path"));
        ImageRead {
            id,
            source: source_file(root, &path),
            path,
            hash: row.get("hash"),
            width: row.get("width"),
            height: row.get("height"),
            annotated: row.get("annotated"),
            job: row.get("job"),
            annotations: annotations.remove(&id).unwrap_or_default(),
        }
    })
    .collect();
    Ok(ProjectRead { name: row.get("name"), next_label_id: row.get("next_label_id"), next_job_number: row.get("next_job_number"), labels, images })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use oxilabel_core::{
        annotation::{Annotation, Shape},
        geometry::Rect,
    };
    use serde_json::json;
    use sqlx::{Connection, SqliteConnection};

    use super::*;
    use crate::{annotation_store, error::ApiError, labels};

    const CRACK: i64 = 1;

    fn ok<T>(result: Result<T, ApiError>) -> T {
        result.unwrap_or_else(|err| panic!("{}", err.message()))
    }

    /// A database with project 1 "Cells" and its label "Crack" (boxes); the connection.
    async fn project() -> SqliteConnection {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON").execute(&mut conn).await.unwrap();
        sqlx::migrate!("./migrations").run(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO projects (name) VALUES ('Cells')").execute(&mut conn).await.unwrap();
        ok(labels::insert_label(&mut conn, 1, serde_json::from_value(json!({"name": "Crack", "color": "#ff0000"})).unwrap()).await);
        conn
    }

    /// Adds a 20 × 10 image at `path` to project 1; its id.
    async fn add_image(conn: &mut SqliteConnection, path: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO images (project_id, path, hash, width, height) VALUES (1, ?, ?, 20, 10) RETURNING id")
            .bind(path)
            .bind(format!("hash of {path}"))
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    }

    fn crack(min_x: f64) -> Annotation {
        Annotation::manual(CRACK, Shape::Rectangle(Rect::new(min_x, 1.0, min_x + 2.0, 3.0)))
    }

    #[tokio::test]
    async fn images_come_in_path_order_with_their_annotations_in_drawing_order() {
        let mut conn = project().await;
        let b = add_image(&mut conn, "set/b.png").await;
        let a = add_image(&mut conn, "set/a.png").await;
        add_image(&mut conn, "set/c.png").await;
        ok(annotation_store::save(&mut conn, 1, b, 0, vec![crack(9.0), crack(1.0), crack(5.0)]).await);
        ok(annotation_store::save(&mut conn, 1, a, 0, vec![crack(3.0)]).await);

        let read = ok(read(&mut conn, Path::new("/root"), 1, Pending::Include).await);
        assert_eq!(read.name, "Cells");
        assert_eq!(read.labels.iter().map(|label| label.name.as_str()).collect::<Vec<_>>(), ["Crack"]);
        let images: Vec<_> = read.images.iter().map(|image| (image.path.as_str(), image.annotations.clone())).collect();
        assert_eq!(images, [("set/a.png", vec![crack(3.0)]), ("set/b.png", vec![crack(9.0), crack(1.0), crack(5.0)]), ("set/c.png", vec![])]);
    }

    #[tokio::test]
    async fn pending_pre_annotations_are_read_only_when_included() {
        let mut conn = project().await;
        let image = add_image(&mut conn, "a.png").await;
        let predicted = Annotation { source: Some("model".into()), confidence: Some(0.5), pending: true, ..crack(1.0) };
        let reviewed = Annotation { source: Some("model".into()), confidence: Some(0.5), ..crack(5.0) };
        ok(annotation_store::save(&mut conn, 1, image, 0, vec![predicted.clone(), reviewed.clone(), crack(9.0)]).await);

        let included = ok(read(&mut conn, Path::new("/root"), 1, Pending::Include).await);
        assert_eq!(included.images[0].annotations, [predicted, reviewed.clone(), crack(9.0)]);
        let excluded = ok(read(&mut conn, Path::new("/root"), 1, Pending::Exclude).await);
        assert_eq!(excluded.images[0].annotations, [reviewed, crack(9.0)]);
    }

    #[tokio::test]
    async fn an_image_is_read_from_its_path_under_the_root_or_its_absolute_path() {
        let mut conn = project().await;
        add_image(&mut conn, "set/a.png").await;
        add_image(&mut conn, "/uploads/b.png").await;
        let read = ok(read(&mut conn, Path::new("/root"), 1, Pending::Include).await);
        let sources: Vec<_> = read.images.iter().map(|image| (image.path.as_str(), image.source.as_path())).collect();
        assert_eq!(sources, [("/uploads/b.png", Path::new("/uploads/b.png")), ("set/a.png", Path::new("/root/set/a.png"))]);
    }

    #[tokio::test]
    async fn a_missing_project_is_not_found() {
        let mut conn = project().await;
        let Err(err) = read(&mut conn, Path::new("/root"), 2, Pending::Include).await else { panic!("expected NotFound") };
        assert_eq!(err.public().0, axum::http::StatusCode::NOT_FOUND);
    }
}
