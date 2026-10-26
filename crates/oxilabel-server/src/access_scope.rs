//! What the signed-in user may see, in one place: admins everything, annotators the projects
//! they have a job in, those jobs, and those jobs' images. Handlers ask for a visible project,
//! image or run, or build their image and job queries on the scope's `visible_*` views; nothing
//! else knows how job assignment bounds access. Anything outside the scope is not found, exactly
//! as if it did not exist.

use std::path::PathBuf;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use oxilabel_core::{Image, user::Role};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection, sqlite::SqliteRow};

use crate::{
    AppState,
    auth::CurrentUser,
    error::ApiError,
    images::{IMAGE_COLUMNS, image_from_row, source_file},
};

/// The signed-in user's view of the projects, jobs and images.
pub(crate) struct AccessScope {
    user: CurrentUser,
    images_root: PathBuf,
}

/// An image the user may open: its facts and the file it is read from (ADR 0003).
pub(crate) struct VisibleImage {
    pub image: Image,
    pub hash: String,
    pub source: PathBuf,
}

impl AccessScope {
    /// A query over this scope: append `SELECT … FROM visible_images` (or `visible_jobs`,
    /// `visible_projects`) and its conditions. The views hold the rows of those tables the user
    /// may see — an image is visible through the job it is in — and are inlined into the query,
    /// so their conditions take part in the query plan as usual. The annotator's id is bound
    /// before the conditions the caller binds.
    pub(crate) fn sql(&self) -> QueryBuilder<'_, Sqlite> {
        // The annotator's id (null for an admin) is bound at each gap, before the caller's
        // conditions; an admin's NULL lets every row through, including images in no job.
        let owner = self.job_owner();
        let mut query = QueryBuilder::new("WITH visible_jobs AS NOT MATERIALIZED (SELECT * FROM jobs WHERE ");
        query.push_bind(owner).push(" IS NULL OR assignee_id = ").push_bind(owner);
        query.push("), visible_images AS NOT MATERIALIZED (SELECT * FROM images WHERE ");
        query.push_bind(owner);
        query.push(" IS NULL OR job_id IN (SELECT id FROM visible_jobs)), visible_projects AS NOT MATERIALIZED (SELECT * FROM projects WHERE ");
        query.push_bind(owner).push(" IS NULL OR id IN (SELECT project_id FROM visible_jobs)) ");
        query
    }

    /// `NotFound` unless the project exists and is visible.
    pub(crate) async fn project(&self, conn: &mut SqliteConnection, project: i64) -> Result<(), ApiError> {
        self.sql()
            .push("SELECT 1 FROM visible_projects WHERE id = ")
            .push_bind(project)
            .build()
            .fetch_optional(&mut *conn)
            .await?
            .map(|_| ())
            .ok_or(ApiError::NotFound)
    }

    /// The image, or `NotFound` if it does not exist or the user may not open it.
    pub(crate) async fn image(&self, conn: &mut SqliteConnection, image: i64) -> Result<VisibleImage, ApiError> {
        self.image_where(conn, image, None).await
    }

    /// The image of `project`, or `NotFound`: an image of another project, or of another user's
    /// job, is no different from one that does not exist.
    pub(crate) async fn project_image(&self, conn: &mut SqliteConnection, project: i64, image: i64) -> Result<VisibleImage, ApiError> {
        self.image_where(conn, image, Some(project)).await
    }

    async fn image_where(&self, conn: &mut SqliteConnection, image: i64, project: Option<i64>) -> Result<VisibleImage, ApiError> {
        let mut query = self.sql();
        query.push(format!("SELECT id, hash, {IMAGE_COLUMNS} FROM visible_images images WHERE id = "));
        query.push_bind(image);
        if let Some(project) = project {
            query.push(" AND project_id = ").push_bind(project);
        }
        let row = query.build().fetch_optional(&mut *conn).await?.ok_or(ApiError::NotFound)?;
        Ok(self.image_from(row))
    }

    /// The images of one of the user's jobs in `project`, in path order; `NotFound` for a job
    /// the user may not see (their own job with no images is an empty list).
    pub(crate) async fn job_images(&self, conn: &mut SqliteConnection, project: i64, job: i64) -> Result<Vec<i64>, ApiError> {
        self.sql()
            .push("SELECT 1 FROM visible_jobs WHERE id = ")
            .push_bind(job)
            .push(" AND project_id = ")
            .push_bind(project)
            .build()
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(ApiError::NotFound)?;
        let rows = self
            .sql()
            .push("SELECT id FROM visible_images WHERE job_id = ")
            .push_bind(job)
            .push(" ORDER BY path")
            .build()
            .fetch_all(&mut *conn)
            .await?;
        Ok(rows.iter().map(|row| row.get("id")).collect())
    }

    /// Checks every image of a run at once, in one query: `NotFound` if any id is not an image
    /// of `project` the user may see.
    pub(crate) async fn check_run_images(&self, conn: &mut SqliteConnection, project: i64, images: &[i64]) -> Result<(), ApiError> {
        let ids = serde_json::to_string(images).expect("image ids serialise");
        let hidden = self
            .sql()
            .push("SELECT value FROM json_each(")
            .push_bind(ids)
            .push(") WHERE value NOT IN (SELECT id FROM visible_images WHERE project_id = ")
            .push_bind(project)
            .push(") LIMIT 1")
            .build()
            .fetch_optional(&mut *conn)
            .await?;
        if hidden.is_some() {
            return Err(ApiError::NotFound);
        }
        Ok(())
    }

    /// The user whose jobs bound what this one sees: an annotator themself; none for an admin,
    /// who sees everything.
    fn job_owner(&self) -> Option<i64> {
        (self.user.user.role == Role::Annotator).then_some(self.user.user.id)
    }

    fn image_from(&self, row: SqliteRow) -> VisibleImage {
        VisibleImage {
            source: source_file(&self.images_root, &row.get::<String, _>("path")),
            hash: row.get("hash"),
            image: image_from_row(&row),
        }
    }
}

impl<S> FromRequestParts<S> for AccessScope
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        Ok(AccessScope { user: CurrentUser::from_request_parts(parts, state).await?, images_root: AppState::from_ref(state).images_root })
    }
}
