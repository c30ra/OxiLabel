//! A pre-annotation run: a model over images, one image at a time. Each image goes to the worker
//! port; its predictions become pending pre-annotations or skip reasons (the confidence threshold,
//! the model's class map, labels found by name), which the annotation store writes in place of the
//! model's unreviewed ones, refusing what a label or the image does not allow. Between images the
//! run stops if cancelled. Nothing here spawns or polls: `inference` drives it in the background.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use oxilabel_core::{
    Label,
    annotation::Annotation,
    import::SkipCount,
    inference::{ModelDefinition, Prediction, RunState, RunStatus},
    worker::{PredictRequest, WorkerModel},
};
use sqlx::SqlitePool;

use crate::{
    annotation_store::{self, Replacing, Written},
    error::ApiError,
    worker::WorkerPort,
};

const REMOVED: &str = "images removed meanwhile";

/// What a run works with, fixed when it starts.
pub(crate) struct PreAnnotationRun {
    pub pool: SqlitePool,
    /// The folder image paths are relative to.
    pub root: PathBuf,
    pub project: i64,
    pub model: ModelDefinition,
    /// The project's labels when the run started, found by name.
    pub labels: Vec<Label>,
    pub threshold: Option<f64>,
}

/// How an image went.
#[derive(Debug)]
pub(crate) enum Step {
    /// Its pre-annotations were written.
    Done,
    /// It was skipped for the reason tallied in the status.
    Skipped,
    /// The run was cancelled while the worker was on it; nothing of it was kept.
    Cancelled,
}

impl PreAnnotationRun {
    /// Runs over `images` in order, calling `publish` with the status after each image and at the
    /// end; returns the final status.
    pub(crate) async fn run(&self, worker: &impl WorkerPort, images: &[i64], cancelled: &AtomicBool, mut status: RunStatus, mut publish: impl FnMut(&RunStatus)) -> RunStatus {
        status.state = RunState::Finished;
        for &image in images {
            if cancelled.load(Ordering::SeqCst) {
                status.state = RunState::Cancelled;
                break;
            }
            match self.step(worker, image, cancelled, &mut status).await {
                Ok(Step::Done | Step::Skipped) => publish(&status),
                Ok(Step::Cancelled) => {
                    status.state = RunState::Cancelled;
                    break;
                }
                Err(error) => {
                    (status.state, status.error) = (RunState::Failed, Some(error));
                    break;
                }
            }
        }
        publish(&status);
        status
    }

    /// Pre-annotates one image, counting it in `status`; `Err` ends the run with that message.
    pub(crate) async fn step(&self, worker: &impl WorkerPort, image: i64, cancelled: &AtomicBool, status: &mut RunStatus) -> Result<Step, String> {
        let skip = |status: &mut RunStatus, reason: &str| {
            SkipCount::tally(&mut status.skipped, reason.to_owned(), 1);
            Ok(Step::Skipped)
        };
        let path: Option<String> = sqlx::query_scalar("SELECT path FROM images WHERE id = ?").bind(image).fetch_optional(&self.pool).await.map_err(|err| err.to_string())?;
        let Some(path) = path else {
            return skip(status, REMOVED);
        };
        let Ok(bytes) = tokio::fs::read(self.root.join(&path)).await else {
            return skip(status, "image files missing");
        };
        let request = PredictRequest { model: WorkerModel::from(&self.model), image: bytes };
        let predictions = worker.predict(&request).await.map_err(|error| error.to_string())?;
        // A run cancelled while the worker was busy keeps nothing of that image.
        if cancelled.load(Ordering::SeqCst) {
            return Ok(Step::Cancelled);
        }
        let made = self.pre_annotations(predictions, &mut status.skipped);
        match self.replace_pending(image, made).await {
            Ok(Some(written)) => {
                status.done += 1;
                status.created += written.stored;
                for skipped in written.skipped {
                    SkipCount::tally(&mut status.skipped, skipped.reason, skipped.count);
                }
                Ok(Step::Done)
            }
            Ok(None) => skip(status, REMOVED),
            Err(err) => Err(format!("saving failed: {}", err.message())),
        }
    }

    /// The predictions as pending pre-annotations of the model, tallying in `skipped` those under
    /// the threshold or without a label. The store checks shapes against the labels.
    fn pre_annotations(&self, predictions: Vec<Prediction>, skipped: &mut Vec<SkipCount>) -> Vec<Annotation> {
        let mut made = Vec::new();
        for prediction in predictions {
            if let Some(threshold) = self.threshold.filter(|threshold| prediction.confidence < *threshold) {
                SkipCount::tally(skipped, format!("less confident than {threshold}"), 1);
                continue;
            }
            let wanted = self.model.class_map.get(&prediction.class).unwrap_or(&prediction.class);
            let Some(label) = self.labels.iter().find(|label| label.name.eq_ignore_ascii_case(wanted)) else {
                SkipCount::tally(skipped, format!("no label for class \"{}\"", prediction.class), 1);
                continue;
            };
            made.push(Annotation {
                source: Some(self.model.name.clone()),
                confidence: Some(prediction.confidence),
                pending: true,
                ..Annotation::manual(label.id, prediction.shape)
            });
        }
        made
    }

    /// Puts `made` on the image in place of the model's unreviewed pre-annotations there, leaving
    /// out what the store refuses; `None` if the image is gone.
    async fn replace_pending(&self, image: i64, made: Vec<Annotation>) -> Result<Option<Written>, ApiError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let written = annotation_store::write(&mut tx, self.project, image, Replacing::Pending { source: &self.model.name }, made).await?;
        tx.commit().await?;
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use oxilabel_core::{
        annotation::Shape,
        geometry::{Point, Rect},
    };
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        fake_worker::{FakeRequest, FakeWorker},
        labels,
    };

    /// Project 1 with labels "Crack" (boxes and polygons, id 1) and "chip" (boxes, id 2), and
    /// `count` 20 × 10 images; the run of the fake model ("dent" becomes "chip") and the images.
    async fn setup(count: usize, threshold: Option<f64>) -> (PreAnnotationRun, Vec<i64>, TempDir) {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO projects (name) VALUES ('p')").execute(&mut *conn).await.unwrap();
        for definition in [
            json!({"name": "Crack", "color": "#ff0000", "shape_types": ["rectangle", "polygon"]}),
            json!({"name": "chip", "color": "#00ff00", "shape_types": ["rectangle"]}),
        ] {
            labels::insert_label(&mut conn, 1, serde_json::from_value(definition).unwrap()).await.unwrap_or_else(|err| panic!("{}", err.message()));
        }
        let root = TempDir::new().unwrap();
        let mut images = Vec::new();
        for i in 0..count {
            let path = format!("img{i}.png");
            std::fs::write(root.path().join(&path), [i as u8]).unwrap();
            let id = sqlx::query_scalar("INSERT INTO images (project_id, path, hash, width, height) VALUES (1, ?, ?, 20, 10) RETURNING id")
                .bind(&path)
                .bind(&path)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
            images.push(id);
        }
        let labels = labels::project_labels(&mut conn, 1).await.unwrap_or_else(|err| panic!("{}", err.message()));
        drop(conn);
        let model = ModelDefinition {
            name: "fake v1".into(),
            adapter: "ultralytics".into(),
            weights: "/models/fake.pt".into(),
            options: Default::default(),
            class_map: BTreeMap::from([("dent".into(), "chip".into())]),
        };
        let run = PreAnnotationRun { pool, root: root.path().to_owned(), project: 1, model, labels, threshold };
        (run, images, root)
    }

    fn status(total: usize) -> RunStatus {
        RunStatus { id: 1, project: 1, model: "fake v1".into(), state: RunState::Running, total, done: 0, created: 0, skipped: Vec::new(), error: None }
    }

    fn skipped(status: &RunStatus) -> Vec<(&str, usize)> {
        status.skipped.iter().map(|skip| (skip.reason.as_str(), skip.count)).collect()
    }

    fn found(class: &str, confidence: f64, shape: Shape) -> Prediction {
        Prediction { class: class.into(), confidence, shape }
    }

    fn rect(min_x: f64) -> Shape {
        Shape::Rectangle(Rect { min: Point { x: min_x, y: 1.0 }, max: Point { x: min_x + 2.0, y: 3.0 } })
    }

    /// The (label, source, confidence, pending) of the image's annotations.
    async fn stored(run: &PreAnnotationRun, image: i64) -> Vec<(i64, Option<String>, Option<f64>, bool)> {
        let mut conn = run.pool.acquire().await.unwrap();
        let list = annotation_store::load(&mut conn, 1, image).await.unwrap_or_else(|err| panic!("{}", err.message()));
        list.annotations.into_iter().map(|a| (a.label, a.source, a.confidence, a.pending)).collect()
    }

    fn predicted(worker: &FakeWorker) -> usize {
        worker.requests().iter().filter(|request| matches!(request, FakeRequest::Predict(_))).count()
    }

    #[tokio::test]
    async fn classes_map_to_labels_by_name_and_below_threshold_is_dropped() {
        let (run, images, _root) = setup(1, Some(0.5)).await;
        let mut worker = FakeWorker::default();
        worker.predictions = vec![found("CRACK", 0.9, rect(1.0)), found("dent", 0.6, rect(5.0)), found("crack", 0.3, rect(9.0))];
        let mut status = status(1);
        let step = run.step(&worker, images[0], &AtomicBool::new(false), &mut status).await.unwrap();
        assert!(matches!(step, Step::Done));
        assert_eq!((status.done, status.created), (1, 2));
        assert_eq!(skipped(&status), [("less confident than 0.5", 1)]);
        let model = Some("fake v1".to_owned());
        assert_eq!(stored(&run, images[0]).await, [(1, model.clone(), Some(0.9), true), (2, model, Some(0.6), true)]);
    }

    #[tokio::test]
    async fn unknown_classes_and_shapes_a_label_refuses_are_skipped_with_their_reason() {
        let (run, images, _root) = setup(1, None).await;
        let mut worker = FakeWorker::default();
        let triangle = Shape::Polygon { points: vec![Point { x: 1.0, y: 1.0 }, Point { x: 5.0, y: 1.0 }, Point { x: 1.0, y: 5.0 }] };
        worker.predictions = vec![found("smudge", 0.8, rect(1.0)), found("dent", 0.8, triangle), found("crack", 0.8, rect(30.0)), found("crack", 0.8, rect(1.0))];
        let mut status = status(1);
        run.step(&worker, images[0], &AtomicBool::new(false), &mut status).await.unwrap();
        assert_eq!(status.created, 1);
        assert_eq!(skipped(&status), [
            ("no label for class \"smudge\"", 1),
            ("label \"chip\" does not allow polygon shapes", 1),
            ("the shape must lie inside the 20 × 10 image", 1),
        ]);
    }

    /// The fake worker, but the image `removed` leaves the project while the worker is on it.
    struct Removing {
        fake: FakeWorker,
        pool: SqlitePool,
        removed: i64,
    }

    impl WorkerPort for Removing {
        async fn health(&self) -> Result<String, oxilabel_core::worker::WorkerError> {
            self.fake.health().await
        }

        async fn classes(&self, model: &WorkerModel) -> Result<Vec<String>, oxilabel_core::worker::WorkerError> {
            self.fake.classes(model).await
        }

        async fn predict(&self, request: &PredictRequest) -> Result<Vec<Prediction>, oxilabel_core::worker::WorkerError> {
            if predicted(&self.fake) == 1 {
                sqlx::query("DELETE FROM images WHERE id = ?").bind(self.removed).execute(&self.pool).await.unwrap();
            }
            self.fake.predict(request).await
        }
    }

    #[tokio::test]
    async fn an_image_removed_during_the_run_is_skipped() {
        let (run, images, _root) = setup(3, None).await;
        let worker = Removing { fake: FakeWorker::default(), pool: run.pool.clone(), removed: images[1] };
        let status = run.run(&worker, &images, &AtomicBool::new(false), status(3), |_| {}).await;
        assert_eq!((status.state, status.done, status.created), (RunState::Finished, 2, 4), "{status:?}");
        assert!(skipped(&status).contains(&(REMOVED, 1)), "{:?}", status.skipped);
        assert_eq!(predicted(&worker.fake), 3);
    }

    #[tokio::test]
    async fn a_cancelled_run_stops_between_images() {
        let (run, images, _root) = setup(4, None).await;
        let worker = FakeWorker::default();
        let cancelled = AtomicBool::new(false);
        let mut published = 0;
        let status = run
            .run(&worker, &images, &cancelled, status(4), |_| {
                published += 1;
                if published == 2 {
                    cancelled.store(true, Ordering::SeqCst);
                }
            })
            .await;
        assert_eq!((status.state, status.done), (RunState::Cancelled, 2));
        assert_eq!(predicted(&worker), 2, "no more images go to the worker");
        assert_eq!(stored(&run, images[2]).await, []);
    }

    #[tokio::test]
    async fn a_run_cancelled_while_the_worker_is_busy_keeps_nothing_of_that_image() {
        let (run, images, _root) = setup(1, None).await;
        let cancelled = AtomicBool::new(true);
        let mut status = status(1);
        let step = run.step(&FakeWorker::default(), images[0], &cancelled, &mut status).await.unwrap();
        assert!(matches!(step, Step::Cancelled));
        assert_eq!((status.done, stored(&run, images[0]).await), (0, vec![]));
    }

    #[tokio::test]
    async fn a_failing_worker_fails_the_run_with_its_message() {
        let (run, images, _root) = setup(2, None).await;
        let mut worker = FakeWorker::default();
        worker.failure = Some("out of GPU memory".into());
        let status = run.run(&worker, &images, &AtomicBool::new(false), status(2), |_| {}).await;
        assert_eq!(status.state, RunState::Failed);
        assert!(status.error.unwrap().contains("out of GPU memory"));
        assert_eq!(predicted(&worker), 1);
    }
}
