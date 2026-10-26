//! Long operations that report how far they are. A request that accepts `text/event-stream`
//! gets `progress` events while the work runs, then one `done` event with the result or an
//! `error` event; closing it cancels the work. Any other request gets the result as JSON once
//! the work is done, as usual.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::FromRequestParts,
    http::{HeaderName, header, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use oxilabel_core::Progress;
use serde::Serialize;
use serde_json::json;
use tokio::sync::{oneshot, watch};

use crate::error::ApiError;

/// The least time between two progress events; the steps in between are only counted.
const INTERVAL: Duration = Duration::from_millis(100);

/// Whether the request asked for progress events.
pub struct Streaming(bool);

impl<S: Send + Sync> FromRequestParts<S> for Streaming {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let wanted = parts.headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).any(|value| value.contains("text/event-stream"));
        Ok(Streaming(wanted))
    }
}

/// Where an operation reports how far it is. The default one reports to no one.
#[derive(Default)]
pub struct Reporter(Option<watch::Sender<Progress>>);

impl Reporter {
    /// Records that `done` of `total` steps are done. False once the client has gone away, when
    /// the operation should stop.
    pub fn report(&self, done: u64, total: u64) -> bool {
        self.0.as_ref().is_none_or(|sender| sender.send(Progress { done, total }).is_ok())
    }
}

impl Streaming {
    /// Runs `work` and answers with its result, as JSON or (if asked for) as events after its
    /// progress.
    pub async fn run<T, F>(self, work: impl FnOnce(Reporter) -> F) -> Response
    where
        T: Serialize + Send + 'static,
        F: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        if !self.0 {
            return match work(Reporter::default()).await {
                Ok(result) => Json(result).into_response(),
                Err(err) => err.into_response(),
            };
        }
        let (progress, mut changes) = watch::channel(Progress::default());
        changes.mark_unchanged();
        let (finished, mut outcome) = oneshot::channel();
        let work = work(Reporter(Some(progress)));
        // On its own task, so that it goes on between the events' polls.
        tokio::spawn(async move { finished.send(work.await) });

        let events = futures_util::stream::unfold(Some((changes, outcome)), |state| async move {
            let (mut changes, mut outcome) = state?;
            tokio::select! {
                biased;
                result = &mut outcome => Some((Ok(last_event(result)), None)),
                // Once the work drops its reporter only its result is waited for.
                Ok(()) = changes.changed() => {
                    let progress = *changes.borrow_and_update();
                    tokio::time::sleep(INTERVAL).await;
                    Some((Ok::<_, Infallible>(json_event("progress", &progress)), Some((changes, outcome))))
                }
            }
        });
        // Proxies such as nginx would otherwise hold the events back.
        ([(HeaderName::from_static("x-accel-buffering"), "no")], Sse::new(events).keep_alive(KeepAlive::default())).into_response()
    }
}

fn last_event<T: Serialize>(result: Result<Result<T, ApiError>, oneshot::error::RecvError>) -> Event {
    match result {
        Ok(Ok(result)) => json_event("done", &result),
        Ok(Err(err)) => json_event("error", &json!({ "error": err.public().1 })),
        Err(_) => json_event("error", &json!({ "error": "internal error" })),
    }
}

fn json_event(name: &str, data: &impl Serialize) -> Event {
    Event::default().event(name).data(serde_json::to_string(data).expect("results serialize"))
}
