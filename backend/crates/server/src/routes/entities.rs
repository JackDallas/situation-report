//! Entity graph API endpoints.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct EntitiesParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct StateChangesParams {
    pub limit: Option<i64>,
}

/// GET /api/entities?limit=50
pub async fn list_entities(
    State(state): State<AppState>,
    Query(params): Query<EntitiesParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    match sr_pipeline::entity_graph::queries::get_top_entities(&state.db, limit).await {
        Ok(entities) => Json(serde_json::json!(entities)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// GET /api/entities/state-changes?limit=20
pub async fn list_state_changes(
    State(state): State<AppState>,
    Query(params): Query<StateChangesParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(20).min(100);
    match sr_pipeline::entity_graph::queries::get_recent_state_changes(&state.db, limit).await {
        Ok(changes) => Json(serde_json::json!(changes)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// GET /api/entities/{id}
pub async fn get_entity(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match sr_pipeline::entity_graph::queries::get_entity_detail(&state.db, id).await {
        Ok(Some(detail)) => Json(serde_json::json!(detail)),
        Ok(None) => Json(serde_json::json!({"error": "Entity not found"})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}
