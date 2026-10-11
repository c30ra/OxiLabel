use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use rtag_core::{NewProject, Project};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};

pub async fn app(database_url: &str) -> Result<Router, sqlx::Error> {
    // A single connection keeps `sqlite::memory:` databases alive and consistent.
    let pool = SqlitePoolOptions::new().max_connections(1).connect(database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", delete(delete_project))
        .with_state(pool))
}

async fn create_project(
    State(pool): State<SqlitePool>,
    Json(new): Json<NewProject>,
) -> Result<(StatusCode, Json<Project>), StatusCode> {
    let id = sqlx::query("INSERT INTO projects (name) VALUES (?) RETURNING id")
        .bind(&new.name)
        .fetch_one(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .get::<i64, _>("id");
    Ok((StatusCode::CREATED, Json(Project { id, name: new.name })))
}

async fn list_projects(State(pool): State<SqlitePool>) -> Result<Json<Vec<Project>>, StatusCode> {
    let rows = sqlx::query("SELECT id, name FROM projects ORDER BY id")
        .fetch_all(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        rows.iter()
            .map(|r| Project { id: r.get("id"), name: r.get("name") })
            .collect(),
    ))
}

async fn delete_project(State(pool): State<SqlitePool>, Path(id): Path<i64>) -> StatusCode {
    match sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(&pool).await {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT,
        Ok(_) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
