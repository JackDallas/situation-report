use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_pipeline::SituationClusterDTO;
use uuid::Uuid;

use crate::state::AppState;

/// GET /api/situations — current active situation clusters (with latest narrative text)
pub async fn list_situations(
    State(state): State<AppState>,
) -> Json<Vec<SituationClusterDTO>> {
    let mut clusters = state.situations.read()
        .map(|lock| lock.clone())
        .unwrap_or_default();

    // Attach the latest narrative text to each cluster for display in the UI.
    // Single query fetches all latest narratives efficiently.
    let ids: Vec<Uuid> = clusters.iter().map(|c| c.id).collect();
    if !ids.is_empty() {
        if let Ok(rows) = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT DISTINCT ON (situation_id) situation_id, narrative_text \
             FROM situation_narratives \
             WHERE situation_id = ANY($1) \
             ORDER BY situation_id, version DESC"
        )
        .bind(&ids)
        .fetch_all(&state.db)
        .await
        {
            let narr_map: std::collections::HashMap<Uuid, String> =
                rows.into_iter().collect();
            for cluster in &mut clusters {
                if let Some(text) = narr_map.get(&cluster.id) {
                    cluster.narrative_text = Some(text.clone());
                }
            }
        }
    }

    Json(clusters)
}

/// GET /api/situations/:id — single cluster detail
pub async fn get_situation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Option<SituationClusterDTO>> {
    let clusters = state.situations.read()
        .map(|lock| lock.clone())
        .unwrap_or_default();
    let found = clusters.into_iter().find(|c| c.id.to_string() == id);
    Json(found)
}

// --- Narrative types ---

#[derive(Debug, Serialize, sqlx::FromRow)]
struct NarrativeRow {
    id: Uuid,
    situation_id: Uuid,
    version: i32,
    narrative_text: String,
    model: String,
    tokens_used: i32,
    generated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct NarrativeParams {
    pub limit: Option<i64>,
}

/// GET /api/situations/:id/narratives — intelligence narratives for a situation
pub async fn get_situation_narratives(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<NarrativeParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(5).min(20);
    match sqlx::query_as::<_, NarrativeRow>(
        "SELECT id, situation_id, version, narrative_text, model, tokens_used, generated_at \
         FROM situation_narratives WHERE situation_id = $1 \
         ORDER BY version DESC LIMIT $2",
    )
    .bind(id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// GET /api/situations/:id/events — events belonging to a situation
pub async fn get_situation_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<SituationEventsParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(200);

    // Look up cluster to get event IDs + time range
    let cluster = state.situations.read()
        .ok()
        .and_then(|lock| lock.iter().find(|c| c.id.to_string() == id).cloned());

    let cluster = match cluster {
        Some(c) => c,
        None => return Json(serde_json::json!([])),
    };

    // Extract source_ids from the cluster's event_ids
    let source_ids: Vec<String> = cluster.event_ids.iter()
        .map(|(_, sid)| sid.clone())
        .collect();

    if source_ids.is_empty() && cluster.entities.is_empty() {
        return Json(serde_json::json!([]));
    }

    // Primary strategy: match by source_id within the cluster's time range.
    // Fallback: also match by entity names for events that may not have source_ids.
    let entity_patterns: Vec<String> = cluster.entities.iter()
        .take(15)
        .map(|e| format!("%{}%", e))
        .collect();

    match sqlx::query_as::<_, sr_sources::db::models::Event>(
        r#"
        SELECT event_time, ingested_at, source_type, source_id,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               region_code, entity_id, entity_name, event_type,
               severity, confidence, tags, title, description, payload
        FROM events
        WHERE event_time BETWEEN $1 AND $2
          AND (
              source_id = ANY($3)
              OR entity_name = ANY($4)
              OR title ILIKE ANY($5)
          )
          AND event_type NOT IN ('flight_position', 'vessel_position', 'cert_issued', 'shodan_banner')
        ORDER BY event_time DESC
        LIMIT $6
        "#,
    )
    .bind(cluster.first_seen)
    .bind(cluster.last_updated)
    .bind(&source_ids)
    .bind(&cluster.entities)
    .bind(&entity_patterns)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => {
            tracing::warn!("Situation events query failed: {e}");
            Json(serde_json::json!([]))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SituationEventsParams {
    pub limit: Option<i64>,
}

/// GET /api/situations/:id/timeline — hourly activity timeline for a situation
pub async fn get_situation_timeline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match sr_sources::db::queries::query_situation_timeline(&state.db, id).await {
        Ok(buckets) => {
            let data: Vec<serde_json::Value> = buckets.into_iter().map(|b| {
                serde_json::json!({
                    "bucket": b.bucket,
                    "event_count": b.event_count,
                    "source_count": b.source_count,
                    "max_severity": b.max_severity,
                })
            }).collect();
            Json(serde_json::json!(data))
        }
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

/// GET /api/situations/:id/cameras — cameras near a situation
pub async fn get_situation_cameras(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Vec<sr_sources::shodan::CameraResult>> {
    let cameras = state.cameras.read()
        .map(|lock| lock.clone())
        .unwrap_or_default();
    // Look up by situation cluster ID
    let id_uuid = uuid::Uuid::parse_str(&id).ok();
    let result = id_uuid
        .and_then(|uuid| cameras.get(&uuid).cloned())
        .unwrap_or_default();
    Json(result)
}
