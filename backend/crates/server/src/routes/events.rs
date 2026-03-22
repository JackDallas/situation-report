use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub source: Option<String>,
    pub event_type: Option<String>,
    pub region: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub exclude: Option<String>,
}

pub async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<Vec<sr_sources::db::models::Event>>, ApiError> {
    let exclude_types = params
        .exclude
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect::<Vec<_>>());
    let filter = sr_sources::db::queries::EventFilter {
        source_type: params.source,
        event_type: params.event_type,
        region: params.region,
        since: params.since,
        limit: params.limit,
        offset: params.offset,
        exclude_types,
    };
    let events = sr_sources::db::queries::query_events(&state.db, &filter).await?;
    Ok(Json(events))
}

#[derive(Debug, Deserialize)]
pub struct GeoQuery {
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub types: Option<String>,
    pub exclude: Option<String>,
    pub min_lon: Option<f64>,
    pub min_lat: Option<f64>,
    pub max_lon: Option<f64>,
    pub max_lat: Option<f64>,
    /// Map zoom level (0-22). Controls data density:
    /// zoom 0-3: critical/high only, limit 100
    /// zoom 4-6: medium+, limit 300
    /// zoom 7+: all severities, limit 500
    pub zoom: Option<f64>,
}

/// Map zoom level to minimum severity filter and adaptive limit.
fn zoom_to_severity_filter(zoom: Option<f64>) -> (Option<Vec<String>>, i64) {
    match zoom {
        Some(z) if z < 4.0 => {
            // Very zoomed out: only show critical & high severity
            (Some(vec!["critical".into(), "high".into()]), 100)
        }
        Some(z) if z < 7.0 => {
            // Medium zoom: medium and above
            (
                Some(vec!["critical".into(), "high".into(), "medium".into()]),
                300,
            )
        }
        _ => {
            // Close zoom or no zoom specified: everything
            (None, 500)
        }
    }
}

pub async fn events_geo(
    State(state): State<AppState>,
    Query(params): Query<GeoQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let include_types: Option<Vec<String>> = params
        .types
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
    let exclude_types: Option<Vec<String>> = params
        .exclude
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

    // Zoom-aware severity filter and adaptive limit
    let zoom = params.zoom.map(validate::clamp_zoom);
    let (severity_filter, zoom_limit) = zoom_to_severity_filter(zoom);
    let limit = validate::clamp_limit(params.limit.unwrap_or(zoom_limit), 2000);

    // Build optional bbox from query parameters, with 20% padding for smooth panning
    let bbox = match (params.min_lon, params.min_lat, params.max_lon, params.max_lat) {
        (Some(west), Some(south), Some(east), Some(north)) => {
            let west = validate::clamp_lon(west);
            let east = validate::clamp_lon(east);
            let south = validate::clamp_lat(south);
            let north = validate::clamp_lat(north);
            let lon_pad = (east - west) * 0.2;
            let lat_pad = (north - south) * 0.2;
            Some((
                (west - lon_pad).max(-180.0),
                (south - lat_pad).max(-90.0),
                (east + lon_pad).min(180.0),
                (north + lat_pad).min(90.0),
            ))
        }
        _ => None,
    };
    let geojson = sr_sources::db::queries::get_events_geojson(
        &state.db,
        params.since,
        limit,
        include_types.as_deref(),
        exclude_types.as_deref(),
        bbox,
        severity_filter.as_deref(),
    )
    .await?;
    Ok(Json(geojson))
}

pub async fn latest_events(
    State(state): State<AppState>,
) -> Result<Json<Vec<sr_sources::db::models::Event>>, ApiError> {
    let filter = sr_sources::db::queries::EventFilter {
        limit: Some(50),
        ..Default::default()
    };
    let events = sr_sources::db::queries::query_events(&state.db, &filter).await?;
    Ok(Json(events))
}

pub async fn event_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let stats = sr_sources::db::queries::get_event_stats(&state.db).await?;
    Ok(Json(stats))
}
