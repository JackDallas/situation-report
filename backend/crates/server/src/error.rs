use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "API error");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": self.0.to_string() }).to_string(),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
