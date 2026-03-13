//! Intelligence report API endpoints.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct IntelReport {
    pub id: Uuid,
    pub report_type: String,
    pub title: String,
    pub content_json: serde_json::Value,
    pub content_html: Option<String>,
    pub situation_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub model: String,
    pub tokens_used: i32,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ReportListParams {
    pub report_type: Option<String>,
    pub situation_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub since: Option<DateTime<Utc>>,
}

/// GET /api/reports
pub async fn list_reports(
    State(state): State<AppState>,
    Query(params): Query<ReportListParams>,
) -> Result<Json<Vec<IntelReport>>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100);
    let since = params
        .since
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));

    let rows = if let Some(ref report_type) = params.report_type {
        sqlx::query_as::<_, IntelReport>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE report_type = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(report_type)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else if let Some(situation_id) = params.situation_id {
        sqlx::query_as::<_, IntelReport>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE situation_id = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(situation_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else if let Some(entity_id) = params.entity_id {
        sqlx::query_as::<_, IntelReport>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE entity_id = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(entity_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, IntelReport>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE generated_at >= $1 \
             ORDER BY generated_at DESC LIMIT $2",
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(rows))
}

/// GET /api/reports/:id
pub async fn get_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
) -> Result<Json<Option<IntelReport>>, ApiError> {
    let row = sqlx::query_as::<_, IntelReport>(
        "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
         model, tokens_used, generated_at \
         FROM intel_reports WHERE id = $1",
    )
    .bind(report_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(row))
}
