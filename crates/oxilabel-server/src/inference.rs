//! Pre-annotation: the registered models, the worker that runs them (ADR 0001, through
//! `worker::WorkerPort`), and runs of a model over images (`pre_annotation_run`), in the
//! background with progress and cancelling.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};
use oxilabel_core::{
    inference::{DEFAULT_WORKER_URL, Model, ModelDefinition, RunRequest, RunState, RunStatus, WorkerHealth, WorkerSettings},
    worker::WorkerModel,
};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{
    AppState,
    access_scope::AccessScope,
    auth::{Admin, CurrentUser},
    error::ApiError,
    images, labels,
    pre_annotation_run::PreAnnotationRun,
    worker::{HttpWorker, WorkerPort},
};

/// Ended runs kept for their status; older ones are forgotten.
const KEPT_RUNS: usize = 50;
const NO_WORKER: &str = "no pre-annotation worker is set up: an admin sets one on the Settings screen";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/worker", get(worker_health))
        .route("/api/models", get(list_models).post(create_model))
        .route("/api/models/{id}", put(change_model).delete(delete_model))
        .route("/api/models/{id}/classes", get(model_classes))
        .route("/api/projects/{project}/pre-annotations", post(start_run))
        .route("/api/runs/{id}", get(run_status).delete(cancel_run))
        .route("/api/settings/worker", get(worker_settings).put(change_worker_settings))
}

/// The worker, unless none is set up.
fn current_worker(state: &AppState) -> Result<HttpWorker, ApiError> {
    state.worker.current().ok_or_else(|| ApiError::Conflict(NO_WORKER.into()))
}

/// Uses `settings` from now on: the URL to call, and the command to run (restarted if it was
/// running).
pub(crate) fn apply_worker_settings(state: &AppState, settings: &WorkerSettings) {
    state.worker.point_at(settings.url.clone());
    let addr = settings.url.as_deref().unwrap_or(DEFAULT_WORKER_URL).trim_start_matches("http://").to_owned();
    state.worker_process.run(settings.command.clone(), addr);
}

async fn worker_health(_: CurrentUser, State(state): State<AppState>) -> Json<WorkerHealth> {
    let device = match state.worker.current() {
        Some(worker) => worker.health().await.map_err(|err| err.to_string()),
        None => Err(NO_WORKER.to_owned()),
    };
    Json(match device {
        Ok(device) => WorkerHealth { reachable: true, device: Some(device), error: None },
        Err(error) => WorkerHealth { reachable: false, device: None, error: Some(error) },
    })
}

fn model_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Model, ApiError> {
    fn json<T: serde::de::DeserializeOwned>(row: &sqlx::sqlite::SqliteRow, key: &str) -> Result<T, ApiError> {
        serde_json::from_str(row.get(key)).map_err(|err| ApiError::Internal(format!("stored model {key} unreadable: {err}")))
    }
    Ok(Model {
        id: row.get("id"),
        definition: ModelDefinition {
            name: row.get("name"),
            adapter: row.get("adapter"),
            weights: row.get("weights"),
            options: json(row, "options")?,
            class_map: json(row, "class_map")?,
        },
    })
}

async fn model(conn: &mut SqliteConnection, id: i64) -> Result<Model, ApiError> {
    let row = sqlx::query("SELECT * FROM models WHERE id = ?").bind(id).fetch_optional(conn).await?.ok_or(ApiError::NotFound)?;
    model_from_row(&row)
}

async fn list_models(_: CurrentUser, State(pool): State<SqlitePool>) -> Result<Json<Vec<Model>>, ApiError> {
    let rows = sqlx::query("SELECT * FROM models ORDER BY name").fetch_all(&pool).await?;
    Ok(Json(rows.iter().map(model_from_row).collect::<Result<_, _>>()?))
}

/// The definition with its text trimmed, unless something is missing.
fn validated(mut definition: ModelDefinition) -> Result<ModelDefinition, ApiError> {
    for (field, value) in [("name", &mut definition.name), ("adapter", &mut definition.adapter), ("weights", &mut definition.weights)] {
        *value = value.trim().to_owned();
        if value.is_empty() {
            return Err(ApiError::BadRequest(format!("the model's {field} must not be blank")));
        }
    }
    Ok(definition)
}

fn name_taken(err: sqlx::Error, name: &str) -> ApiError {
    match err.as_database_error() {
        Some(db) if db.is_unique_violation() => ApiError::Conflict(format!("there is already a model named \"{name}\"")),
        _ => err.into(),
    }
}

/// Binds a definition's name, adapter, weights, options and class map, in that order.
fn bind_definition<'q, O>(
    query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    definition: &ModelDefinition,
) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, sqlx::sqlite::SqliteArguments<'q>> {
    query
        .bind(definition.name.clone())
        .bind(definition.adapter.clone())
        .bind(definition.weights.clone())
        .bind(serde_json::to_string(&definition.options).expect("options serialise"))
        .bind(serde_json::to_string(&definition.class_map).expect("class maps serialise"))
}

async fn create_model(_: Admin, State(pool): State<SqlitePool>, Json(definition): Json<ModelDefinition>) -> Result<(StatusCode, Json<Model>), ApiError> {
    let definition = validated(definition)?;
    let (id,): (i64,) = bind_definition(sqlx::query_as("INSERT INTO models (name, adapter, weights, options, class_map) VALUES (?, ?, ?, ?, ?) RETURNING id"), &definition)
        .fetch_one(&pool)
        .await
        .map_err(|err| name_taken(err, &definition.name))?;
    Ok((StatusCode::CREATED, Json(Model { id, definition })))
}

async fn change_model(_: Admin, State(pool): State<SqlitePool>, Path(id): Path<i64>, Json(definition): Json<ModelDefinition>) -> Result<Json<Model>, ApiError> {
    let definition = validated(definition)?;
    let changed: Option<(i64,)> = bind_definition(sqlx::query_as("UPDATE models SET name = ?, adapter = ?, weights = ?, options = ?, class_map = ? WHERE id = ? RETURNING id"), &definition)
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|err| name_taken(err, &definition.name))?;
    changed.ok_or(ApiError::NotFound)?;
    Ok(Json(Model { id, definition }))
}

/// Forgets a model; its pre-annotations stay, with its name as their source.
async fn delete_model(_: Admin, State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let deleted = sqlx::query("DELETE FROM models WHERE id = ?").bind(id).execute(&pool).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The model's class names, as the worker reads them from the weights.
async fn model_classes(_: Admin, State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Vec<String>>, ApiError> {
    let model = model(&mut *state.pool.acquire().await?, id).await?;
    let worker = current_worker(&state)?;
    worker.classes(&WorkerModel::from(&model.definition)).await.map(Json).map_err(|err| ApiError::Conflict(err.to_string()))
}

/// The runs of this server's lifetime: their status, and a flag that cancels each.
#[derive(Clone, Default)]
pub struct Runs(Arc<Mutex<RunTable>>);

#[derive(Default)]
struct RunTable {
    next: u64,
    runs: BTreeMap<u64, (RunStatus, Arc<AtomicBool>)>,
}

impl Runs {
    fn set(&self, status: &RunStatus) {
        if let Some((current, _)) = self.0.lock().expect("runs lock").runs.get_mut(&status.id) {
            current.clone_from(status);
        }
    }

    fn get(&self, id: u64) -> Option<(RunStatus, Arc<AtomicBool>)> {
        self.0.lock().expect("runs lock").runs.get(&id).cloned()
    }
}

#[derive(Deserialize)]
struct RunPath {
    id: u64,
}

/// Starts running a model on the given images, or a job's, in the background.
async fn start_run(scope: AccessScope, State(state): State<AppState>, Path(project): Path<i64>, Json(request): Json<RunRequest>) -> Result<(StatusCode, Json<RunStatus>), ApiError> {
    let mut conn = state.pool.acquire().await?;
    scope.project(&mut conn, project).await?;
    let model = model(&mut conn, request.model).await?;
    let image_ids: Vec<i64> = match request.job {
        Some(job) => scope.job_images(&mut conn, project, job).await?,
        None => {
            scope.check_run_images(&mut conn, project, &request.image_ids).await?;
            request.image_ids.clone()
        }
    };
    if image_ids.is_empty() {
        return Err(ApiError::BadRequest("choose images to run the model on".into()));
    }
    let worker = current_worker(&state)?;
    let labels = labels::project_labels(&mut conn, project).await?;
    drop(conn);
    let root = images::image_root(&state);

    let cancelled = Arc::new(AtomicBool::new(false));
    let status = {
        let mut table = state.runs.0.lock().expect("runs lock");
        let ended: Vec<u64> = table.runs.iter().filter(|(_, (status, _))| status.state != RunState::Running).map(|(id, _)| *id).collect();
        for id in ended.iter().take(ended.len().saturating_sub(KEPT_RUNS)) {
            table.runs.remove(id);
        }
        table.next += 1;
        let status = RunStatus {
            id: table.next,
            project,
            model: model.definition.name.clone(),
            state: RunState::Running,
            total: image_ids.len(),
            done: 0,
            created: 0,
            skipped: Vec::new(),
            error: None,
        };
        table.runs.insert(status.id, (status.clone(), cancelled.clone()));
        status
    };
    let runs = state.runs.clone();
    let started = status.clone();
    tokio::spawn(async move {
        let Ok(root) = root else {
            return runs.set(&RunStatus { state: RunState::Failed, error: Some("the image folder is not available".into()), ..started });
        };
        let run = PreAnnotationRun { pool: state.pool.clone(), root, project, model: model.definition, labels, threshold: request.threshold };
        run.run(&worker, &image_ids, &cancelled, started, |status| runs.set(status)).await;
    });
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn visible_run(state: &AppState, scope: &AccessScope, id: u64) -> Result<(RunStatus, Arc<AtomicBool>), ApiError> {
    let (status, cancelled) = state.runs.get(id).ok_or(ApiError::NotFound)?;
    scope.project(&mut *state.pool.acquire().await?, status.project).await?;
    Ok((status, cancelled))
}

async fn run_status(scope: AccessScope, State(state): State<AppState>, Path(RunPath { id }): Path<RunPath>) -> Result<Json<RunStatus>, ApiError> {
    Ok(Json(visible_run(&state, &scope, id).await?.0))
}

/// Stops a run after the image the worker is on.
async fn cancel_run(scope: AccessScope, State(state): State<AppState>, Path(RunPath { id }): Path<RunPath>) -> Result<StatusCode, ApiError> {
    visible_run(&state, &scope, id).await?.1.store(true, Ordering::SeqCst);
    Ok(StatusCode::NO_CONTENT)
}

/// The stored worker settings (none set: no worker).
pub async fn stored_worker_settings(pool: &SqlitePool) -> Result<WorkerSettings, ApiError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'worker'").fetch_optional(pool).await?;
    Ok(value.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or_default())
}

async fn worker_settings(_: Admin, State(pool): State<SqlitePool>) -> Result<Json<WorkerSettings>, ApiError> {
    Ok(Json(stored_worker_settings(&pool).await?))
}

/// Stores the worker settings and applies them at once.
async fn change_worker_settings(_: Admin, State(state): State<AppState>, Json(settings): Json<WorkerSettings>) -> Result<Json<WorkerSettings>, ApiError> {
    let given = |text: Option<String>| text.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
    let command = given(settings.command);
    let url = given(settings.url).map(|url| url.trim_end_matches('/').to_owned());
    if url.as_deref().is_some_and(|url| !url.starts_with("http://")) {
        return Err(ApiError::BadRequest("the worker's URL must start with http://".into()));
    }
    let url = url.or_else(|| command.is_some().then(|| DEFAULT_WORKER_URL.to_owned()));
    let settings = WorkerSettings { command, url };
    sqlx::query("INSERT INTO settings (key, value) VALUES ('worker', ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(serde_json::to_string(&settings).expect("settings serialise"))
        .execute(&state.pool)
        .await?;
    apply_worker_settings(&state, &settings);
    Ok(Json(settings))
}
