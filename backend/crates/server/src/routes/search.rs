//! Vector search + hybrid search API endpoints.
//! Activates the pgvector HNSW index that was built but never queried.

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// Text query for semantic + lexical search.
    pub q: String,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub radius_km: Option<f64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub source_type: String,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub event_type: Option<String>,
    pub severity: Option<String>,
    pub event_time: DateTime<Utc>,
    pub region_code: Option<String>,
    pub score: f64,
    pub match_type: String, // "semantic", "lexical", "hybrid"
}

#[derive(Debug, Deserialize)]
pub struct SimilarParams {
    /// source_type of the target event
    pub source_type: String,
    /// source_id of the target event
    pub source_id: String,
    /// event_time of the target event
    pub event_time: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SimilarResult {
    pub source_type: String,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub event_type: Option<String>,
    pub event_time: DateTime<Utc>,
    pub distance: f64,
}

/// GET /api/search?q=text&from=&to=&radius_km=&lat=&lon=&limit=
/// Hybrid search: lexical via tsvector (semantic planned via embedding).
pub async fn search_events(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<Vec<SearchResult>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let from = params
        .from
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
    let to = params.to.unwrap_or_else(Utc::now);

    // Spatial filter: if lat/lon/radius_km are all present, filter by geography
    let results = if let (Some(lat), Some(lon), Some(radius_km)) =
        (params.lat, params.lon, params.radius_km)
    {
        let radius_m = radius_km * 1000.0;
        sqlx::query_as::<_, SearchRow>(
            r#"
            SELECT source_type, source_id, title, event_type, severity,
                   event_time, region_code,
                   ts_rank(content_tsv, plainto_tsquery('english', $1)) as score
            FROM events
            WHERE content_tsv @@ plainto_tsquery('english', $1)
              AND event_time >= $2 AND event_time <= $3
              AND location IS NOT NULL
              AND ST_DWithin(location, ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography, $6)
            ORDER BY score DESC
            LIMIT $7
            "#,
        )
        .bind(&params.q)
        .bind(from)
        .bind(to)
        .bind(lon)
        .bind(lat)
        .bind(radius_m)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
    } else {
        // Lexical search via tsvector (no spatial filter)
        sqlx::query_as::<_, SearchRow>(
            r#"
            SELECT source_type, source_id, title, event_type, severity,
                   event_time, region_code,
                   ts_rank(content_tsv, plainto_tsquery('english', $1)) as score
            FROM events
            WHERE content_tsv @@ plainto_tsquery('english', $1)
              AND event_time >= $2 AND event_time <= $3
            ORDER BY score DESC
            LIMIT $4
            "#,
        )
        .bind(&params.q)
        .bind(from)
        .bind(to)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
    };

    Json(
        results
            .into_iter()
            .map(|r| SearchResult {
                source_type: r.source_type,
                source_id: r.source_id,
                title: r.title,
                event_type: r.event_type,
                severity: r.severity,
                event_time: r.event_time,
                region_code: r.region_code,
                score: r.score as f64,
                match_type: "lexical".to_string(),
            })
            .collect(),
    )
}

/// GET /api/search/similar?source_type=&source_id=&event_time=
/// Find events with similar embeddings to the given event.
pub async fn search_similar(
    State(state): State<AppState>,
    Query(params): Query<SimilarParams>,
) -> Json<Vec<SimilarResult>> {
    let results = sqlx::query_as::<_, SimilarRow>(
        r#"
        WITH target AS (
            SELECT embedding, event_time as t_time
            FROM events
            WHERE source_type = $1
              AND source_id = $2
              AND event_time = $3
              AND embedding IS NOT NULL
            LIMIT 1
        )
        SELECT e.source_type, e.source_id, e.title, e.event_type, e.event_time,
               (e.embedding <=> t.embedding) as distance
        FROM events e, target t
        WHERE e.embedding IS NOT NULL
          AND NOT (e.source_type = $1 AND e.source_id = $2 AND e.event_time = $3)
          AND e.event_time > NOW() - INTERVAL '7 days'
        ORDER BY e.embedding <=> t.embedding
        LIMIT 20
        "#,
    )
    .bind(&params.source_type)
    .bind(&params.source_id)
    .bind(params.event_time)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    Json(
        results
            .into_iter()
            .map(|r| SimilarResult {
                source_type: r.source_type,
                source_id: r.source_id,
                title: r.title,
                event_type: r.event_type,
                event_time: r.event_time,
                distance: r.distance,
            })
            .collect(),
    )
}

// --- Row types ---

#[derive(sqlx::FromRow, Default)]
struct SearchRow {
    source_type: String,
    source_id: Option<String>,
    title: Option<String>,
    event_type: Option<String>,
    severity: Option<String>,
    event_time: DateTime<Utc>,
    region_code: Option<String>,
    score: f32,
}

#[derive(sqlx::FromRow)]
struct SimilarRow {
    source_type: String,
    source_id: Option<String>,
    title: Option<String>,
    event_type: Option<String>,
    event_time: DateTime<Utc>,
    distance: f64,
}
