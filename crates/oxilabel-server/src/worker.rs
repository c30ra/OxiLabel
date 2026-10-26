//! The port the server talks to the pre-annotation worker through, with its requests and replies
//! typed in `oxilabel_core::worker`, and the port's HTTP adapter for the Python worker (`worker/`,
//! ADR 0001). Starting the worker's process is `worker_process`'s, apart from this.

use std::{
    future::Future,
    sync::{Arc, RwLock},
    time::Duration,
};

use oxilabel_core::{
    api::ErrorReply,
    inference::Prediction,
    worker::{ClassesReply, HealthReply, PredictReply, PredictRequest, WorkerError, WorkerModel},
};
use serde::de::DeserializeOwned;

/// Loading a model can take a while the first time; a prediction on a large photo too.
const PREDICT_TIMEOUT: Duration = Duration::from_secs(600);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// What the server asks of a pre-annotation worker.
pub trait WorkerPort: Send + Sync {
    /// What the worker runs models on, e.g. "cuda:0 AMD Radeon RX 7800 XT".
    fn health(&self) -> impl Future<Output = Result<String, WorkerError>> + Send;
    /// The model's class names, as the worker reads them from the weights.
    fn classes(&self, model: &WorkerModel) -> impl Future<Output = Result<Vec<String>, WorkerError>> + Send;
    /// What the model finds on one image, in the image's pixels.
    fn predict(&self, request: &PredictRequest) -> impl Future<Output = Result<Vec<Prediction>, WorkerError>> + Send;
}

/// The Python worker, answering over HTTP at `url`.
#[derive(Clone)]
pub(crate) struct HttpWorker {
    url: String,
    client: reqwest::Client,
}

impl HttpWorker {
    /// Sends a request and reads the reply, or the worker's reason for failing.
    async fn call<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T, WorkerError> {
        let response = request.send().await.map_err(|err| WorkerError::Unreachable(err.to_string()))?;
        let status = response.status();
        let body = response.bytes().await.map_err(|err| WorkerError::Unreadable(err.to_string()))?;
        if !status.is_success() {
            // A proxy or a crash may answer without the worker's reason.
            let reason = serde_json::from_slice::<ErrorReply>(&body).map_or_else(|_| format!("status {status}"), |reply| reply.error);
            return Err(WorkerError::Failed(reason));
        }
        serde_json::from_slice(&body).map_err(|err| WorkerError::Unreadable(err.to_string()))
    }
}

impl WorkerPort for HttpWorker {
    async fn health(&self) -> Result<String, WorkerError> {
        let reply: HealthReply = self.call(self.client.get(format!("{}/health", self.url)).timeout(HEALTH_TIMEOUT)).await?;
        Ok(reply.device)
    }

    async fn classes(&self, model: &WorkerModel) -> Result<Vec<String>, WorkerError> {
        let reply: ClassesReply = self.call(self.client.post(format!("{}/classes", self.url)).json(model).timeout(PREDICT_TIMEOUT)).await?;
        Ok(reply.classes)
    }

    async fn predict(&self, request: &PredictRequest) -> Result<Vec<Prediction>, WorkerError> {
        let reply: PredictReply = self.call(self.client.post(format!("{}/predict", self.url)).json(request).timeout(PREDICT_TIMEOUT)).await?;
        Ok(reply.predictions)
    }
}

/// The worker the server talks to now: the Python worker at the URL an admin set, if any.
#[derive(Clone, Default)]
pub(crate) struct Worker {
    current: Arc<RwLock<Option<HttpWorker>>>,
    client: reqwest::Client,
}

impl Worker {
    /// Talks to the worker at `url` from now on (none: no worker).
    pub(crate) fn point_at(&self, url: Option<String>) {
        *self.current.write().expect("worker lock") = url.map(|url| HttpWorker { url, client: self.client.clone() });
    }

    /// The worker, unless none is set up.
    pub(crate) fn current(&self) -> Option<HttpWorker> {
        self.current.read().expect("worker lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, http::StatusCode, routing::get};

    use super::*;

    /// A worker at a free local port answering `/health` with `status` and `body`.
    async fn answering(status: StatusCode, body: &'static str) -> HttpWorker {
        let app = Router::new().route("/health", get(move || async move { (status, body) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        HttpWorker { url, client: reqwest::Client::new() }
    }

    #[tokio::test]
    async fn an_answer_off_the_wire_format_is_a_failure_or_unreadable() {
        let proxy = answering(StatusCode::BAD_GATEWAY, "<html>bad gateway</html>").await;
        assert_eq!(proxy.health().await, Err(WorkerError::Failed("status 502 Bad Gateway".into())));
        let garbled = answering(StatusCode::OK, "{\"devices\": []}").await;
        assert!(matches!(garbled.health().await, Err(WorkerError::Unreadable(_))));
    }
}
