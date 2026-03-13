use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct IncidentParams {
    pub limit: Option<i64>,
    pub since: Option<DateTime<Utc>>,
}

fn severity_label(rank: i32) -> &'static str {
    match rank {
        4 => "critical",
        3 => "high",
        2 => "medium",
        1 => "low",
        _ => "info",
    }
}

/// GET /api/incidents — recent correlated incidents
pub async fn list_incidents(
    State(state): State<AppState>,
    Query(params): Query<IncidentParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    let since = params.since.unwrap_or_else(|| Utc::now() - chrono::Duration::hours(72));

    // DISTINCT ON (rule_id, title) deduplicates ongoing events like coordinated_shutdown
    // that fire multiple times — only the most recent instance is returned.
    let rows = sqlx::query(
        r#"
        SELECT * FROM (
            SELECT DISTINCT ON (rule_id, title)
                   id, rule_id, title, description, severity, confidence,
                   first_seen, last_updated, region_code,
                   ST_Y(location::geometry) as latitude,
                   ST_X(location::geometry) as longitude,
                   tags, evidence, parent_id, display_title
            FROM incidents
            WHERE first_seen >= $1
            ORDER BY rule_id, title, first_seen DESC
        ) deduped
        ORDER BY first_seen DESC
        LIMIT $2
        "#,
    )
    .bind(since)
    .bind(limit)
    .fetch_all(&state.db)
    .await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let incidents: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<uuid::Uuid, _>("id"),
                        "rule_id": r.get::<String, _>("rule_id"),
                        "title": r.get::<String, _>("title"),
                        "description": r.get::<String, _>("description"),
                        "severity": severity_label(r.get::<i32, _>("severity")),
                        "confidence": r.get::<f32, _>("confidence"),
                        "first_seen": r.get::<DateTime<Utc>, _>("first_seen"),
                        "last_updated": r.get::<DateTime<Utc>, _>("last_updated"),
                        "region_code": r.get::<Option<String>, _>("region_code"),
                        "latitude": r.get::<Option<f64>, _>("latitude"),
                        "longitude": r.get::<Option<f64>, _>("longitude"),
                        "tags": r.get::<Vec<String>, _>("tags"),
                        "evidence": r.get::<serde_json::Value, _>("evidence"),
                        "parent_id": r.get::<Option<uuid::Uuid>, _>("parent_id"),
                        "display_title": r.get::<Option<String>, _>("display_title"),
                    })
                })
                .collect();
            Json(serde_json::json!(incidents))
        }
        Err(e) => {
            tracing::warn!("Failed to fetch incidents: {e}");
            Json(serde_json::json!([]))
        }
    }
}
