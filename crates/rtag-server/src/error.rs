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

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Database(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
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
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
