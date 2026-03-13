use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use crate::error::ApiError;
use crate::state::AppState;
use crate::validate;

#[derive(Debug, Deserialize)]
pub struct PositionQuery {
    pub source_type: Option<String>,
    pub min_lat: Option<f64>,
    pub min_lon: Option<f64>,
    pub max_lat: Option<f64>,
    pub max_lon: Option<f64>,
    pub since: Option<String>,
    /// If true, return all vessels including low-interest civilian/inland.
    /// Default false: filters AIS to only military, tankers, cargo >100m, and fast movers.
    pub all_vessels: Option<bool>,
}

/// AIS ship_type codes considered interesting for OSINT display.
/// Military (35=mil ops, 55=law enforcement), tankers (80-89), cargo (70-79),
/// and special craft (50-59 includes SAR, pilot, tug).
/// We keep all aircraft sources unfiltered.
fn is_interesting_vessel(pos: &sr_sources::db::models::LatestPosition) -> bool {
    // Non-AIS sources always pass through
    if pos.source_type != "ais" {
        return true;
    }

    let payload = &pos.payload;

    // Military MMSI flag from source
    if payload.get("is_military").and_then(|v| v.as_bool()).unwrap_or(false) {
        return true;
    }

    // Ship type filtering — keep tankers (80-89), cargo (70-79), military ops (35),
    // law enforcement (55), and other interesting types
    if let Some(st) = payload.get("ship_type").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))) {
        match st {
            35 | 55 => return true,         // Military ops, law enforcement
            50..=59 => return true,          // SAR, pilot, tug (port security interest)
            70..=79 => return true,          // Cargo
            80..=89 => return true,          // Tanker
            _ => {}
        }
    }

    // Fast movers (>15 knots) are interesting regardless of type
    if pos.speed.unwrap_or(0.0) > 15.0 {
        return true;
    }

    false
}

pub async fn list_positions(
    State(state): State<AppState>,
    Query(params): Query<PositionQuery>,
) -> Result<Json<Vec<sr_sources::db::models::LatestPosition>>, ApiError> {
    let bbox = match (params.min_lat, params.min_lon, params.max_lat, params.max_lon) {
        (Some(min_lat), Some(min_lon), Some(max_lat), Some(max_lon)) => {
            Some((
                validate::clamp_lon(min_lon),
                validate::clamp_lat(min_lat),
                validate::clamp_lon(max_lon),
                validate::clamp_lat(max_lat),
            ))
        }
        _ => None,
    };
    let since = params.since.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let positions = sr_sources::db::queries::query_latest_positions(
        &state.db,
        params.source_type.as_deref(),
        bbox,
        since,
    )
    .await
    .map_err(ApiError::from)?;

    // Filter low-interest AIS vessels unless all_vessels=true
    let show_all = params.all_vessels.unwrap_or(false);
    let filtered = if show_all {
        positions
    } else {
        positions.into_iter().filter(is_interesting_vessel).collect()
    };

    Ok(Json(filtered))
}

#[derive(Debug, Deserialize)]
pub struct TrailQuery {
    pub hours: Option<f64>,
    pub limit: Option<i64>,
}

pub async fn get_position_trail(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Query(params): Query<TrailQuery>,
) -> Result<Json<Vec<sr_sources::db::queries::TrailPoint>>, ApiError> {
    let hours = validate::clamp_hours(params.hours.unwrap_or(2.0));
    let limit = validate::clamp_limit(params.limit.unwrap_or(500), 2000);
    let trail = sr_sources::db::queries::get_position_trail(
        &state.db,
        &entity_id,
        hours,
        limit,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(trail))
}
