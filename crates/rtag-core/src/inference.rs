//! Pre-annotation: models registered with the worker, and runs of a model over a project's
//! images. The worker runs a model through an *adapter* (see ADR 0001 and ticket 17): the bundled
//! `ultralytics` one or a Python file of the user's; rtag only knows its name, weights and options.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{annotation::Shape, import::SkipCount};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: i64,
    #[serde(flatten)]
    pub definition: ModelDefinition,
}

/// What an admin registers: a model as the worker loads it, and which label each of its classes
/// becomes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDefinition {
    /// Also the `source` of its pre-annotations.
    pub name: String,
    /// `ultralytics`, or the path of a Python adapter file on the worker's machine.
    pub adapter: String,
    /// The weights file, on the worker's machine.
    pub weights: String,
    /// Passed to the adapter as they are (e.g. `{"conf": 0.25, "imgsz": 1024}`).
    #[serde(default)]
    pub options: Map<String, Value>,
    /// Label names by class name; a class not listed becomes the project's label of the same
    /// name, ignoring case.
    #[serde(default)]
    pub class_map: BTreeMap<String, String>,
}

/// Whether the worker answers, and on what it runs models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerHealth {
    pub reachable: bool,
    /// E.g. "cuda:0 AMD Radeon RX 7800 XT", as the worker describes it.
    pub device: Option<String>,
    pub error: Option<String>,
}

/// One thing a model found on an image, as the worker returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    pub class: String,
    pub confidence: f64,
    /// In the image's pixels.
    pub shape: Shape,
}

/// Runs a model on images of a project: these images, or the images of a job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    pub model: i64,
    #[serde(default)]
    pub image_ids: Vec<i64>,
    #[serde(default)]
    pub job: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    Finished,
    Cancelled,
    Failed,
}

/// How a run is going: images done of all, pre-annotations made, predictions left out and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStatus {
    pub id: u64,
    pub project: i64,
    pub model: String,
    pub state: RunState,
    pub total: usize,
    pub done: usize,
    pub created: usize,
    pub skipped: Vec<SkipCount>,
    pub error: Option<String>,
}
