//! Pre-annotation: the registered models, the worker that runs them (ADR 0001), and runs of a
//! model over images, in the background with progress and cancelling.
//!
//! The worker contract (HTTP/JSON): `GET /health` → `{device}`; `POST /classes` with a model
//! (`adapter`, `weights`, `options`) → `{classes}`; `POST /predict` with a model and `image`
//! (the file's bytes, base64) → `{predictions: [{class, confidence, shape}]}`, shapes in the
//! image's pixels as rtag writes them. Errors come as a non-2xx status with `{error}`.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};
use base64::Engine;
use rtag_core::{
    Label,
    annotation::Annotation,
    import::SkipCount,
    inference::{Model, ModelDefinition, Prediction, RunRequest, RunState, RunStatus, WorkerHealth},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::{
    AppState,
    annotations::{check_annotation, insert_annotations},
    auth::{Admin, CurrentUser},
    error::ApiError,
    images, jobs, labels,
};

/// Loading a model can take a while the first time; a prediction on a large photo too.
const PREDICT_TIMEOUT: Duration = Duration::from_secs(600);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/worker", get(worker_health))
        .route("/api/models", get(list_models).post(create_model))
        .route("/api/models/{id}", put(change_model).delete(delete_model))
        .route("/api/models/{id}/classes", get(model_classes))
        .route("/api/projects/{project}/pre-annotations", post(start_run))
        .route("/api/runs/{id}", get(run_status).delete(cancel_run))
}

/// The worker's client: where it listens, if anywhere.
#[derive(Clone)]
pub struct Worker {
    url: Option<String>,
    client: reqwest::Client,
}

impl Worker {
    pub fn new(url: Option<String>) -> Self {
        Worker { url: url.map(|url| url.trim_end_matches('/').to_owned()), client: reqwest::Client::new() }
    }

    fn url(&self, path: &str) -> Result<String, String> {
        let base = self.url.as_deref().ok_or("no pre-annotation worker is set up: start the server with RTAG_WORKER_URL (or RTAG_WORKER_COMMAND)")?;
        Ok(format!("{base}{path}"))
    }

    /// Sends a request and reads the JSON answer, or the worker's `{error}`.
    async fn call(&self, request: reqwest::RequestBuilder) -> Result<Value, String> {
        let response = request.send().await.map_err(|err| format!("the worker did not answer: {err}"))?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|err| format!("the worker's answer is unreadable: {err}"))?;
        if !status.is_success() {
            let message = body["error"].as_str().map(str::to_owned).unwrap_or_else(|| format!("status {status}"));
            return Err(format!("the worker failed: {message}"));
        }
        Ok(body)
    }

    async fn health(&self) -> WorkerHealth {
        let answer = match self.url("/health") {
            Ok(url) => self.call(self.client.get(url).timeout(HEALTH_TIMEOUT)).await,
            Err(message) => Err(message),
        };
        match answer {
            Ok(body) => WorkerHealth { reachable: true, device: body["device"].as_str().map(str::to_owned), error: None },
            Err(error) => WorkerHealth { reachable: false, device: None, error: Some(error) },
        }
    }

    async fn post(&self, path: &str, model: &ModelDefinition, extra: Option<(&str, Value)>) -> Result<Value, String> {
        let mut body = json!({"adapter": model.adapter, "weights": model.weights, "options": model.options});
        if let Some((key, value)) = extra {
            body[key] = value;
        }
        self.call(self.client.post(self.url(path)?).json(&body).timeout(PREDICT_TIMEOUT)).await
    }

    async fn classes(&self, model: &ModelDefinition) -> Result<Vec<String>, String> {
        let body = self.post("/classes", model, None).await?;
        serde_json::from_value(body["classes"].clone()).map_err(|err| format!("the worker's classes are unreadable: {err}"))
    }

    async fn predict(&self, model: &ModelDefinition, image: &[u8]) -> Result<Vec<Prediction>, String> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(image);
        let body = self.post("/predict", model, Some(("image", Value::String(encoded)))).await?;
        serde_json::from_value(body["predictions"].clone()).map_err(|err| format!("the worker's predictions are unreadable: {err}"))
    }
}

async fn worker_health(_: CurrentUser, State(state): State<AppState>) -> Json<WorkerHealth> {
    Json(state.worker.health().await)
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

async fn create_model(_: Admin, State(pool): State<SqlitePool>, Json(definition): Json<ModelDefinition>) -> Result<(StatusCode, Json<Model>), ApiError> {
    let definition = validated(definition)?;
    let id = sqlx::query_scalar("INSERT INTO models (name, adapter, weights, options, class_map) VALUES (?, ?, ?, ?, ?) RETURNING id")
        .bind(&definition.name)
        .bind(&definition.adapter)
        .bind(&definition.weights)
        .bind(serde_json::to_string(&definition.options).expect("options serialise"))
        .bind(serde_json::to_string(&definition.class_map).expect("class maps serialise"))
        .fetch_one(&pool)
        .await
        .map_err(|err| name_taken(err, &definition.name))?;
    Ok((StatusCode::CREATED, Json(Model { id, definition })))
}

async fn change_model(_: Admin, State(pool): State<SqlitePool>, Path(id): Path<i64>, Json(definition): Json<ModelDefinition>) -> Result<Json<Model>, ApiError> {
    let definition = validated(definition)?;
    let changed = sqlx::query("UPDATE models SET name = ?, adapter = ?, weights = ?, options = ?, class_map = ? WHERE id = ?")
        .bind(&definition.name)
        .bind(&definition.adapter)
        .bind(&definition.weights)
        .bind(serde_json::to_string(&definition.options).expect("options serialise"))
        .bind(serde_json::to_string(&definition.class_map).expect("class maps serialise"))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|err| name_taken(err, &definition.name))?;
    if changed.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
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
    state.worker.classes(&model.definition).await.map(Json).map_err(ApiError::Conflict)
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
    fn update(&self, id: u64, change: impl FnOnce(&mut RunStatus)) {
        if let Some((status, _)) = self.0.lock().expect("runs lock").runs.get_mut(&id) {
            change(status);
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
async fn start_run(user: CurrentUser, State(state): State<AppState>, Path(project): Path<i64>, Json(request): Json<RunRequest>) -> Result<(StatusCode, Json<RunStatus>), ApiError> {
    let mut conn = state.pool.acquire().await?;
    jobs::ensure_project_visible(&mut conn, &user, project).await?;
    let model = model(&mut conn, request.model).await?;
    let image_ids: Vec<i64> = match request.job {
        Some(job) => sqlx::query_scalar("SELECT images.id FROM images JOIN jobs ON jobs.id = images.job_id WHERE jobs.id = ? AND jobs.project_id = ? ORDER BY images.path")
            .bind(job)
            .bind(project)
            .fetch_all(&mut *conn)
            .await?,
        None => request.image_ids.clone(),
    };
    if image_ids.is_empty() {
        return Err(ApiError::BadRequest("choose images to run the model on".into()));
    }
    for &image in &image_ids {
        let in_project: Option<i64> = sqlx::query_scalar("SELECT id FROM images WHERE id = ? AND project_id = ?").bind(image).bind(project).fetch_optional(&mut *conn).await?;
        in_project.ok_or_else(|| ApiError::BadRequest(format!("image {image} is not in this project")))?;
        jobs::ensure_image_access(&mut conn, &user, image).await?;
    }
    if state.worker.url.is_none() {
        return Err(ApiError::Conflict(state.worker.url("").unwrap_err()));
    }
    let labels = labels::project_labels(&mut conn, project).await?;
    drop(conn);

    let cancelled = Arc::new(AtomicBool::new(false));
    let status = {
        let mut table = state.runs.0.lock().expect("runs lock");
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
    tokio::spawn(run(state, status.id, project, model.definition, labels, image_ids, cancelled));
    Ok((StatusCode::ACCEPTED, Json(status)))
}

/// Runs the model image by image; between images it stops if cancelled.
async fn run(state: AppState, id: u64, project: i64, model: ModelDefinition, labels: Vec<Label>, image_ids: Vec<i64>, cancelled: Arc<AtomicBool>) {
    let skip = |reason: String| {
        state.runs.update(id, |status| match status.skipped.iter_mut().find(|skip| skip.reason == reason) {
            Some(skip) => skip.count += 1,
            None => status.skipped.push(SkipCount { reason, count: 1 }),
        })
    };
    let fail = |error: String| state.runs.update(id, |status| (status.state, status.error) = (RunState::Failed, Some(error)));
    let root = match images::image_root(&state) {
        Ok(root) => root,
        Err(_) => return fail("the image folder is not available".into()),
    };
    for image in image_ids {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        let row = match sqlx::query("SELECT path, width, height FROM images WHERE id = ?").bind(image).fetch_optional(&state.pool).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                skip("images removed meanwhile".into());
                continue;
            }
            Err(err) => return fail(err.to_string()),
        };
        let (path, width, height): (String, u32, u32) = (row.get("path"), row.get("width"), row.get("height"));
        let Ok(bytes) = tokio::fs::read(root.join(&path)).await else {
            skip("image files missing".into());
            continue;
        };
        let predictions = match state.worker.predict(&model, &bytes).await {
            Ok(predictions) => predictions,
            Err(error) => return fail(error),
        };
        // A run cancelled while the worker was busy keeps nothing of that image.
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        let mut made = Vec::new();
        for prediction in predictions {
            let wanted = model.class_map.get(&prediction.class).unwrap_or(&prediction.class);
            let Some(label) = labels.iter().find(|label| label.name.eq_ignore_ascii_case(wanted)) else {
                skip(format!("no label for class \"{}\"", prediction.class));
                continue;
            };
            let annotation = Annotation {
                source: Some(model.name.clone()),
                confidence: Some(prediction.confidence),
                pending: true,
                ..Annotation::manual(label.id, prediction.shape)
            };
            match check_annotation(label, &annotation, width, height) {
                Ok(()) => made.push(annotation),
                Err(reason) => skip(reason),
            }
        }
        match replace_pending(&state.pool, project, image, &model.name, &made).await {
            Ok(()) => state.runs.update(id, |status| (status.done, status.created) = (status.done + 1, status.created + made.len())),
            Err(err) => return fail(format!("saving failed: {}", err.message())),
        }
    }
    let finished = if cancelled.load(Ordering::SeqCst) { RunState::Cancelled } else { RunState::Finished };
    state.runs.update(id, |status| status.state = finished);
}

/// Puts `made` on the image in place of the model's unreviewed pre-annotations there.
async fn replace_pending(pool: &SqlitePool, project: i64, image: i64, model: &str, made: &[Annotation]) -> Result<(), ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("DELETE FROM annotations WHERE image_id = ? AND pending AND source = ?").bind(image).bind(model).execute(&mut *tx).await?;
    let first: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM annotations WHERE image_id = ?").bind(image).fetch_one(&mut *tx).await?;
    insert_annotations(&mut tx, project, image, first, made).await?;
    // An editor holding the old version is told to reload.
    sqlx::query("UPDATE images SET version = version + 1 WHERE id = ?").bind(image).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn visible_run(state: &AppState, user: &CurrentUser, id: u64) -> Result<(RunStatus, Arc<AtomicBool>), ApiError> {
    let (status, cancelled) = state.runs.get(id).ok_or(ApiError::NotFound)?;
    jobs::ensure_project_visible(&mut *state.pool.acquire().await?, user, status.project).await?;
    Ok((status, cancelled))
}

async fn run_status(user: CurrentUser, State(state): State<AppState>, Path(RunPath { id }): Path<RunPath>) -> Result<Json<RunStatus>, ApiError> {
    Ok(Json(visible_run(&state, &user, id).await?.0))
}

/// Stops a run after the image the worker is on.
async fn cancel_run(user: CurrentUser, State(state): State<AppState>, Path(RunPath { id }): Path<RunPath>) -> Result<StatusCode, ApiError> {
    visible_run(&state, &user, id).await?.1.store(true, Ordering::SeqCst);
    Ok(StatusCode::NO_CONTENT)
}
