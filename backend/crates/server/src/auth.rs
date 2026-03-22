use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

/// API key authentication middleware.
/// If `AppState::api_key` is None, all requests pass through (dev mode).
/// If set, requires `Authorization: Bearer <key>` header or `?api_key=<key>` query parameter.
pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let expected = match &state.api_key {
        Some(key) => key,
        None => return Ok(next.run(req).await), // No key configured — dev mode
    };

    // Check Authorization: Bearer <key> header
    let header_match = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected.as_str());

    if header_match {
        return Ok(next.run(req).await);
    }

    // Check ?api_key=<key> query parameter (useful for WebSocket upgrade)
    let query_match = req
        .uri()
        .query()
        .into_iter()
        .flat_map(|q| q.split('&'))
        .filter_map(|pair| pair.strip_prefix("api_key="))
        .any(|key| key == expected.as_str());

    if query_match {
        return Ok(next.run(req).await);
    }

    Err((StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"error": "unauthorized"}))).into_response())
}
