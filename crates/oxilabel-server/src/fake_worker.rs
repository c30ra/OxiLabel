//! A fake pre-annotation worker for the server's and the CLI's tests (feature `fake-worker`).
//! It answers in process through the worker port, or over HTTP exactly as the Python worker does
//! (`serve`), so the server's HTTP adapter is tested against it.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use oxilabel_core::{
    annotation::Shape,
    api::ErrorReply,
    geometry::{Point, Rect},
    inference::Prediction,
    worker::{ClassesReply, HealthReply, PredictReply, PredictRequest, WorkerError, WorkerModel},
};

use crate::worker::WorkerPort;

/// A request the fake was given.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeRequest {
    Classes(WorkerModel),
    Predict(PredictRequest),
}

/// Runs any model on any image alike. By default it runs on "fake gpu", knows the classes "crack",
/// "dent" and "smudge", and finds on every image: a crack box (1, 1)–(9, 6) at 0.91, a dent box
/// (2, 2)–(4, 4) at 0.4, a smudge point at 0.8, and a crack polygon reaching x = 500 at 0.7.
#[derive(Debug, Clone)]
pub struct FakeWorker {
    pub device: String,
    pub classes: Vec<String>,
    pub predictions: Vec<Prediction>,
    /// Set: asking for classes or predictions fails with this, as the worker's own reason.
    pub failure: Option<String>,
    /// How long each prediction takes.
    pub delay: Duration,
    /// Shared by the clones, so a test keeps a clone to see what a served fake was asked.
    requests: Arc<Mutex<Vec<FakeRequest>>>,
}

impl Default for FakeWorker {
    fn default() -> Self {
        let point = |x, y| Point { x, y };
        let found = |class: &str, confidence, shape| Prediction { class: class.into(), confidence, shape };
        FakeWorker {
            device: "fake gpu".into(),
            classes: ["crack", "dent", "smudge"].map(String::from).to_vec(),
            predictions: vec![
                found("crack", 0.91, Shape::Rectangle(Rect { min: point(1.0, 1.0), max: point(9.0, 6.0) })),
                found("dent", 0.4, Shape::Rectangle(Rect { min: point(2.0, 2.0), max: point(4.0, 4.0) })),
                found("smudge", 0.8, Shape::Points { points: vec![point(3.0, 3.0)] }),
                found("crack", 0.7, Shape::Polygon { points: vec![point(0.0, 0.0), point(500.0, 0.0), point(0.0, 5.0)] }),
            ],
            failure: None,
            delay: Duration::ZERO,
            requests: Arc::default(),
        }
    }
}

impl FakeWorker {
    /// The requests given so far, in order.
    pub fn requests(&self) -> Vec<FakeRequest> {
        self.requests.lock().expect("fake worker lock").clone()
    }

    fn take(&self, request: FakeRequest) -> Result<(), WorkerError> {
        self.requests.lock().expect("fake worker lock").push(request);
        self.failure.clone().map_or(Ok(()), |reason| Err(WorkerError::Failed(reason)))
    }

    /// Answers over HTTP on a free local port, as the Python worker does; returns its URL.
    pub async fn serve(self) -> String {
        let app = Router::new()
            .route("/health", get(|State(worker): State<FakeWorker>| async move { reply(worker.health().await.map(|device| HealthReply { device })) }))
            .route(
                "/classes",
                post(|State(worker): State<FakeWorker>, Json(model): Json<WorkerModel>| async move { reply(worker.classes(&model).await.map(|classes| ClassesReply { classes })) }),
            )
            .route(
                "/predict",
                post(|State(worker): State<FakeWorker>, Json(request): Json<PredictRequest>| async move {
                    reply(worker.predict(&request).await.map(|predictions| PredictReply { predictions }))
                }),
            )
            .with_state(self);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a free local port");
        let url = format!("http://{}", listener.local_addr().expect("the fake worker's address"));
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serving the fake worker") });
        url
    }
}

/// A reply, or a failing status with the reason, as the Python worker answers.
fn reply<T>(answer: Result<T, WorkerError>) -> Result<Json<T>, (StatusCode, Json<ErrorReply>)> {
    answer.map(Json).map_err(|err| {
        let error = match err {
            WorkerError::Failed(reason) => reason,
            other => other.to_string(),
        };
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorReply { error }))
    })
}

impl WorkerPort for FakeWorker {
    async fn health(&self) -> Result<String, WorkerError> {
        Ok(self.device.clone())
    }

    async fn classes(&self, model: &WorkerModel) -> Result<Vec<String>, WorkerError> {
        self.take(FakeRequest::Classes(model.clone()))?;
        Ok(self.classes.clone())
    }

    async fn predict(&self, request: &PredictRequest) -> Result<Vec<Prediction>, WorkerError> {
        tokio::time::sleep(self.delay).await;
        self.take(FakeRequest::Predict(request.clone()))?;
        Ok(self.predictions.clone())
    }
}
