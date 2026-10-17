//! Jobs: batches of a project's images, each assigned to at most one user. An annotator sees and
//! edits only the images of their own jobs (and the projects those are in); admins everything.
//! Also the project statistics, which count annotations per label, job and user.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use rtag_core::{
    job::{Job, JobChange, LabelCount, NewJobs, Statistics, UserCount},
    user::{Role, User},
};
use sqlx::{QueryBuilder, Row, SqliteConnection, SqlitePool, sqlite::SqliteRow};

use crate::{
    AppState,
    auth::{Admin, CurrentUser},
    error::ApiError,
    projects,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{project}/jobs", get(list_jobs).post(create_jobs))
        .route("/api/projects/{project}/jobs/{id}", put(change_job).delete(delete_job))
        .route("/api/projects/{project}/statistics", get(statistics))
}

/// Fails with `NotFound` unless the project exists and `user` may see it: admins see every
/// project, annotators those with a job assigned to them.
pub async fn ensure_project_visible(conn: &mut SqliteConnection, user: &CurrentUser, project: i64) -> Result<(), ApiError> {
    projects::ensure_exists(conn, project).await?;
    if let Some(owner) = user.job_owner() {
        sqlx::query("SELECT 1 FROM jobs WHERE project_id = ? AND assignee_id = ?")
            .bind(project)
            .bind(owner)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(ApiError::NotFound)?;
    }
    Ok(())
}

/// Fails with `Forbidden` if `user` is an annotator and the image is not in one of their jobs.
/// Whether the image exists is the caller's business.
pub async fn ensure_image_access(conn: &mut SqliteConnection, user: &CurrentUser, image: i64) -> Result<(), ApiError> {
    if let Some(owner) = user.job_owner() {
        sqlx::query("SELECT 1 FROM images JOIN jobs ON jobs.id = images.job_id WHERE images.id = ? AND jobs.assignee_id = ?")
            .bind(image)
            .bind(owner)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| ApiError::Forbidden("this image is not in one of your jobs".to_owned()))?;
    }
    Ok(())
}

/// Each job with its assignee and counts, selected from `jobs`; callers append the conditions.
const JOB_COLUMNS: &str = "SELECT jobs.id, jobs.number, users.id AS user_id, users.username, users.role, \
     (SELECT COUNT(*) FROM images WHERE images.job_id = jobs.id) AS images, \
     (SELECT COUNT(*) FROM images WHERE images.job_id = jobs.id AND annotated) AS annotated, \
     (SELECT COUNT(*) FROM annotations JOIN images ON images.id = annotations.image_id WHERE images.job_id = jobs.id) AS annotations \
     FROM jobs LEFT JOIN users ON users.id = jobs.assignee_id";

fn user_from_job_row(row: &SqliteRow) -> Option<User> {
    let id: Option<i64> = row.get("user_id");
    id.map(|id| User {
        id,
        username: row.get("username"),
        role: Role::parse(row.get("role")).expect("the schema allows only known roles"),
    })
}

fn job_from_row(row: &SqliteRow) -> Job {
    Job {
        id: row.get("id"),
        number: row.get("number"),
        assignee: user_from_job_row(row),
        images: row.get("images"),
        annotated: row.get("annotated"),
        annotations: row.get("annotations"),
    }
}

async fn project_jobs(conn: &mut SqliteConnection, project: i64, owner: Option<i64>) -> Result<Vec<Job>, ApiError> {
    let mut select = QueryBuilder::new(JOB_COLUMNS);
    select.push(" WHERE jobs.project_id = ").push_bind(project);
    if let Some(owner) = owner {
        select.push(" AND jobs.assignee_id = ").push_bind(owner);
    }
    select.push(" ORDER BY jobs.number");
    Ok(select.build().fetch_all(&mut *conn).await?.iter().map(job_from_row).collect())
}

async fn job(conn: &mut SqliteConnection, project: i64, id: i64) -> Result<Job, ApiError> {
    let row = sqlx::query(&format!("{JOB_COLUMNS} WHERE jobs.project_id = ? AND jobs.id = ?"))
        .bind(project)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(job_from_row(&row))
}

async fn list_jobs(user: CurrentUser, State(pool): State<SqlitePool>, Path(project): Path<i64>) -> Result<Json<Vec<Job>>, ApiError> {
    let mut conn = pool.acquire().await?;
    ensure_project_visible(&mut conn, &user, project).await?;
    Ok(Json(project_jobs(&mut conn, project, user.job_owner()).await?))
}

async fn ensure_user_exists(conn: &mut SqliteConnection, user: Option<i64>) -> Result<(), ApiError> {
    if let Some(user) = user {
        sqlx::query("SELECT 1 FROM users WHERE id = ?")
            .bind(user)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| ApiError::BadRequest(format!("there is no user {user}")))?;
    }
    Ok(())
}

/// Puts the given images of the project (in path order) into one new job, or into new jobs of
/// `per_job` images; images in other jobs move. Ids of other projects' images are ignored.
async fn create_jobs(
    _: Admin,
    State(pool): State<SqlitePool>,
    Path(project): Path<i64>,
    Json(new): Json<NewJobs>,
) -> Result<(StatusCode, Json<Vec<Job>>), ApiError> {
    if new.per_job == Some(0) {
        return Err(ApiError::BadRequest("a job needs at least one image".to_owned()));
    }
    // Takes the write lock at once, so a concurrent creation waits instead of failing midway.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    projects::ensure_exists(&mut tx, project).await?;
    ensure_user_exists(&mut tx, new.assignee).await?;
    // The ids go as one JSON array: a selection can hold more ids than SQLite binds at once.
    let images: Vec<i64> = sqlx::query("SELECT id FROM images WHERE project_id = ? AND id IN (SELECT value FROM json_each(?)) ORDER BY path")
        .bind(project)
        .bind(serde_json::to_string(&new.image_ids).expect("ids serialise"))
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| row.get("id"))
        .collect();
    if images.is_empty() {
        return Err(ApiError::BadRequest("choose images of this project for the job".to_owned()));
    }
    let mut created = Vec::new();
    for batch in images.chunks(new.per_job.map_or(images.len(), |n| n as usize)) {
        let id: i64 = sqlx::query(
            "INSERT INTO jobs (project_id, number, assignee_id) \
             SELECT id, next_job_number, ? FROM projects WHERE id = ? RETURNING id",
        )
        .bind(new.assignee)
        .bind(project)
        .fetch_one(&mut *tx)
        .await?
        .get("id");
        sqlx::query("UPDATE projects SET next_job_number = next_job_number + 1 WHERE id = ?").bind(project).execute(&mut *tx).await?;
        sqlx::query("UPDATE images SET job_id = ? WHERE id IN (SELECT value FROM json_each(?))")
            .bind(id)
            .bind(serde_json::to_string(batch).expect("ids serialise"))
            .execute(&mut *tx)
            .await?;
        created.push(id);
    }
    // Jobs emptied by the move stay, so their numbers keep meaning the same.
    let mut jobs = Vec::new();
    for id in created {
        jobs.push(job(&mut tx, project, id).await?);
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(jobs)))
}

/// Assigns a job to another user, or to nobody.
async fn change_job(
    _: Admin,
    State(pool): State<SqlitePool>,
    Path((project, id)): Path<(i64, i64)>,
    Json(change): Json<JobChange>,
) -> Result<Json<Job>, ApiError> {
    let mut conn = pool.acquire().await?;
    ensure_user_exists(&mut conn, change.assignee).await?;
    let updated = sqlx::query("UPDATE jobs SET assignee_id = ? WHERE id = ? AND project_id = ?")
        .bind(change.assignee)
        .bind(id)
        .bind(project)
        .execute(&mut *conn)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(job(&mut conn, project, id).await?))
}

/// Deletes a job; its images stay in the project, in no job.
async fn delete_job(_: Admin, State(pool): State<SqlitePool>, Path((project, id)): Path<(i64, i64)>) -> Result<StatusCode, ApiError> {
    let deleted = sqlx::query("DELETE FROM jobs WHERE id = ? AND project_id = ?").bind(id).bind(project).execute(&pool).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn statistics(_: Admin, State(pool): State<SqlitePool>, Path(project): Path<i64>) -> Result<Json<Statistics>, ApiError> {
    let mut conn = pool.acquire().await?;
    projects::ensure_exists(&mut conn, project).await?;
    let labels = sqlx::query(
        "SELECT labels.id, labels.name, labels.color, COUNT(annotations.id) AS annotations, \
           COUNT(DISTINCT annotations.image_id) AS images \
         FROM labels LEFT JOIN annotations ON annotations.project_id = labels.project_id AND annotations.label_id = labels.id \
         WHERE labels.project_id = ? GROUP BY labels.id ORDER BY labels.id",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(|row| LabelCount {
        id: row.get("id"),
        name: row.get("name"),
        color: row.get("color"),
        annotations: row.get("annotations"),
        images: row.get("images"),
    })
    .collect();
    // Per user: the images of the jobs assigned to them; the rest (no job, or an unassigned one)
    // goes last, under no user.
    let users = sqlx::query(
        "SELECT users.id AS user_id, users.username, users.role, COUNT(*) AS images, \
           COALESCE(SUM(images.annotated), 0) AS annotated, \
           COALESCE(SUM((SELECT COUNT(*) FROM annotations WHERE annotations.image_id = images.id)), 0) AS annotations \
         FROM images LEFT JOIN jobs ON jobs.id = images.job_id LEFT JOIN users ON users.id = jobs.assignee_id \
         WHERE images.project_id = ? GROUP BY users.id ORDER BY users.id IS NULL, users.username",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(|row| UserCount {
        user: user_from_job_row(row),
        images: row.get("images"),
        annotated: row.get("annotated"),
        annotations: row.get("annotations"),
    })
    .collect();
    let jobs = project_jobs(&mut conn, project, None).await?;
    Ok(Json(Statistics { labels, jobs, users }))
}
