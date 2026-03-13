//! Alert rules CRUD and alert history endpoints.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: Uuid,
    pub name: String,
    pub rule_type: String,
    pub conditions: serde_json::Value,
    pub delivery: serde_json::Value,
    pub enabled: bool,
    pub cooldown_minutes: i32,
    pub max_per_hour: i32,
    pub min_severity: String,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAlertRule {
    pub name: String,
    pub rule_type: String,
    pub conditions: serde_json::Value,
    pub delivery: Option<serde_json::Value>,
    pub cooldown_minutes: Option<i32>,
    pub max_per_hour: Option<i32>,
    pub min_severity: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AlertHistoryEntry {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub situation_id: Option<Uuid>,
    pub severity: String,
    pub title: String,
    pub body: Option<String>,
    pub delivered_via: Vec<String>,
    pub fired_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AlertHistoryParams {
    pub limit: Option<i64>,
    pub since: Option<DateTime<Utc>>,
}

/// GET /api/alerts/rules
pub async fn list_rules(State(state): State<AppState>) -> Result<Json<Vec<AlertRule>>, ApiError> {
    let rows = sqlx::query_as::<_, AlertRuleRow>(
        "SELECT id, name, rule_type, conditions, delivery, enabled, \
         EXTRACT(EPOCH FROM cooldown)::int / 60 as cooldown_minutes, \
         max_per_hour, min_severity, last_fired_at, created_at \
         FROM alert_rules ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AlertRule {
                id: r.id,
                name: r.name,
                rule_type: r.rule_type,
                conditions: r.conditions,
                delivery: r.delivery,
                enabled: r.enabled,
                cooldown_minutes: r.cooldown_minutes,
                max_per_hour: r.max_per_hour,
                min_severity: r.min_severity,
                last_fired_at: r.last_fired_at,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

/// POST /api/alerts/rules
pub async fn create_rule(
    State(state): State<AppState>,
    Json(input): Json<CreateAlertRule>,
) -> Result<Json<AlertRule>, ApiError> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let delivery = input
        .delivery
        .unwrap_or_else(|| serde_json::json!(["sse"]));
    let cooldown = input.cooldown_minutes.unwrap_or(30);
    let max_per_hour = input.max_per_hour.unwrap_or(10);
    let min_severity = input
        .min_severity
        .unwrap_or_else(|| "medium".to_string());

    sqlx::query(
        "INSERT INTO alert_rules (id, name, rule_type, conditions, delivery, \
         cooldown, max_per_hour, min_severity, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6::int * INTERVAL '1 minute', $7, $8, $9)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.rule_type)
    .bind(&input.conditions)
    .bind(&delivery)
    .bind(cooldown)
    .bind(max_per_hour)
    .bind(&min_severity)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(AlertRule {
        id,
        name: input.name,
        rule_type: input.rule_type,
        conditions: input.conditions,
        delivery,
        enabled: true,
        cooldown_minutes: cooldown,
        max_per_hour,
        min_severity,
        last_fired_at: None,
        created_at: now,
    }))
}

/// DELETE /api/alerts/rules/:id
pub async fn delete_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("DELETE FROM alert_rules WHERE id = $1")
        .bind(rule_id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({"deleted": true})))
}

/// GET /api/alerts/history
pub async fn get_history(
    State(state): State<AppState>,
    Query(params): Query<AlertHistoryParams>,
) -> Result<Json<Vec<AlertHistoryEntry>>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(200);
    let since = params
        .since
        .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));

    let rows = sqlx::query_as::<_, AlertHistoryRow>(
        "SELECT id, rule_id, situation_id, severity, title, body, delivered_via, fired_at \
         FROM alert_history WHERE fired_at >= $1 \
         ORDER BY fired_at DESC LIMIT $2",
    )
    .bind(since)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AlertHistoryEntry {
                id: r.id,
                rule_id: r.rule_id,
                situation_id: r.situation_id,
                severity: r.severity,
                title: r.title,
                body: r.body,
                delivered_via: r.delivered_via,
                fired_at: r.fired_at,
            })
            .collect(),
    ))
}

// --- Row types ---

#[derive(sqlx::FromRow, Default)]
struct AlertRuleRow {
    id: Uuid,
    name: String,
    rule_type: String,
    conditions: serde_json::Value,
    delivery: serde_json::Value,
    enabled: bool,
    cooldown_minutes: i32,
    max_per_hour: i32,
    min_severity: String,
    last_fired_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Default)]
struct AlertHistoryRow {
    id: Uuid,
    rule_id: Option<Uuid>,
    situation_id: Option<Uuid>,
    severity: String,
    title: String,
    body: Option<String>,
    delivered_via: Vec<String>,
    fired_at: DateTime<Utc>,
}
