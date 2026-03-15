use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        tracing::error!("API error: {:?}", self.0);
        let body = Json(serde_json::json!({
            "error": "Internal server error"
        }));
        (status, body).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
