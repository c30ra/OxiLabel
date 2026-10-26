//! The pre-annotation worker's requests and replies (ADR 0001), as `worker/oxilabel_worker/server.py`
//! puts them on the wire: `GET /health` → `HealthReply`; `POST /classes` with a `WorkerModel` →
//! `ClassesReply`; `POST /predict` with a `PredictRequest` → `PredictReply`. A failure is a non-2xx
//! status with an `ErrorReply`.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::inference::{ModelDefinition, Prediction};

/// A model as the worker loads it: the part of a `ModelDefinition` the worker sees. Also the
/// body of `POST /classes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerModel {
    pub adapter: String,
    pub weights: String,
    #[serde(default)]
    pub options: Map<String, Value>,
}

impl From<&ModelDefinition> for WorkerModel {
    fn from(definition: &ModelDefinition) -> Self {
        WorkerModel { adapter: definition.adapter.clone(), weights: definition.weights.clone(), options: definition.options.clone() }
    }
}

/// Runs a model on one image, sent as its file's bytes (base64 on the wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictRequest {
    #[serde(flatten)]
    pub model: WorkerModel,
    #[serde(with = "base64_bytes")]
    pub image: Vec<u8>,
}

/// `GET /health`: what the worker runs models on, e.g. "cuda:0 AMD Radeon RX 7800 XT".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthReply {
    pub device: String,
}

/// `POST /classes`: the model's class names, as the worker reads them from the weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassesReply {
    pub classes: Vec<String>,
}

/// `POST /predict`: what the model found on the image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictReply {
    pub predictions: Vec<Prediction>,
}

/// The body of a failing status: the worker's reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorReply {
    pub error: String,
}

/// Why a request to the worker brought no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerError {
    /// Nothing answered at the worker's address.
    Unreachable(String),
    /// The worker answered with its reason (an `ErrorReply`), or with a failing status alone (a
    /// proxy, a crash).
    Failed(String),
    /// The answer is not the wire format.
    Unreadable(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerError::Unreachable(reason) => write!(f, "the worker did not answer: {reason}"),
            WorkerError::Failed(reason) => write!(f, "the worker failed: {reason}"),
            WorkerError::Unreadable(reason) => write!(f, "the worker's answer is unreadable: {reason}"),
        }
    }
}

impl std::error::Error for WorkerError {}

/// Bytes as standard base64 text.
mod base64_bytes {
    use base64::{Engine, display::Base64Display, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&Base64Display::new(bytes, &STANDARD))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        STANDARD.decode(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        ShapeType,
        annotation::Shape,
        ellipse::Ellipse,
        geometry::{Point, Rect},
        mask::Mask,
        oriented::OrientedRect,
    };

    fn model() -> WorkerModel {
        WorkerModel { adapter: "ultralytics".into(), weights: "/models/best.pt".into(), options: json!({"conf": 0.25}).as_object().unwrap().clone() }
    }

    #[test]
    fn a_classes_request_is_the_model_as_the_worker_loads_it() {
        assert_eq!(serde_json::to_value(model()).unwrap(), json!({"adapter": "ultralytics", "weights": "/models/best.pt", "options": {"conf": 0.25}}));
    }

    #[test]
    fn a_prediction_request_is_the_model_and_the_image_file_base64_encoded() {
        let request = PredictRequest { model: model(), image: b"12345".to_vec() };
        let wire = json!({"adapter": "ultralytics", "weights": "/models/best.pt", "options": {"conf": 0.25}, "image": "MTIzNDU="});
        assert_eq!(serde_json::to_value(&request).unwrap(), wire);
        assert_eq!(serde_json::from_value::<PredictRequest>(wire).unwrap(), request);
    }

    /// The replies as `worker/tests/test_worker.py` sees the Python worker write them.
    #[test]
    fn replies_read_as_the_python_worker_writes_them() {
        let health: HealthReply = serde_json::from_value(json!({"device": "cpu (no PyTorch)"})).unwrap();
        assert_eq!(health.device, "cpu (no PyTorch)");
        let classes: ClassesReply = serde_json::from_value(json!({"classes": ["crack", "chip"]})).unwrap();
        assert_eq!(classes.classes, ["crack", "chip"]);
        let reply = json!({"predictions": [{"class": "crack", "confidence": 0.75, "shape": {"type": "rectangle", "min": {"x": 0.0, "y": 0.0}, "max": {"x": 5.0, "y": 1.0}}}]});
        let predictions: PredictReply = serde_json::from_value(reply).unwrap();
        let rectangle = Shape::Rectangle(Rect { min: Point { x: 0.0, y: 0.0 }, max: Point { x: 5.0, y: 1.0 } });
        assert_eq!(predictions.predictions, [Prediction { class: "crack".into(), confidence: 0.75, shape: rectangle }]);
        let error: ErrorReply = serde_json::from_value(json!({"error": "RuntimeError: cannot read broken.pt"})).unwrap();
        assert_eq!(error.error, "RuntimeError: cannot read broken.pt");
    }

    #[test]
    fn a_failure_says_what_the_worker_did() {
        assert_eq!(WorkerError::Failed("out of GPU memory".into()).to_string(), "the worker failed: out of GPU memory");
        assert_eq!(WorkerError::Unreachable("connection refused".into()).to_string(), "the worker did not answer: connection refused");
        assert_eq!(WorkerError::Unreadable("expected value".into()).to_string(), "the worker's answer is unreadable: expected value");
    }

    /// Every shape the Python worker's helpers write (`worker/tests/test_worker.py` checks the same
    /// file against them) is read as the shape it stands for.
    #[test]
    fn every_shape_the_python_worker_writes_is_read() {
        #[derive(Deserialize)]
        struct Case {
            helper: String,
            shape: Shape,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../worker/tests/shapes.json")).unwrap();
        let p = |x, y| Point { x, y };
        let expected = [
            ("rectangle", Shape::Rectangle(Rect { min: p(1.0, 1.0), max: p(9.0, 6.0) })),
            ("oriented_rectangle", Shape::OrientedRectangle(OrientedRect { centre: p(10.0, 5.0), width: 4.0, height: 2.0, angle: 0.5 })),
            ("oriented_from_corners", Shape::OrientedRectangle(OrientedRect { centre: p(2.0, 1.0), width: 4.0, height: 2.0, angle: 0.0 })),
            ("polygon", Shape::Polygon { points: vec![p(0.0, 0.0), p(4.0, 0.0), p(0.0, 3.0)] }),
            ("polyline", Shape::Polyline { points: vec![p(0.0, 0.0), p(4.0, 4.0)] }),
            ("points", Shape::Points { points: vec![p(3.0, 3.0), p(5.0, 1.0)] }),
            ("ellipse", Shape::Ellipse(Ellipse { centre: p(10.0, 5.0), rx: 4.0, ry: 2.0, angle: 0.25 })),
            ("tag", Shape::Tag),
            ("mask", Shape::Mask(Mask { left: 1, top: 1, width: 2, height: 2, rle: vec![0, 2, 1, 1] })),
        ];
        let read: Vec<(&str, Shape)> = cases.iter().map(|case| (case.helper.as_str(), case.shape.clone())).collect();
        assert_eq!(read, expected);
        let missing: Vec<ShapeType> = ShapeType::ALL.into_iter().filter(|kind| !cases.iter().any(|case| case.shape.shape_type() == *kind)).collect();
        assert_eq!(missing, [], "every shape type");
    }
}
