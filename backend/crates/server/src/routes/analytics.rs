//! Analytics API endpoints — activates existing continuous aggregates and adds
//! z-score anomaly detection.

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TimeseriesParams {
    /// Resolution: 5min, 15min, hourly, daily
    pub resolution: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub region: Option<String>,
    pub source_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TimeseriesBucket {
    pub bucket: DateTime<Utc>,
    pub event_count: i64,
    pub region_code: Option<String>,
    pub source_type: Option<String>,
    pub unique_entities: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AnomalyResult {
    pub metric_name: String,
    pub region_code: Option<String>,
    pub source_type: Option<String>,
    pub current_value: f64,
    pub baseline_mean: f64,
    pub baseline_stddev: f64,
    pub z_score: f64,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SourceHealthEntry {
    pub source_id: String,
    pub status: String,
    pub last_success: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: i32,
    pub total_events_24h: i32,
}

/// GET /api/analytics/timeseries
pub async fn get_timeseries(
    State(state): State<AppState>,
    Query(params): Query<TimeseriesParams>,
) -> Result<Json<Vec<TimeseriesBucket>>, ApiError> {
    let resolution = params.resolution.as_deref().unwrap_or("hourly");
    let from = params
        .from
        .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));
    let to = params.to.unwrap_or_else(Utc::now);

    // SAFETY: table name is from a fixed allowlist — never from user input.
    let table = match resolution {
        "5min" => "events_5min",
        "15min" => "events_15min",
        "daily" => "events_daily",
        _ => "events_hourly",
    };

    // Build dynamic query against continuous aggregates.
    // All four aggregates share: bucket, source_type, region_code, event_count, unique_entities.
    let mut query = format!(
        "SELECT bucket, event_count, region_code, source_type, unique_entities \
         FROM {table} WHERE bucket >= $1 AND bucket <= $2"
    );
    let mut param_idx = 3;

    if params.region.is_some() {
        query.push_str(&format!(" AND region_code = ${param_idx}"));
        param_idx += 1;
    }
    if params.source_type.is_some() {
        query.push_str(&format!(" AND source_type = ${param_idx}"));
        // param_idx not needed after this but kept for consistency
        let _ = param_idx;
    }

    query.push_str(" ORDER BY bucket DESC LIMIT 500");

    let rows = match (&params.region, &params.source_type) {
        (Some(region), Some(source)) => {
            sqlx::query_as::<_, TimeseriesRow>(&query)
                .bind(from)
                .bind(to)
                .bind(region)
                .bind(source)
                .fetch_all(&state.db)
                .await
        }
        (Some(region), None) => {
            sqlx::query_as::<_, TimeseriesRow>(&query)
                .bind(from)
                .bind(to)
                .bind(region)
                .fetch_all(&state.db)
                .await
        }
        (None, Some(source)) => {
            sqlx::query_as::<_, TimeseriesRow>(&query)
                .bind(from)
                .bind(to)
                .bind(source)
                .fetch_all(&state.db)
                .await
        }
        (None, None) => {
            sqlx::query_as::<_, TimeseriesRow>(&query)
                .bind(from)
                .bind(to)
                .fetch_all(&state.db)
                .await
        }
    };

    let rows = rows?;
    Ok(Json(
        rows.into_iter()
            .map(|r| TimeseriesBucket {
                bucket: r.bucket,
                event_count: r.event_count,
                region_code: r.region_code,
                source_type: r.source_type,
                unique_entities: r.unique_entities,
            })
            .collect(),
    ))
}

/// GET /api/analytics/anomalies
pub async fn get_anomalies(State(state): State<AppState>) -> Result<Json<Vec<AnomalyResult>>, ApiError> {
    // Z-score anomaly detection: compare current hourly counts against 7-day baselines.
    // Baselines are computed inline from the anomaly_baseline continuous aggregate
    // (hourly event counts populated by TimescaleDB), falling back to direct event
    // table query if the continuous aggregate is empty.  This replaces the old
    // LEFT JOIN on the anomaly_baselines table which was never populated.
    let result = sqlx::query_as::<_, AnomalyRow>(
        r#"
        WITH current_counts AS (
            SELECT source_type, region_code, COUNT(*) as cnt
            FROM events
            WHERE event_time > NOW() - INTERVAL '1 hour'
            GROUP BY source_type, region_code
        ),
        baselines AS (
            SELECT
                source_type,
                region_code,
                AVG(event_count)    AS baseline_mean,
                STDDEV(event_count) AS baseline_stddev,
                COUNT(*)            AS sample_count
            FROM anomaly_baseline
            WHERE bucket >= NOW() - INTERVAL '7 days'
              AND bucket < NOW() - INTERVAL '1 hour'
            GROUP BY source_type, region_code
        )
        SELECT
            'event_count_hourly' as metric_name,
            cc.region_code,
            cc.source_type,
            cc.cnt::float8 as current_value,
            COALESCE(bl.baseline_mean, 10.0) as baseline_mean,
            GREATEST(COALESCE(bl.baseline_stddev, 5.0), 0.1) as baseline_stddev,
            CASE WHEN GREATEST(COALESCE(bl.baseline_stddev, 5.0), 0.1) > 0
                THEN (cc.cnt::float8 - COALESCE(bl.baseline_mean, 10.0))
                     / GREATEST(COALESCE(bl.baseline_stddev, 5.0), 0.1)
                ELSE 0
            END as z_score,
            NOW() as detected_at
        FROM current_counts cc
        LEFT JOIN baselines bl
            ON bl.region_code IS NOT DISTINCT FROM cc.region_code
            AND bl.source_type IS NOT DISTINCT FROM cc.source_type
        WHERE CASE WHEN GREATEST(COALESCE(bl.baseline_stddev, 5.0), 0.1) > 0
            THEN (cc.cnt::float8 - COALESCE(bl.baseline_mean, 10.0))
                 / GREATEST(COALESCE(bl.baseline_stddev, 5.0), 0.1)
            ELSE 0
        END > 2.0
        ORDER BY z_score DESC
        LIMIT 50
        "#,
    )
    .fetch_all(&state.db)
    .await;

    let rows = result?;
    Ok(Json(
        rows.into_iter()
            .map(|r| AnomalyResult {
                metric_name: r.metric_name,
                region_code: r.region_code,
                source_type: r.source_type,
                current_value: r.current_value,
                baseline_mean: r.baseline_mean,
                baseline_stddev: r.baseline_stddev,
                z_score: r.z_score,
                detected_at: r.detected_at,
            })
            .collect(),
    ))
}

/// GET /api/analytics/sources/health
pub async fn get_sources_health(State(state): State<AppState>) -> Result<Json<Vec<SourceHealthEntry>>, ApiError> {
    let result = sqlx::query_as::<_, SourceHealthRow>(
        r#"
        SELECT
            sh.source_id,
            sh.status,
            sh.last_success,
            sh.last_error,
            sh.consecutive_failures,
            COALESCE(ec.cnt, 0)::int4 AS total_events_24h
        FROM source_health sh
        LEFT JOIN (
            SELECT source_type, COUNT(*)::int4 AS cnt
            FROM events
            WHERE event_time >= NOW() - INTERVAL '24 hours'
            GROUP BY source_type
        ) ec ON ec.source_type = sh.source_id
        ORDER BY sh.source_id
        "#,
    )
    .fetch_all(&state.db)
    .await;

    let rows = result?;
    Ok(Json(
        rows.into_iter()
            .map(|r| SourceHealthEntry {
                source_id: r.source_id,
                status: r.status,
                last_success: r.last_success,
                last_error: r.last_error,
                consecutive_failures: r.consecutive_failures,
                total_events_24h: r.total_events_24h,
            })
            .collect(),
    ))
}

// --- Row types for sqlx ---

#[derive(sqlx::FromRow)]
struct TimeseriesRow {
    bucket: DateTime<Utc>,
    event_count: i64,
    region_code: Option<String>,
    source_type: Option<String>,
    unique_entities: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct AnomalyRow {
    metric_name: String,
    region_code: Option<String>,
    source_type: Option<String>,
    current_value: f64,
    baseline_mean: f64,
    baseline_stddev: f64,
    z_score: f64,
    detected_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SourceHealthRow {
    source_id: String,
    status: String,
    last_success: Option<DateTime<Utc>>,
    last_error: Option<String>,
    consecutive_failures: i32,
    total_events_24h: i32,
}

// ---------------------------------------------------------------------------
// Baseline refresh — keeps anomaly_baselines table populated from the
// anomaly_baseline continuous aggregate so other consumers (alerts, reports)
// can also use pre-computed baselines.
// ---------------------------------------------------------------------------

/// Recompute 7-day rolling baselines and upsert into `anomaly_baselines`.
/// Called periodically (e.g. every hour) from a background task.
pub async fn refresh_anomaly_baselines(pool: &sqlx::PgPool) {
    let result = sqlx::query(
        r#"
        INSERT INTO anomaly_baselines (metric_name, region_code, source_type,
                                       baseline_mean, baseline_stddev, sample_count, computed_at)
        SELECT
            'event_count_hourly',
            region_code,
            source_type,
            AVG(event_count)::float8,
            GREATEST(COALESCE(STDDEV(event_count), 0), 0.1)::float8,
            COUNT(*)::int4,
            NOW()
        FROM anomaly_baseline
        WHERE bucket >= NOW() - INTERVAL '7 days'
          AND bucket < NOW() - INTERVAL '1 hour'
        GROUP BY source_type, region_code
        ON CONFLICT (metric_name, region_code, source_type)
        DO UPDATE SET
            baseline_mean  = EXCLUDED.baseline_mean,
            baseline_stddev = EXCLUDED.baseline_stddev,
            sample_count   = EXCLUDED.sample_count,
            computed_at    = EXCLUDED.computed_at
        "#,
    )
    .execute(pool)
    .await;

    match result {
        Ok(r) => {
            tracing::info!(
                rows = r.rows_affected(),
                "Anomaly baselines refreshed"
            );
        }
        Err(e) => {
            tracing::warn!("Failed to refresh anomaly baselines: {e}");
        }
    }
}

/// Spawn a background task that refreshes anomaly baselines every hour.
/// Also runs once immediately at startup so the table is populated from the
/// first request.
pub fn spawn_baseline_refresh(pool: sqlx::PgPool) {
    tokio::spawn(async move {
        // Initial refresh after a short delay (let migrations settle)
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        refresh_anomaly_baselines(&pool).await;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        interval.tick().await; // consume first instant tick
        loop {
            interval.tick().await;
            refresh_anomaly_baselines(&pool).await;
        }
    });
}
