use chrono::{DateTime, Utc};
use sqlx::PgPool;

use serde::{Deserialize, Serialize};

use super::models::{Event, LatestPosition, SourceConfig, SourceHealth};

/// A single point in a position trail, returned by `get_position_trail`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TrailPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub heading: Option<f32>,
    pub speed: Option<f32>,
    pub altitude: Option<f32>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct EventFilter {
    pub source_type: Option<String>,
    pub event_type: Option<String>,
    pub region: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub exclude_types: Option<Vec<String>>,
}

pub async fn query_events(pool: &PgPool, filter: &EventFilter) -> anyhow::Result<Vec<Event>> {
    let limit = filter.limit.unwrap_or(100);
    let offset = filter.offset.unwrap_or(0);

    let events = sqlx::query_as::<_, Event>(
        r#"
        SELECT event_time, ingested_at, source_type, source_id,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               region_code, entity_id, entity_name, event_type,
               severity, confidence, tags, title, description, payload
        FROM events
        WHERE ($1::text IS NULL OR source_type = $1)
          AND ($2::text IS NULL OR event_type = $2)
          AND ($3::text IS NULL OR region_code = $3)
          AND ($4::timestamptz IS NULL OR event_time >= $4)
          AND ($7::text[] IS NULL OR event_type != ALL($7))
        ORDER BY event_time DESC
        LIMIT $5
        OFFSET $6
        "#,
    )
    .bind(&filter.source_type)
    .bind(&filter.event_type)
    .bind(&filter.region)
    .bind(filter.since)
    .bind(limit)
    .bind(offset)
    .bind(&filter.exclude_types)
    .fetch_all(pool)
    .await?;

    Ok(events)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_event(
    pool: &PgPool,
    event_time: DateTime<Utc>,
    source_type: &str,
    source_id: Option<&str>,
    longitude: Option<f64>,
    latitude: Option<f64>,
    region_code: Option<&str>,
    entity_id: Option<&str>,
    entity_name: Option<&str>,
    event_type: Option<&str>,
    severity: Option<&str>,
    confidence: Option<f32>,
    tags: Option<&[String]>,
    title: Option<&str>,
    description: Option<&str>,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO events (
            event_time, source_type, source_id, location,
            region_code, entity_id, entity_name, event_type,
            severity, confidence, tags, title, description, payload
        )
        VALUES (
            $1, $2, $3,
            CASE WHEN $4::double precision IS NOT NULL AND $5::double precision IS NOT NULL
                 THEN ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography
                 ELSE NULL
            END,
            $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
        )
        ON CONFLICT (source_type, source_id, event_time) WHERE source_id IS NOT NULL
        DO NOTHING
        "#,
    )
    .bind(event_time)
    .bind(source_type)
    .bind(source_id)
    .bind(longitude)
    .bind(latitude)
    .bind(region_code)
    .bind(entity_id)
    .bind(entity_name)
    .bind(event_type)
    .bind(severity)
    .bind(confidence)
    .bind(tags)
    .bind(title)
    .bind(description)
    .bind(payload)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_latest_position(
    pool: &PgPool,
    entity_id: &str,
    source_type: &str,
    entity_name: Option<&str>,
    longitude: f64,
    latitude: f64,
    heading: Option<f32>,
    speed: Option<f32>,
    altitude: Option<f32>,
    last_seen: DateTime<Utc>,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO latest_positions (
            entity_id, source_type, entity_name, location,
            heading, speed, altitude, last_seen, payload
        )
        VALUES (
            $1, $2, $3,
            ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography,
            $6, $7, $8, $9, $10
        )
        ON CONFLICT (entity_id) DO UPDATE SET
            location = EXCLUDED.location,
            heading = EXCLUDED.heading,
            speed = EXCLUDED.speed,
            altitude = EXCLUDED.altitude,
            last_seen = EXCLUDED.last_seen,
            entity_name = COALESCE(EXCLUDED.entity_name, latest_positions.entity_name),
            payload = latest_positions.payload || EXCLUDED.payload
        "#,
    )
    .bind(entity_id)
    .bind(source_type)
    .bind(entity_name)
    .bind(longitude)
    .bind(latitude)
    .bind(heading)
    .bind(speed)
    .bind(altitude)
    .bind(last_seen)
    .bind(payload)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_events_geojson(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    limit: i64,
    include_types: Option<&[String]>,
    exclude_types: Option<&[String]>,
    bbox: Option<(f64, f64, f64, f64)>, // (west, south, east, north)
    severity_filter: Option<&[String]>,
) -> anyhow::Result<serde_json::Value> {
    let events = sqlx::query_as::<_, Event>(
        r#"
        SELECT event_time, ingested_at, source_type, source_id,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               region_code, entity_id, entity_name, event_type,
               severity, confidence, tags, title, description, payload
        FROM events
        WHERE location IS NOT NULL
          AND ($1::timestamptz IS NULL OR event_time >= $1)
          AND ($3::text[] IS NULL OR event_type = ANY($3))
          AND ($4::text[] IS NULL OR event_type != ALL($4))
          AND (
              $5::double precision IS NULL
              OR ST_Within(
                  location::geometry,
                  ST_MakeEnvelope($5, $6, $7, $8, 4326)
              )
          )
          AND ($9::text[] IS NULL OR severity = ANY($9))
        ORDER BY event_time DESC
        LIMIT $2
        "#,
    )
    .bind(since)
    .bind(limit)
    .bind(include_types)
    .bind(exclude_types)
    .bind(bbox.map(|b| b.0)) // west
    .bind(bbox.map(|b| b.1)) // south
    .bind(bbox.map(|b| b.2)) // east
    .bind(bbox.map(|b| b.3)) // north
    .bind(severity_filter)
    .fetch_all(pool)
    .await?;

    let features: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [e.longitude, e.latitude]
                },
                "properties": {
                    "source_type": e.source_type,
                    "source_id": e.source_id,
                    "event_type": e.event_type,
                    "event_time": e.event_time,
                    "entity_id": e.entity_id,
                    "entity_name": e.entity_name,
                    "severity": e.severity,
                    "confidence": e.confidence,
                    "title": e.title,
                    "region_code": e.region_code,
                    "payload": e.payload
                }
            })
        })
        .collect();

    Ok(serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    }))
}

pub async fn query_latest_positions(
    pool: &PgPool,
    source_type: Option<&str>,
    bbox: Option<(f64, f64, f64, f64)>, // (west, south, east, north)
    since: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<LatestPosition>> {
    let positions = sqlx::query_as::<_, LatestPosition>(
        r#"
        SELECT entity_id, source_type, entity_name,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               heading, speed, altitude, last_seen, payload
        FROM latest_positions
        WHERE ($1::text IS NULL OR source_type = $1)
          AND ($2::timestamptz IS NULL OR last_seen >= $2)
          AND (
              $3::double precision IS NULL
              OR ST_Within(
                  location::geometry,
                  ST_MakeEnvelope($3, $4, $5, $6, 4326)
              )
          )
        ORDER BY last_seen DESC
        "#,
    )
    .bind(source_type)
    .bind(since)
    .bind(bbox.map(|b| b.0)) // west
    .bind(bbox.map(|b| b.1)) // south
    .bind(bbox.map(|b| b.2)) // east
    .bind(bbox.map(|b| b.3)) // north
    .fetch_all(pool)
    .await?;

    Ok(positions)
}

// =========================================================================
// Source config and health queries (unchanged)
// =========================================================================

pub async fn get_all_source_configs(pool: &PgPool) -> anyhow::Result<Vec<SourceConfig>> {
    let configs = sqlx::query_as::<_, SourceConfig>(
        "SELECT source_id, enabled, poll_interval_secs, api_key_encrypted, extra_config, updated_at FROM source_config ORDER BY source_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(configs)
}

pub async fn get_source_config(pool: &PgPool, source_id: &str) -> anyhow::Result<Option<SourceConfig>> {
    let config = sqlx::query_as::<_, SourceConfig>(
        "SELECT source_id, enabled, poll_interval_secs, api_key_encrypted, extra_config, updated_at FROM source_config WHERE source_id = $1",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;
    Ok(config)
}

pub async fn upsert_source_config(
    pool: &PgPool,
    source_id: &str,
    enabled: bool,
    poll_interval_secs: Option<i32>,
    extra_config: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO source_config (source_id, enabled, poll_interval_secs, extra_config, updated_at)
        VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (source_id) DO UPDATE SET
            enabled = EXCLUDED.enabled,
            poll_interval_secs = EXCLUDED.poll_interval_secs,
            extra_config = EXCLUDED.extra_config,
            updated_at = NOW()
        "#,
    )
    .bind(source_id)
    .bind(enabled)
    .bind(poll_interval_secs)
    .bind(extra_config)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_all_source_health(pool: &PgPool) -> anyhow::Result<Vec<SourceHealth>> {
    let health = sqlx::query_as::<_, SourceHealth>(
        "SELECT source_id, last_success, last_failure, last_error, consecutive_failures, total_events_24h, status FROM source_health ORDER BY source_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(health)
}

pub async fn get_source_health(pool: &PgPool, source_id: &str) -> anyhow::Result<Option<SourceHealth>> {
    let health = sqlx::query_as::<_, SourceHealth>(
        "SELECT source_id, last_success, last_failure, last_error, consecutive_failures, total_events_24h, status FROM source_health WHERE source_id = $1",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;
    Ok(health)
}

pub async fn update_source_health(
    pool: &PgPool,
    source_id: &str,
    status: &str,
    last_error: Option<&str>,
) -> anyhow::Result<()> {
    let is_healthy = status == "healthy";

    sqlx::query(
        r#"
        INSERT INTO source_health (source_id, status, last_error, last_success, last_failure, consecutive_failures)
        VALUES ($1, $2, $3,
            CASE WHEN $4 THEN NOW() ELSE NULL END,
            CASE WHEN $4 THEN NULL ELSE NOW() END,
            CASE WHEN $4 THEN 0 ELSE 1 END)
        ON CONFLICT (source_id) DO UPDATE SET
            status = EXCLUDED.status,
            last_error = EXCLUDED.last_error,
            last_success = CASE WHEN $4 THEN NOW() ELSE source_health.last_success END,
            last_failure = CASE WHEN $4 THEN source_health.last_failure ELSE NOW() END,
            consecutive_failures = CASE
                WHEN $4 THEN 0
                ELSE COALESCE(source_health.consecutive_failures, 0) + 1
            END
        "#,
    )
    .bind(source_id)
    .bind(status)
    .bind(last_error)
    .bind(is_healthy)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_event_stats(pool: &PgPool) -> anyhow::Result<serde_json::Value> {
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events")
        .fetch_one(pool)
        .await?;

    let last_24h = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM events WHERE event_time >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(pool)
    .await?;

    Ok(serde_json::json!({
        "total_events": total,
        "events_24h": last_24h,
    }))
}

// =========================================================================
// Budget tracking queries
// =========================================================================

/// Token counters for today, loaded from DB on startup.
#[derive(Debug, Default, sqlx::FromRow)]
pub struct DailyBudgetRow {
    pub haiku_input_tokens: i64,
    pub haiku_output_tokens: i64,
    pub haiku_cache_read_tokens: i64,
    pub sonnet_input_tokens: i64,
    pub sonnet_output_tokens: i64,
    pub sonnet_cache_read_tokens: i64,
}

/// Load today's budget counters from DB. Returns defaults if no row exists yet.
pub async fn load_today_budget(pool: &PgPool) -> anyhow::Result<DailyBudgetRow> {
    let row = sqlx::query_as::<_, DailyBudgetRow>(
        r#"
        SELECT haiku_input_tokens, haiku_output_tokens, haiku_cache_read_tokens,
               sonnet_input_tokens, sonnet_output_tokens, sonnet_cache_read_tokens
        FROM budget_daily
        WHERE day = CURRENT_DATE
        "#
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or_default())
}

/// Atomically increment token counters for today (upsert).
pub async fn record_budget_tokens(
    pool: &PgPool,
    haiku_input: i64,
    haiku_output: i64,
    haiku_cache_read: i64,
    sonnet_input: i64,
    sonnet_output: i64,
    sonnet_cache_read: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO budget_daily (day, haiku_input_tokens, haiku_output_tokens, haiku_cache_read_tokens,
                                  sonnet_input_tokens, sonnet_output_tokens, sonnet_cache_read_tokens)
        VALUES (CURRENT_DATE, $1, $2, $3, $4, $5, $6)
        ON CONFLICT (day) DO UPDATE SET
            haiku_input_tokens = budget_daily.haiku_input_tokens + EXCLUDED.haiku_input_tokens,
            haiku_output_tokens = budget_daily.haiku_output_tokens + EXCLUDED.haiku_output_tokens,
            haiku_cache_read_tokens = budget_daily.haiku_cache_read_tokens + EXCLUDED.haiku_cache_read_tokens,
            sonnet_input_tokens = budget_daily.sonnet_input_tokens + EXCLUDED.sonnet_input_tokens,
            sonnet_output_tokens = budget_daily.sonnet_output_tokens + EXCLUDED.sonnet_output_tokens,
            sonnet_cache_read_tokens = budget_daily.sonnet_cache_read_tokens + EXCLUDED.sonnet_cache_read_tokens,
            updated_at = NOW()
        "#,
    )
    .bind(haiku_input)
    .bind(haiku_output)
    .bind(haiku_cache_read)
    .bind(sonnet_input)
    .bind(sonnet_output)
    .bind(sonnet_cache_read)
    .execute(pool)
    .await?;
    Ok(())
}

/// Check if an event already has enrichment data in its payload.
/// Used to skip re-enrichment after container restarts.
pub async fn event_has_enrichment(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT payload ? 'enrichment'
        FROM events
        WHERE source_type = $1 AND source_id = $2 AND event_time = $3
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .fetch_optional(pool)
    .await?;
    Ok(exists.unwrap_or(false))
}

/// Merge enrichment JSON into an event's payload (jsonb || jsonb).
/// Uses source_type + source_id + event_time as the composite key.
pub async fn update_event_enrichment(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
    enrichment: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE events
        SET payload = payload || jsonb_build_object('enrichment', $4::jsonb)
        WHERE source_type = $1
          AND source_id = $2
          AND event_time = $3
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .bind(enrichment)
    .execute(pool)
    .await?;

    Ok(())
}

/// Update the severity of an event after enrichment-based escalation.
/// Uses source_type + source_id + event_time as the composite key.
pub async fn update_event_severity(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
    severity: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE events
        SET severity = $4
        WHERE source_type = $1
          AND source_id = $2
          AND event_time = $3
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .bind(severity)
    .execute(pool)
    .await?;

    Ok(())
}

/// Persist AI-inferred location and region_code back to an event after enrichment.
/// Only updates rows that still have NULL location (avoids overwriting real coordinates).
pub async fn update_event_location(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
    lat: f64,
    lon: f64,
    region_code: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE events
        SET location = ST_SetSRID(ST_MakePoint($4, $5), 4326),
            region_code = COALESCE($6, region_code)
        WHERE source_type = $1 AND source_id = $2 AND event_time = $3
          AND location IS NULL
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .bind(lon)
    .bind(lat)
    .bind(region_code)
    .execute(pool)
    .await?;

    Ok(())
}

/// Upgrade an event's location from a region centroid to a more precise coordinate.
/// Unlike `update_event_location`, this overwrites existing (non-NULL) locations.
/// Used after enrichment geocodes an RSS article to a specific city/country.
pub async fn update_event_location_upgrade(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
    lat: f64,
    lon: f64,
    region_code: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE events
        SET location = ST_SetSRID(ST_MakePoint($4, $5), 4326),
            region_code = COALESCE($6, region_code)
        WHERE source_type = $1 AND source_id = $2 AND event_time = $3
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .bind(lon)
    .bind(lat)
    .bind(region_code)
    .execute(pool)
    .await?;

    Ok(())
}

// =========================================================================
// Position history queries
// =========================================================================

/// Insert a position into position_history (append-only, no upsert).
#[allow(clippy::too_many_arguments)]
pub async fn insert_position_history(
    pool: &PgPool,
    entity_id: &str,
    source_type: &str,
    latitude: f64,
    longitude: f64,
    heading: Option<f32>,
    speed: Option<f32>,
    altitude: Option<f32>,
    recorded_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO position_history (
            entity_id, source_type, latitude, longitude,
            heading, speed, altitude, recorded_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(entity_id)
    .bind(source_type)
    .bind(latitude)
    .bind(longitude)
    .bind(heading)
    .bind(speed)
    .bind(altitude)
    .bind(recorded_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get position trail for an entity (most recent first, limited).
pub async fn get_position_trail(
    pool: &PgPool,
    entity_id: &str,
    hours: f64,
    limit: i64,
) -> Result<Vec<TrailPoint>, sqlx::Error> {
    let points = sqlx::query_as::<_, TrailPoint>(
        r#"
        SELECT latitude, longitude, heading, speed, altitude, recorded_at
        FROM position_history
        WHERE entity_id = $1
          AND recorded_at > NOW() - make_interval(secs => $2)
        ORDER BY recorded_at DESC
        LIMIT $3
        "#,
    )
    .bind(entity_id)
    .bind(hours * 3600.0) // convert hours to seconds for make_interval
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(points)
}

/// Load recent "important" events from the last N hours for pipeline backfill.
/// Returns events ordered by event_time ASC so they can be replayed in order.
/// Only selects event types that pass the pipeline importance filter — skips
/// high-volume position/banner/cert types that would just be noise.
pub async fn query_backfill_events(
    pool: &PgPool,
    hours: i32,
    limit: i64,
) -> anyhow::Result<Vec<Event>> {
    let events = sqlx::query_as::<_, Event>(
        r#"
        SELECT event_time, ingested_at, source_type, source_id,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               region_code, entity_id, entity_name, event_type,
               severity, confidence, tags, title, description, payload
        FROM events
        WHERE event_time > NOW() - make_interval(hours => $1)
          AND (
              severity IN ('high', 'critical')
              OR event_type IN (
                  'conflict_event', 'news_article', 'geo_news',
                  'geoconfirmed', 'nuclear_event',
                  'gps_interference', 'seismic_event',
                  'internet_outage', 'censorship_event', 'notam_event',
                  'telegram_message', 'threat_intel', 'fishing_event',
                  'bgp_leak', 'geo_event'
              )
          )
        ORDER BY event_time ASC
        LIMIT $2
        "#,
    )
    .bind(hours)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(events)
}
