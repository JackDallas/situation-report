//! Intelligence report API endpoints.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Serialize)]
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
) -> Json<Vec<IntelReport>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let since = params
        .since
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));

    let rows = if let Some(ref report_type) = params.report_type {
        sqlx::query_as::<_, ReportRow>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE report_type = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(report_type)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else if let Some(situation_id) = params.situation_id {
        sqlx::query_as::<_, ReportRow>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE situation_id = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(situation_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else if let Some(entity_id) = params.entity_id {
        sqlx::query_as::<_, ReportRow>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE entity_id = $1 AND generated_at >= $2 \
             ORDER BY generated_at DESC LIMIT $3",
        )
        .bind(entity_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as::<_, ReportRow>(
            "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
             model, tokens_used, generated_at \
             FROM intel_reports WHERE generated_at >= $1 \
             ORDER BY generated_at DESC LIMIT $2",
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    };

    match rows {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|r| IntelReport {
                    id: r.id,
                    report_type: r.report_type,
                    title: r.title,
                    content_json: r.content_json,
                    content_html: r.content_html,
                    situation_id: r.situation_id,
                    entity_id: r.entity_id,
                    model: r.model,
                    tokens_used: r.tokens_used,
                    generated_at: r.generated_at,
                })
                .collect(),
        ),
        Err(e) => {
            tracing::error!("Report query failed: {e}");
            Json(Vec::new())
        }
    }
}

/// GET /api/reports/:id
pub async fn get_report(
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
) -> Json<Option<IntelReport>> {
    let row = sqlx::query_as::<_, ReportRow>(
        "SELECT id, report_type, title, content_json, content_html, situation_id, entity_id, \
         model, tokens_used, generated_at \
         FROM intel_reports WHERE id = $1",
    )
    .bind(report_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    Json(row.map(|r| IntelReport {
        id: r.id,
        report_type: r.report_type,
        title: r.title,
        content_json: r.content_json,
        content_html: r.content_html,
        situation_id: r.situation_id,
        entity_id: r.entity_id,
        model: r.model,
        tokens_used: r.tokens_used,
        generated_at: r.generated_at,
    }))
}

// --- Row types ---

#[derive(sqlx::FromRow)]
struct ReportRow {
    id: Uuid,
    report_type: String,
    title: String,
    content_json: serde_json::Value,
    content_html: Option<String>,
    situation_id: Option<Uuid>,
    entity_id: Option<Uuid>,
    model: String,
    tokens_used: i32,
    generated_at: DateTime<Utc>,
}
