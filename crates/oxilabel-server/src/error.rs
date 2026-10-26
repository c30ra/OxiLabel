use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum ApiError {
    BadRequest(String),
    NotFound,
    /// The request has no valid session.
    Unauthorized(String),
    Forbidden(String),
    /// The request clashes with the current state, e.g. a save based on an outdated version.
    Conflict(String),
    Database(sqlx::Error),
    /// An unexpected failure; the detail is logged, not sent to the client.
    Internal(String),
}

impl ApiError {
    /// What went wrong, for a message that is not an HTTP response (e.g. a background run's).
    pub fn message(&self) -> String {
        match self {
            ApiError::BadRequest(message) | ApiError::Unauthorized(message) | ApiError::Forbidden(message) | ApiError::Conflict(message) | ApiError::Internal(message) => message.clone(),
            ApiError::NotFound => "not found".to_owned(),
            ApiError::Database(err) => err.to_string(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Database(err)
    }
}

impl ApiError {
    /// The status and message a client is told; the detail of an unexpected failure is logged.
    pub fn public(self) -> (StatusCode, String) {
        match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_owned()),
            ApiError::Unauthorized(message) => (StatusCode::UNAUTHORIZED, message),
            ApiError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            ApiError::Database(err) => {
                eprintln!("database error: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_owned())
            }
            ApiError::Internal(detail) => {
                eprintln!("internal error: {detail}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_owned())
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = self.public();
        (status, Json(json!({ "error": message }))).into_response()
    }
}
