use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub config: Option<sr_sources::db::models::SourceConfig>,
    pub health: Option<sr_sources::db::models::SourceHealth>,
}

pub async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<SourceInfo>>, ApiError> {
    let configs = sr_sources::db::queries::get_all_source_configs(&state.db).await?;
    let health = sr_sources::db::queries::get_all_source_health(&state.db).await?;

    // O(1) lookups instead of O(n*m) linear scans
    let config_map: std::collections::HashMap<&str, &sr_sources::db::models::SourceConfig> =
        configs.iter().map(|c| (c.source_id.as_str(), c)).collect();
    let health_map: std::collections::HashMap<&str, &sr_sources::db::models::SourceHealth> =
        health.iter().map(|h| (h.source_id.as_str(), h)).collect();

    let sources: Vec<SourceInfo> = state
        .source_registry
        .sources()
        .iter()
        .map(|s| {
            let cfg = config_map.get(s.id()).cloned().cloned()
                .or_else(|| Some(sr_sources::db::models::SourceConfig {
                    source_id: s.id().to_string(),
                    enabled: true,
                    poll_interval_secs: None,
                    api_key_encrypted: None,
                    extra_config: serde_json::json!({}),
                    updated_at: chrono::Utc::now(),
                }));
            let h = health_map.get(s.id()).cloned().cloned();
            SourceInfo {
                id: s.id().to_string(),
                name: s.name().to_string(),
                config: cfg,
                health: h,
            }
        })
        .collect();

    Ok(Json(sources))
}

pub async fn get_source_config(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<Option<sr_sources::db::models::SourceConfig>>, ApiError> {
    let config = sr_sources::db::queries::get_source_config(&state.db, &source_id).await?;
    Ok(Json(config))
}

#[derive(Deserialize)]
pub struct UpdateSourceConfig {
    pub enabled: bool,
    pub poll_interval_secs: Option<i32>,
    pub extra_config: Option<serde_json::Value>,
}

pub async fn update_source_config(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    Json(body): Json<UpdateSourceConfig>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let extra = body.extra_config.unwrap_or(serde_json::json!({}));
    sr_sources::db::queries::upsert_source_config(
        &state.db,
        &source_id,
        body.enabled,
        body.poll_interval_secs,
        &extra,
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn toggle_source(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = sr_sources::db::queries::get_source_config(&state.db, &source_id).await?;
    let new_enabled = config.map(|c| !c.enabled).unwrap_or(true);
    sr_sources::db::queries::upsert_source_config(
        &state.db,
        &source_id,
        new_enabled,
        None,
        &serde_json::json!({}),
    )
    .await?;
    Ok(Json(serde_json::json!({ "enabled": new_enabled })))
}
