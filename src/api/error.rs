use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    code: &'static str,
    message: String,
}

pub fn err(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> axum::response::Response {
    (
        status,
        Json(ApiError {
            code,
            message: message.into(),
        }),
    )
        .into_response()
}
