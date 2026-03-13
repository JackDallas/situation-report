use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sr_config::PipelineConfig;
use sr_embeddings::{EmbeddingModel, compose_text};
use sr_embeddings::cache::embed_key;
use sr_intel::{
    AnalysisInput, BudgetManager, ClaudeClient, GeminiClient, GapType, OllamaClient, SharedAnalysis,
    SearchRateLimiter, SupplementaryData,
    analyze_current_state, analyze_tiered, analysis_interval_secs, article_from_event,
    build_search_query, enrich_article_tiered,
    generate_narrative_tiered, generate_situation_title,
    search_situation_context, tempo_label,
};
use sr_sources::db::PgPool;
use sr_sources::InsertableEvent;
use sr_types::{EventType, Severity, SourceType};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use crate::airspace::SharedAirspaceIndex;
use crate::alerts::{AlertRule, AlertTracker, FiredAlert, evaluate_rules};
use crate::core::{PipelineCore, NarrativeState};
use crate::rules;
use crate::situation_graph::{SituationCluster, SituationGraph, SituationPhase, is_region_center_fallback};
use crate::types::{Incident, PublishEvent, Summary, SharedEntityResolver, SharedEntityGraph};
use crate::window::CorrelationWindow;

/// Atomic counters for pipeline throughput monitoring and runtime controls.
pub struct PipelineMetrics {
    pub events_ingested: AtomicU64,
    pub events_correlated: AtomicU64,
    pub events_enriched: AtomicU64,
    pub events_published: AtomicU64,
    pub events_filtered: AtomicU64,
    pub incidents_created: AtomicU64,
    /// When true, GPU-intensive work is paused (embeddings, Ollama LLM calls).
    /// Data ingestion, clustering, correlation rules, and SSE publishing continue.
    pub gpu_paused: AtomicBool,
}

impl PipelineMetrics {
    pub fn new() -> Self {
        Self {
            events_ingested: AtomicU64::new(0),
            events_correlated: AtomicU64::new(0),
            events_enriched: AtomicU64::new(0),
            events_published: AtomicU64::new(0),
            events_filtered: AtomicU64::new(0),
            incidents_created: AtomicU64::new(0),
            gpu_paused: AtomicBool::new(false),
        }
    }

    /// Check if GPU processing is currently paused.
    pub fn is_gpu_paused(&self) -> bool {
        self.gpu_paused.load(Ordering::Relaxed)
    }
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary accumulator for a single high-volume event type.
/// These are NOT sent over SSE — they're internal state exposed via REST.
struct SummaryBucket {
    event_type: EventType,
    interval: Duration,
    count: u64,
    entities: HashSet<String>,
    regions: HashSet<String>,
    highlight: Option<InsertableEvent>,
    last_flush: tokio::time::Instant,
}

impl SummaryBucket {
    fn new(event_type: EventType, interval: Duration) -> Self {
        Self {
            event_type,
            interval,
            count: 0,
            entities: HashSet::new(),
            regions: HashSet::new(),
            highlight: None,
            last_flush: tokio::time::Instant::now(),
        }
    }

    fn push(&mut self, event: &InsertableEvent) {
        self.count += 1;
        if let Some(ref entity) = event.entity_id {
            self.entities.insert(entity.clone());
        }
        if let Some(ref region) = event.region_code {
            self.regions.insert(region.clone());
        }
        if self.highlight.is_none() {
            self.highlight = Some(event.clone());
        }
    }

    fn should_flush(&self) -> bool {
        self.count > 0 && self.last_flush.elapsed() >= self.interval
    }

    fn flush(&mut self) -> Summary {
        let summary = Summary {
            event_type: self.event_type,
            window_secs: self.interval.as_secs(),
            count: self.count,
            unique_entities: self.entities.len() as u64,
            regions: self.regions.iter().cloned().collect(),
            highlight: self.highlight.take(),
        };
        self.count = 0;
        self.entities.clear();
        self.regions.clear();
        self.last_flush = tokio::time::Instant::now();
        summary
    }
}

/// Convert a DB `Event` row back to an `InsertableEvent` for pipeline backfill.
/// Returns `None` if the source_type or event_type string doesn't deserialize
/// (shouldn't happen in practice — just a safety guard).
fn db_event_to_insertable(row: &sr_sources::db::models::Event) -> Option<InsertableEvent> {
    let source_type: SourceType =
        serde_json::from_value(serde_json::Value::String(row.source_type.clone())).ok()?;
    let event_type: EventType = row
        .event_type
        .as_ref()
        .and_then(|et| serde_json::from_value(serde_json::Value::String(et.clone())).ok())?;
    let severity = row
        .severity
        .as_ref()
        .map(|s| Severity::from_str_lossy(s))
        .unwrap_or_default();

    Some(InsertableEvent {
        event_time: row.event_time,
        source_type,
        source_id: row.source_id.clone(),
        longitude: row.longitude,
        latitude: row.latitude,
        region_code: row.region_code.clone(),
        entity_id: row.entity_id.clone(),
        entity_name: row.entity_name.clone(),
        event_type,
        severity,
        confidence: row.confidence,
        tags: row.tags.clone().unwrap_or_default(),
        title: row.title.clone(),
        description: row.description.clone(),
        payload: row.payload.clone(),
        heading: None,
        speed: None,
        altitude: None,
    })
}

/// High-volume event types that are silently absorbed into the correlation window
/// but never emitted individually on SSE. They still trigger correlation rules.
///
/// NOTE: bgp_anomaly was removed from this list because the BGP source now
/// pre-filters at ingest time — routine announcements are persisted directly to
/// DB without entering the broadcast channel. Only withdrawals (severity "high")
/// reach the pipeline, and those are low-volume enough to process individually.
const HIGH_VOLUME_TYPES: &[(EventType, u64)] = &[
    (EventType::FlightPosition, 30),
    (EventType::VesselPosition, 30),
    (EventType::CertIssued, 60),
    (EventType::ShodanBanner, 60),
];

/// Event types that pass through the importance filter as individual SSE events.
fn is_important(event: &InsertableEvent) -> bool {
    if event.severity.rank() >= Severity::High.rank() {
        return true;
    }

    match event.event_type {
        // Conflict with fatalities
        EventType::ConflictEvent => event
            .payload
            .get("fatalities")
            .and_then(|v| v.as_f64())
            .is_some_and(|f| f > 0.0),
        // Significant seismic
        EventType::SeismicEvent => event
            .payload
            .get("magnitude")
            .and_then(|v| v.as_f64())
            .is_some_and(|m| m >= 4.0),
        // Always-pass editorial / low-volume types
        EventType::NuclearEvent
        | EventType::GpsInterference
        | EventType::NewsArticle
        | EventType::GeoNews
        | EventType::GeoEvent
        | EventType::ThermalAnomaly
        | EventType::CensorshipEvent
        | EventType::NotamEvent
        | EventType::TelegramMessage
        | EventType::BlueskyPost
        | EventType::ThreatIntel
        | EventType::InternetOutage
        | EventType::FishingEvent
        | EventType::BgpLeak
        | EventType::SourceHealth => true,
        _ => false,
    }
}

/// Returns true for high-volume event types that are "routine" — i.e., not
/// worth clustering in SituationGraph. Anomalous versions (military flights,
/// emergencies, high-value assets) still get through.
fn is_routine_high_volume(event: &InsertableEvent) -> bool {
    match event.event_type {
        EventType::FlightPosition => {
            // Allow military, emergency, and high-value flights through
            let military = event.tags.iter().any(|t| t == "military");
            let high_value = event.tags.iter().any(|t| t == "high_value");
            let emergency = event
                .payload
                .get("emergency")
                .and_then(|v| v.as_str())
                .is_some_and(|e| e != "none" && !e.is_empty());
            let high_sev = event.severity.rank() >= Severity::High.rank();
            // Routine if none of the anomaly signals are present
            !military && !high_value && !emergency && !high_sev
        }
        EventType::VesselPosition => {
            // Allow dark-ship alerts, fishing violations, enforcement through
            let has_alert = event.tags.iter().any(|t| {
                t.contains("dark") || t.contains("violation") || t.contains("enforcement")
            });
            let high_sev = event.severity.rank() >= Severity::High.rank();
            !has_alert && !high_sev
        }
        // bgp_anomaly removed: only withdrawals reach the pipeline now (pre-filtered at source)
        EventType::CertIssued | EventType::ShodanBanner => true,
        _ => false,
    }
}

/// Shared summary state — exposed to REST handlers for dashboard stats.
pub type SharedSummaries = Arc<RwLock<HashMap<EventType, Summary>>>;

/// Result from the embedding worker sent back to the pipeline loop.
struct EmbedResult {
    key: String,
    embedding: Vec<f32>,
}

/// Result from an async Haiku title generation call.
struct TitleResult {
    cluster_id: uuid::Uuid,
    title: String,
}

/// Result from an async Exa web search call.
struct SearchResult {
    cluster_id: uuid::Uuid,
    data: Option<SupplementaryData>,
    gap_type: Option<GapType>,
}

/// Result from an async narrative generation call.
struct NarrativeResult {
    situation_id: uuid::Uuid,
    narrative: sr_intel::SituationNarrative,
}

/// Result from an async summary generation call.
struct SummaryResult {
    situation_id: uuid::Uuid,
    summary_text: String,
    key_entities: serde_json::Value,
    key_dates: serde_json::Value,
}

/// Result from an async Qwen merge audit — contains merges that should be undone.
struct MergeAuditResult {
    parent_id: uuid::Uuid,
    child_id: uuid::Uuid,
}

// NarrativeState is defined in core.rs and re-exported from lib.rs

/// Load alert rules from the database.
async fn load_alert_rules(pool: &PgPool) -> Vec<AlertRule> {
    let rows = sqlx::query_as::<_, AlertRuleDbRow>(
        "SELECT id, name, rule_type, conditions, enabled, \
         EXTRACT(EPOCH FROM cooldown)::int / 60 as cooldown_minutes, \
         max_per_hour, min_severity, last_fired_at \
         FROM alert_rules WHERE enabled = true",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|r| AlertRule {
            id: r.id,
            name: r.name,
            rule_type: r.rule_type,
            conditions: r.conditions,
            enabled: r.enabled,
            cooldown_minutes: r.cooldown_minutes,
            max_per_hour: r.max_per_hour,
            min_severity: Severity::from_str_lossy(&r.min_severity),
            last_fired_at: r.last_fired_at,
        })
        .collect()
}

#[derive(sqlx::FromRow)]
struct AlertRuleDbRow {
    id: uuid::Uuid,
    name: String,
    rule_type: String,
    conditions: serde_json::Value,
    enabled: bool,
    cooldown_minutes: i32,
    max_per_hour: i32,
    min_severity: String,
    last_fired_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(sqlx::FromRow)]
struct NarrativeDbRow {
    situation_id: uuid::Uuid,
    version: i32,
    generated_at: chrono::DateTime<chrono::Utc>,
    narrative_text: String,
}

// ---------------------------------------------------------------------------
// Load persisted clusters from the situations table
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // source_diversity read from DB but computed from source_types on restore
struct SituationDbRow {
    id: uuid::Uuid,
    title: String,
    phase: String,
    phase_changed_at: chrono::DateTime<chrono::Utc>,
    event_count_5m: i32,
    event_count_30m: i32,
    peak_event_rate: f64,
    max_severity: i32,
    source_diversity: i32,
    properties: serde_json::Value,
    started_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Load active situation clusters from the DB, returning fully-reconstructed
/// `SituationCluster` objects that can be fed directly into `SituationGraph::restore_clusters()`.
async fn load_persisted_clusters(pool: &PgPool) -> Vec<SituationCluster> {
    let rows = sqlx::query_as::<_, SituationDbRow>(
        "SELECT id, title, phase::text as phase, phase_changed_at, \
         event_count_5m, event_count_30m, peak_event_rate, \
         max_severity, source_diversity, properties, started_at, updated_at \
         FROM situations \
         WHERE phase::text NOT IN ('resolved', 'historical') \
         AND updated_at > NOW() - INTERVAL '72 hours' \
         ORDER BY updated_at DESC \
         LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.iter().filter_map(cluster_from_db_row).collect()
}

/// Convert a DB row + its properties JSONB back into a `SituationCluster`.
fn cluster_from_db_row(row: &SituationDbRow) -> Option<SituationCluster> {
    let props = &row.properties;

    Some(SituationCluster {
        id: row.id,
        title: row.title.clone(),
        parent_id: props["parent_id"]
            .as_str()
            .and_then(|s| s.parse().ok()),
        entities: json_string_array_to_hashset(&props["entities"]),
        topics: json_string_array_to_hashset(&props["topics"]),
        region_codes: json_string_array_to_hashset(&props["region_codes"]),
        source_types: json_string_array_to_source_types(&props["source_types"]),
        event_ids: json_event_ids(&props["event_ids"]),
        event_titles: json_string_array_to_vec(&props["event_titles"]),
        event_count: props["event_count_total"]
            .as_u64()
            .unwrap_or(row.event_count_5m.max(0) as u64) as usize,
        signal_event_count: props["signal_event_count"]
            .as_u64()
            .unwrap_or(row.event_count_30m.max(0) as u64) as usize,
        severity: severity_from_rank(row.max_severity),
        // coord_buffer and centroid: if a persisted coord_buffer exists, use it
        // to recompute the centroid via median. If no buffer was persisted (old data),
        // discard the old centroid too — it was computed via a corrupted running average.
        centroid: {
            let buf = props["coord_buffer"].as_array();
            match buf {
                Some(arr) => {
                    let coords: Vec<(f64, f64)> = arr.iter()
                        .filter_map(|pair| {
                            let a = pair.as_array()?;
                            if a.len() >= 2 {
                                let lat = a[0].as_f64()?;
                                let lon = a[1].as_f64()?;
                                if is_region_center_fallback(lat, lon) { None } else { Some((lat, lon)) }
                            } else { None }
                        })
                        .collect();
                    if coords.is_empty() {
                        None
                    } else {
                        Some(crate::situation_graph::median_centroid(&coords))
                    }
                }
                // No coord_buffer persisted — old data. Discard corrupted centroid.
                None => None,
            }
        },
        coord_buffer: props["coord_buffer"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|pair| {
                        let a = pair.as_array()?;
                        if a.len() >= 2 {
                            let lat = a[0].as_f64()?;
                            let lon = a[1].as_f64()?;
                            if is_region_center_fallback(lat, lon) {
                                None
                            } else {
                                Some((lat, lon))
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        first_seen: row.started_at,
        last_updated: row.updated_at,
        phase: SituationPhase::from_str_lossy(&row.phase),
        phase_changed_at: row.phase_changed_at,
        phase_transitions: Vec::new(), // Not persisted, starts fresh
        peak_event_rate: row.peak_event_rate,
        peak_rate_at: row.updated_at, // Approximate
        has_ai_title: props["has_ai_title"].as_bool().unwrap_or(false),
        title_signal_count_at_gen: props["title_signal_count_at_gen"]
            .as_u64()
            .unwrap_or(0) as usize,
        last_title_gen: props["last_title_gen"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(row.updated_at),
        search_history: Default::default(), // Loaded separately
        last_searched: None,                // Loaded separately
        supplementary: None,                // Not critical to persist
        certainty: props["certainty"].as_f64().unwrap_or(0.0) as f32,
        anomaly_score: props["anomaly_score"].as_f64().unwrap_or(0.0),
        last_retro_sweep: props["last_retro_sweep"]
            .as_str()
            .and_then(|s| s.parse().ok()),
        total_events_ingested: props["total_events_ingested"]
            .as_u64()
            .unwrap_or(row.event_count_5m.max(0) as u64) as usize,
        direct_event_count: props["direct_event_count"]
            .as_u64()
            .unwrap_or(0) as usize,
        direct_source_types: json_string_array_to_source_types(&props["direct_source_types"]),
    })
}

/// Parse a JSON array of strings into a `HashSet<String>`.
fn json_string_array_to_hashset(v: &serde_json::Value) -> HashSet<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a JSON array of strings into a `Vec<String>`.
fn json_string_array_to_vec(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a JSON array of source type strings back into `HashSet<SourceType>`.
fn json_string_array_to_source_types(v: &serde_json::Value) -> HashSet<SourceType> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let s = item.as_str()?;
                    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a JSON array of `{t, id}` objects back into event ID tuples.
/// Sorts chronologically (oldest first) so `.last()` = most recent event,
/// matching the order used during live operation.
fn json_event_ids(v: &serde_json::Value) -> Vec<(chrono::DateTime<chrono::Utc>, String)> {
    let mut ids: Vec<_> = v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let t = item["t"].as_str()?;
                    let id = item["id"].as_str()?;
                    let dt = chrono::DateTime::parse_from_rfc3339(t).ok()?.to_utc();
                    Some((dt, id.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    ids.sort_by_key(|(dt, _)| *dt);
    ids
}

/// Reverse the severity rank integer back to a `Severity` variant.
fn severity_from_rank(rank: i32) -> Severity {
    match rank {
        0 => Severity::Info,
        1 => Severity::Low,
        2 => Severity::Medium,
        3 => Severity::High,
        4 => Severity::Critical,
        _ => Severity::Low,
    }
}

/// Persist a fired alert to the alert_history table.
async fn persist_alert(pool: &PgPool, alert: &FiredAlert) {
    let result = sqlx::query(
        "INSERT INTO alert_history (id, rule_id, situation_id, severity, title, body, delivered_via, fired_at) \
         VALUES ($1, $2, $3, $4, $5, $6, ARRAY['sse'], $7)",
    )
    .bind(alert.id)
    .bind(alert.rule_id)
    .bind(alert.situation_id)
    .bind(alert.severity.as_str())
    .bind(&alert.title)
    .bind(&alert.body)
    .bind(alert.fired_at)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(alert_id = %alert.id, "Failed to persist alert: {e}");
    }
}

/// Persist a situation cluster to the situations table.
///
/// Writes full cluster state into the `properties` JSONB column so that
/// clusters can be restored across restarts without data loss.
/// Optionally persists the embedding centroid vector for cache warming on restart.
async fn upsert_situation(
    pool: &PgPool,
    cluster: &crate::situation_graph::SituationClusterDTO,
    centroid_embedding: Option<Vec<f32>>,
) {
    let severity_int: i32 = cluster.severity.rank() as i32;

    // Build comprehensive properties JSONB for full cluster restore
    let event_ids_json: Vec<serde_json::Value> = cluster
        .event_ids
        .iter()
        .map(|(dt, id)| serde_json::json!({"t": dt.to_rfc3339(), "id": id}))
        .collect();

    let properties = serde_json::json!({
        "entities": &cluster.entities,
        "topics": &cluster.topics,
        "region_codes": &cluster.region_codes,
        "source_types": &cluster.source_types,
        "event_titles": &cluster.event_titles,
        "parent_id": cluster.parent_id,
        "child_ids": &cluster.child_ids,
        "phase_changed_at": cluster.phase_changed_at.to_rfc3339(),
        "peak_event_rate": cluster.peak_event_rate,
        "signal_event_count": cluster.signal_event_count,
        "has_ai_title": cluster.has_ai_title,
        "title_signal_count_at_gen": cluster.title_signal_count_at_gen,
        "last_title_gen": cluster.last_title_gen.to_rfc3339(),
        "event_count_total": cluster.event_count,
        "event_ids": event_ids_json,
        "centroid": cluster.centroid,
        "coord_buffer": cluster.coord_buffer,
        "certainty": cluster.certainty,
        "total_events_ingested": cluster.total_events_ingested,
        "direct_event_count": cluster.direct_event_count,
        "direct_source_types": &cluster.direct_source_types,
    });

    // Build PostGIS point from centroid if available
    let location_wkt: Option<String> = cluster.centroid.map(|(lat, lon)| {
        format!("SRID=4326;POINT({lon} {lat})")
    });

    // Build pgvector for centroid embedding if available
    let centroid_vec = centroid_embedding.map(pgvector::Vector::from);

    let result = sqlx::query(
        "INSERT INTO situations \
            (id, title, phase, phase_changed_at, event_count_5m, event_count_30m, \
             source_diversity, max_severity, peak_event_rate, current_event_rate, \
             properties, started_at, updated_at, location, centroid_embedding) \
         VALUES ($1, $2, $3::situation_phase, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                 CASE WHEN $14::text IS NOT NULL THEN ST_GeogFromText($14) ELSE NULL END, $15) \
         ON CONFLICT (id) DO UPDATE SET \
            title = EXCLUDED.title, \
            phase = EXCLUDED.phase, \
            phase_changed_at = EXCLUDED.phase_changed_at, \
            event_count_5m = EXCLUDED.event_count_5m, \
            event_count_30m = EXCLUDED.event_count_30m, \
            source_diversity = EXCLUDED.source_diversity, \
            max_severity = EXCLUDED.max_severity, \
            peak_event_rate = EXCLUDED.peak_event_rate, \
            current_event_rate = EXCLUDED.current_event_rate, \
            properties = EXCLUDED.properties, \
            updated_at = EXCLUDED.updated_at, \
            location = EXCLUDED.location, \
            centroid_embedding = COALESCE(EXCLUDED.centroid_embedding, situations.centroid_embedding)",
    )
    .bind(cluster.id)                           // $1
    .bind(&cluster.title)                       // $2
    .bind(cluster.phase.as_str())               // $3
    .bind(cluster.phase_changed_at)             // $4
    .bind(cluster.event_count as i32)           // $5  event_count_5m (total count)
    .bind(cluster.signal_event_count as i32)    // $6  event_count_30m (signal count)
    .bind(cluster.source_count as i32)          // $7  source_diversity
    .bind(severity_int)                         // $8  max_severity
    .bind(cluster.peak_event_rate)              // $9  peak_event_rate
    .bind(0.0_f64)                              // $10 current_event_rate (computed at read time)
    .bind(&properties)                          // $11 properties
    .bind(cluster.first_seen)                   // $12 started_at
    .bind(cluster.last_updated)                 // $13 updated_at
    .bind(&location_wkt)                        // $14 location (EWKT or NULL)
    .bind(centroid_vec)                         // $15 centroid_embedding (vector(1024) or NULL)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(cluster_id = %cluster.id, "Failed to upsert situation: {e}");
    }
}

/// Persist a narrative to the situation_narratives table.
async fn persist_narrative(pool: &PgPool, narrative: &sr_intel::SituationNarrative) {
    let result = sqlx::query(
        "INSERT INTO situation_narratives (id, situation_id, version, narrative_text, model, tokens_used, generated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (situation_id, version) DO NOTHING",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(narrative.situation_id)
    .bind(narrative.version)
    .bind(&narrative.narrative_text)
    .bind(&narrative.model)
    .bind(narrative.tokens_used as i32)
    .bind(narrative.generated_at)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::debug!(situation_id = %narrative.situation_id, version = narrative.version, "Failed to persist narrative: {e}");
    }
}

/// Load recent incidents from DB to seed active_incidents on startup.
/// This ensures cooldowns survive pipeline restarts.
async fn load_recent_incidents(pool: &PgPool) -> Vec<crate::types::Incident> {
    let rows = sqlx::query(
        "SELECT id, rule_id, title, description, severity, confidence, \
         first_seen, last_updated, region_code, tags \
         FROM incidents WHERE first_seen > NOW() - INTERVAL '1 hour' \
         ORDER BY first_seen DESC LIMIT 50",
    )
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let incidents: Vec<_> = rows
                .iter()
                .filter_map(|r| {
                    Some(crate::types::Incident {
                        id: r.get::<uuid::Uuid, _>("id"),
                        rule_id: r.get::<String, _>("rule_id"),
                        title: r.get::<String, _>("title"),
                        description: r.get::<String, _>("description"),
                        severity: match r.get::<i32, _>("severity") {
                            4 => Severity::Critical,
                            3 => Severity::High,
                            2 => Severity::Medium,
                            1 => Severity::Low,
                            _ => Severity::Info,
                        },
                        confidence: r.get::<f32, _>("confidence"),
                        first_seen: r.get("first_seen"),
                        last_updated: r.get("last_updated"),
                        region_code: r.get("region_code"),
                        latitude: None,
                        longitude: None,
                        tags: r.get::<Vec<String>, _>("tags"),
                        evidence: Vec::new(),
                        parent_id: None,
                        related_ids: Vec::new(),
                        merged_from: Vec::new(),
                        display_title: None,
                    })
                })
                .collect();
            if !incidents.is_empty() {
                info!(count = incidents.len(), "Loaded recent incidents from DB for cooldown");
            }
            incidents
        }
        Err(e) => {
            warn!("Failed to load recent incidents: {e}");
            Vec::new()
        }
    }
}

/// Persist a correlated incident to the incidents table.
async fn persist_incident(pool: &PgPool, incident: &crate::types::Incident) {
    let severity_int: i32 = incident.severity.rank() as i32;
    let location_wkt: Option<String> = match (incident.latitude, incident.longitude) {
        (Some(lat), Some(lon)) => Some(format!("SRID=4326;POINT({lon} {lat})")),
        _ => None,
    };
    let evidence_json = serde_json::to_value(&incident.evidence).unwrap_or_default();

    let result = sqlx::query(
        "INSERT INTO incidents (id, rule_id, title, description, severity, confidence, \
         first_seen, last_updated, region_code, location, tags, evidence, parent_id, display_title) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, \
                 CASE WHEN $10::text IS NOT NULL THEN ST_GeogFromText($10) ELSE NULL END, \
                 $11, $12, $13, $14) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(incident.id)
    .bind(&incident.rule_id)
    .bind(&incident.title)
    .bind(&incident.description)
    .bind(severity_int)
    .bind(incident.confidence)
    .bind(incident.first_seen)
    .bind(incident.last_updated)
    .bind(&incident.region_code)
    .bind(&location_wkt)
    .bind(&incident.tags)
    .bind(&evidence_json)
    .bind(incident.parent_id)
    .bind(&incident.display_title)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(incident_id = %incident.id, rule_id = %incident.rule_id, "Failed to persist incident: {e}");
    }
}

/// Spawn the pipeline task. Returns (publish_tx for SSE, shared summaries for REST, shared analysis for REST, metrics).
pub fn spawn_pipeline(
    ingest_tx: broadcast::Sender<InsertableEvent>,
    publish_tx: broadcast::Sender<PublishEvent>,
    claude_client: Option<Arc<ClaudeClient>>,
    ollama_client: Option<Arc<OllamaClient>>,
    gemini_client: Option<Arc<GeminiClient>>,
    budget: Arc<BudgetManager>,
    pool: PgPool,
    embedding_model: Option<Arc<EmbeddingModel>>,
    entity_resolver: SharedEntityResolver,
    entity_graph: SharedEntityGraph,
    config: Arc<PipelineConfig>,
    _airspace_index: SharedAirspaceIndex,
) -> (SharedSummaries, SharedAnalysis, Arc<PipelineMetrics>) {
    let publish_tx_clone = publish_tx.clone();
    let summaries: SharedSummaries = Arc::new(RwLock::new(HashMap::new()));
    let summaries_clone = summaries.clone();
    let analysis: SharedAnalysis = Arc::new(RwLock::new(None));
    let analysis_clone = analysis.clone();
    let metrics = Arc::new(PipelineMetrics::new());
    let metrics_clone = metrics.clone();

    tokio::spawn(async move {
        run_pipeline(
            ingest_tx,
            publish_tx_clone,
            summaries_clone,
            claude_client,
            ollama_client,
            gemini_client,
            budget,
            analysis_clone,
            pool,
            metrics_clone,
            embedding_model,
            entity_resolver,
            entity_graph,
            config,
        )
        .await;
    });

    (summaries, analysis, metrics)
}

#[allow(clippy::too_many_arguments)]
async fn run_pipeline(
    ingest_tx: broadcast::Sender<InsertableEvent>,
    publish_tx: broadcast::Sender<PublishEvent>,
    shared_summaries: SharedSummaries,
    claude_client: Option<Arc<ClaudeClient>>,
    ollama_client: Option<Arc<OllamaClient>>,
    gemini_client: Option<Arc<GeminiClient>>,
    budget: Arc<BudgetManager>,
    shared_analysis: SharedAnalysis,
    pool: PgPool,
    metrics: Arc<PipelineMetrics>,
    embedding_model: Option<Arc<EmbeddingModel>>,
    entity_resolver: SharedEntityResolver,
    entity_graph: SharedEntityGraph,
    config: Arc<PipelineConfig>,
) {
    let mut rx = ingest_tx.subscribe();
    let window_duration = Duration::from_secs(config.intervals.correlation_window_hours * 3600);
    let mut window = CorrelationWindow::new(window_duration);
    let rule_registry = rules::default_rules();
    // Seed active incidents from DB so cooldowns survive restarts
    let mut active_incidents: Vec<Incident> = load_recent_incidents(&pool).await;

    let embeddings_enabled = embedding_model.is_some();
    let mut core = PipelineCore::new(
        config.clone(),
        embeddings_enabled,
        ollama_client.clone(),
        claude_client.clone(),
        gemini_client.clone(),
        Some(Arc::clone(&budget)),
    );
    let mut situation_publish_interval = tokio::time::interval(Duration::from_secs(config.intervals.situation_publish_secs));
    // Consume first tick
    situation_publish_interval.tick().await;

    // Embedding infrastructure (channels + background worker)
    let (embed_tx, mut embed_rx) = mpsc::channel::<(InsertableEvent, String)>(1024);
    let (embed_result_tx, mut embed_result_rx) = mpsc::channel::<EmbedResult>(256);

    if let Some(model) = embedding_model {
        let result_tx = embed_result_tx.clone();
        let embed_pool = pool.clone();
        let embed_metrics = metrics.clone();
        tokio::spawn(async move {
            let mut restarts = 0u32;
            loop {
                embedding_worker(
                    Arc::clone(&model), &mut embed_rx, result_tx.clone(), embed_pool.clone(), &embed_metrics,
                ).await;
                // If embedding_worker returned, the channel was closed — exit.
                // Panics are caught inside via spawn_blocking error handling.
                // Only restart if the channel is still open.
                if embed_rx.is_closed() {
                    break;
                }
                restarts += 1;
                warn!(restarts, "Embedding worker exited unexpectedly — restarting");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    // Drop the sender so embed_result_rx closes when the worker is done
    drop(embed_result_tx);

    // Concurrency limiter for fire-and-forget background tasks (DB writes, enrichment)
    let bg_semaphore = Arc::new(tokio::sync::Semaphore::new(32));

    // AI title generation infrastructure
    let (title_tx, mut title_rx) = mpsc::channel::<TitleResult>(64);

    // Exa web search infrastructure for situation enrichment
    let (search_tx, mut search_rx) = mpsc::channel::<SearchResult>(16);
    let search_rate_limiter = Arc::new(SearchRateLimiter::new());
    let search_http = reqwest::Client::new();
    let mut search_pending: HashSet<uuid::Uuid> = HashSet::new();

    // Channel for re-ingesting enriched events back into the SituationGraph
    let (reingest_tx, mut reingest_rx) = mpsc::channel::<InsertableEvent>(64);

    // Build summary buckets (internal state, not emitted on SSE)
    let high_volume_set: HashMap<EventType, usize> = HIGH_VOLUME_TYPES
        .iter()
        .enumerate()
        .map(|(i, (t, _))| (*t, i))
        .collect();
    let mut buckets: Vec<SummaryBucket> = HIGH_VOLUME_TYPES
        .iter()
        .map(|(t, secs)| SummaryBucket::new(*t, Duration::from_secs(*secs)))
        .collect();

    let mut prune_interval = tokio::time::interval(Duration::from_secs(60));
    let mut summary_interval = tokio::time::interval(Duration::from_secs(5));

    // Tempo tracking for analysis scheduling
    let mut event_count_5min: u64 = 0;
    let mut tempo_window_start = tokio::time::Instant::now();
    let mut last_analysis = tokio::time::Instant::now();

    // Analysis interval (checked every tick, actual run gated by tempo)
    let mut analysis_check = tokio::time::interval(Duration::from_secs(60));
    // Consume the immediate first tick so we don't run analysis on startup
    analysis_check.tick().await;

    // Alert evaluation infrastructure
    let mut alert_tracker = AlertTracker::new();
    let mut alert_rules: Vec<AlertRule> = Vec::new();
    let mut alert_refresh_interval = tokio::time::interval(Duration::from_secs(120));
    alert_refresh_interval.tick().await; // consume first tick

    // Pending re-ingest queue: enriched events waiting for their embedding to be cached
    struct PendingReingest {
        event: InsertableEvent,
        queued_at: tokio::time::Instant,
    }
    let mut pending_reingest: HashMap<String, PendingReingest> = HashMap::new();
    let mut reingest_sweep = tokio::time::interval(Duration::from_secs(10));
    reingest_sweep.tick().await; // consume first tick

    // Retroactive sweep — periodically link old events to current situations
    let retro_interval_secs = core.graph.config.sweep.retro_sweep_interval_secs;
    let mut retro_sweep_interval = tokio::time::interval(Duration::from_secs(retro_interval_secs));
    retro_sweep_interval.tick().await; // consume first tick
    let (retro_tx, mut retro_rx) = mpsc::channel::<(uuid::Uuid, Vec<InsertableEvent>)>(32);

    // Narrative generation infrastructure
    let (narrative_tx, mut narrative_rx) = mpsc::channel::<NarrativeResult>(16);

    // Summary generation infrastructure (situation memory)
    let (summary_tx, mut summary_rx) = mpsc::channel::<SummaryResult>(16);

    // Timeline materialization interval (every 5 minutes)
    let mut timeline_interval = tokio::time::interval(Duration::from_secs(300));
    timeline_interval.tick().await; // consume first tick

    // Merge audit infrastructure — Qwen post-hoc validation of situation merges
    let (merge_audit_tx, mut merge_audit_rx) = mpsc::channel::<MergeAuditResult>(32);

    // Ollama health check watchdog — runs in background task to avoid blocking the pipeline loop.
    // Uses Arc<AtomicU32> for failure counter shared with the spawned task.
    let ollama_failure_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    if let Some(ref ollama) = ollama_client {
        let ollama = ollama.clone();
        let failures = ollama_failure_count.clone();
        let gpu_paused = metrics.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600)); // 10 minutes
            interval.tick().await; // consume first tick
            loop {
                interval.tick().await;
                if gpu_paused.is_gpu_paused() { continue; }
                if ollama.health_check().await {
                    let prev = failures.swap(0, std::sync::atomic::Ordering::Relaxed);
                    if prev > 0 {
                        info!("Ollama health check passed — resetting failure counter");
                    }
                } else {
                    let count = failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    warn!(consecutive_failures = count, "Ollama health check failed");
                    if count >= 3 {
                        info!("Ollama unresponsive after 3 checks — attempting warm_model");
                        match ollama.warm_model().await {
                            Ok(()) => {
                                info!("Ollama model re-warmed successfully");
                                failures.store(0, std::sync::atomic::Ordering::Relaxed);
                            }
                            Err(e) => {
                                warn!("Ollama warm_model failed (Sonnet fallback active): {e}");
                            }
                        }
                    }
                }
            }
        });
    }

    let intel_available = claude_client.is_some();
    info!(
        "Pipeline started: 8 correlation rules, {} high-volume types absorbed, intel={}, embeddings={}",
        HIGH_VOLUME_TYPES.len(),
        intel_available,
        embeddings_enabled,
    );

    // ── Situation restore / historical backfill ─────────────────────────
    // Try to load persisted clusters from the situations table first.
    // If none exist (fresh DB), fall back to replaying recent events.
    {
        let restore_start = tokio::time::Instant::now();
        let persisted = load_persisted_clusters(&pool).await;

        if !persisted.is_empty() {
            let count = persisted.len();
            core.graph.restore_clusters(persisted);

            // Publish restored situations via SSE so any already-connected clients see them
            let clusters = core.active_clusters();
            if !clusters.is_empty() {
                let _ = publish_tx.send(PublishEvent::Situations { clusters });
            }

            // Still backfill the correlation window so rules have context,
            // but skip re-ingesting into the situation graph.
            match sr_sources::db::queries::query_backfill_events(&pool, 6, 10_000).await {
                Ok(rows) => {
                    for row in &rows {
                        if let Some(event) = db_event_to_insertable(row) {
                            window.push(event);
                        }
                    }
                    info!(
                        window_events = rows.len(),
                        "Backfilled correlation window (situations loaded from DB)"
                    );
                }
                Err(e) => {
                    warn!("Correlation window backfill failed (non-fatal): {e}");
                }
            }

            info!(
                situations = count,
                elapsed_ms = restore_start.elapsed().as_millis(),
                "Restored persisted situation clusters from DB"
            );

            // Warm embedding cache with persisted centroids so vector-based
            // merge scoring works immediately (prevents cluster proliferation)
            match sr_embeddings::store::load_all_centroids(&pool).await {
                Ok(centroids) => {
                    let centroid_count = centroids.len();
                    for (id, vec) in centroids {
                        core.embedding_cache.init_centroid(id, &vec);
                    }
                    info!(centroids = centroid_count, "Warmed embedding cache from DB");
                }
                Err(e) => {
                    warn!("Failed to load centroids from DB (non-fatal): {e}");
                }
            }
        } else {
            info!("No persisted clusters found — falling back to event backfill");
            match sr_sources::db::queries::query_backfill_events(&pool, 6, 10_000).await {
                Ok(rows) => {
                    let total = rows.len();
                    let mut fed = 0usize;
                    for row in &rows {
                        if let Some(event) = db_event_to_insertable(row) {
                            // Feed into correlation window (for rule context)
                            window.push(event.clone());

                            // Feed into situation graph for clustering (skip routine high-volume)
                            if !is_routine_high_volume(&event) {
                                core.ingest_event(&event);
                                fed += 1;
                            }
                        }
                    }

                    // Run initial situation cluster so the dashboard gets data immediately
                    let clusters = core.active_clusters();
                    let situation_count = clusters.len();

                    // Persist backfilled situations to DB
                    for cluster in &clusters {
                        let pool = pool.clone();
                        let cluster = cluster.clone();
                        tokio::spawn(async move { upsert_situation(&pool, &cluster, None).await });
                    }

                    // Publish initial situations via SSE so any already-connected clients see them
                    if !clusters.is_empty() {
                        let _ = publish_tx.send(PublishEvent::Situations { clusters });
                    }

                    info!(
                        db_rows = total,
                        fed_to_graph = fed,
                        situations = situation_count,
                        elapsed_ms = restore_start.elapsed().as_millis(),
                        "Historical backfill complete"
                    );
                }
                Err(e) => {
                    warn!("Historical backfill failed (non-fatal): {e}");
                }
            }
        }
    }

    // Load existing narrative state from DB to prevent cascade on restart
    {
        let rows = sqlx::query_as::<_, NarrativeDbRow>(
            "SELECT DISTINCT ON (situation_id) situation_id, version, generated_at, narrative_text \
             FROM situation_narratives ORDER BY situation_id, version DESC"
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        for row in rows {
            // Use current signal_event_count to prevent cascade regeneration on restart.
            let signal_count = core.graph.get_cluster(&row.situation_id)
                .map(|c| c.signal_event_count)
                .unwrap_or(0);
            core.narrative_state.insert(row.situation_id, NarrativeState {
                version: row.version,
                last_generated: Some(row.generated_at),
                signal_count_at_gen: signal_count,
                last_narrative: Some(row.narrative_text),
                previous_summary: None, // Loaded separately below
                event_count_at_summary: 0,
                last_summary_generated: None,
            });
        }
        if !core.narrative_state.is_empty() {
            info!(count = core.narrative_state.len(), "Loaded narrative state from DB — preventing cascade");
        }
    }

    // Load existing situation summaries from DB for long-running situation memory
    {
        let summary_rows = sqlx::query_as::<_, (uuid::Uuid, String, chrono::DateTime<chrono::Utc>)>(
            "SELECT situation_id, summary_text, updated_at FROM situation_summaries"
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        let summary_count = summary_rows.len();
        for (sit_id, summary_text, updated_at) in summary_rows {
            let event_count = core.graph.get_cluster(&sit_id)
                .map(|c| c.event_count)
                .unwrap_or(0);
            if let Some(ns) = core.narrative_state.get_mut(&sit_id) {
                ns.previous_summary = Some(summary_text);
                ns.event_count_at_summary = event_count;
                ns.last_summary_generated = Some(updated_at);
            } else {
                core.narrative_state.insert(sit_id, NarrativeState {
                    version: 0,
                    last_generated: None,
                    signal_count_at_gen: 0,
                    last_narrative: None,
                    previous_summary: Some(summary_text),
                    event_count_at_summary: event_count,
                    last_summary_generated: Some(updated_at),
                });
            }
        }
        if summary_count > 0 {
            info!(count = summary_count, "Loaded situation summaries from DB");
        }
    }

    // Load search history for backfilled clusters to prevent search thrashing on restart
    {
        let search_rows = sqlx::query_as::<_, (uuid::Uuid, String, chrono::DateTime<chrono::Utc>, i32, i32)>(
            "SELECT situation_id, gap_type, last_searched_at, total_searches, empty_searches \
             FROM situation_search_history"
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        let search_loaded = search_rows.len();
        for (sit_id, gap_str, last_ts, total, empty) in search_rows {
            if let Ok(gap) = gap_str.parse::<GapType>() {
                core.graph.restore_search_history(sit_id, gap, last_ts, total as u32, empty as u32);
            }
        }
        if search_loaded > 0 {
            info!(loaded = search_loaded, "Loaded search history from DB");
        }
    }

    // 6h cooldown prevents duplicate incidents for the same ongoing event
    // (e.g., Iran coordinated_shutdown re-firing every sweep).
    let incident_max_age = chrono::Duration::hours(6);

    loop {
        tokio::select! {
            result = rx.recv() => {
                let event = match result {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Pipeline lagged, skipped {n} events");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Ingest channel closed, pipeline shutting down");
                        break;
                    }
                };

                // Track event rate for tempo
                event_count_5min += 1;
                metrics.events_ingested.fetch_add(1, Ordering::Relaxed);

                // 1. Always push into correlation window (even high-volume types)
                window.push(event.clone());

                // 2. Run matching correlation rules — this is the core value
                let matching_rules = rule_registry.rules_for(event.event_type);
                for rule in matching_rules {
                    if let Some(incident) = rule.evaluate(&event, &window, &active_incidents) {
                        info!(
                            rule_id = incident.rule_id,
                            severity = incident.severity.as_str(),
                            title = incident.title,
                            "Incident detected"
                        );
                        let _ = publish_tx.send(PublishEvent::Incident(incident.clone()));
                        {
                            let pool = pool.clone();
                            let inc = incident.clone();
                            tokio::spawn(async move { persist_incident(&pool, &inc).await; });
                        }
                        active_incidents.push(incident);
                        metrics.incidents_created.fetch_add(1, Ordering::Relaxed);
                    }
                }
                metrics.events_correlated.fetch_add(1, Ordering::Relaxed);

                // Feed into situation graph for entity-based clustering
                // Skip routine high-volume events (flight positions, vessel positions,
                // BGP anomalies, certs, Shodan banners) unless they carry anomaly signals.
                // Always ingest immediately — enrichment is best-effort enhancement,
                // not a gate. Unenriched articles still have title/tags for basic
                // clustering. If enrichment succeeds later, the enriched version is
                // reingested via reingest_tx for improved cluster assignment.
                if !is_routine_high_volume(&event) {
                    core.ingest_event(&event);
                }

                // Send to embedding worker (non-blocking, drop if full)
                if embeddings_enabled
                    && let Some(text) = compose_text(&event)
                {
                    if embed_tx.try_send((event.clone(), text)).is_err() {
                        warn!("Embedding queue full — dropping embedding for {}", event.event_type);
                    }
                }

                // 3. High-volume types: accumulate stats internally, don't emit to SSE
                if let Some(&bucket_idx) = high_volume_set.get(&event.event_type) {
                    buckets[bucket_idx].push(&event);
                    metrics.events_filtered.fetch_add(1, Ordering::Relaxed);
                } else if is_important(&event) {
                    // 4. Enrich news articles with Haiku (non-blocking)
                    // 4a. Evaluate alert rules on important events
                    let fired_alerts = evaluate_rules(&alert_rules, &mut alert_tracker, &event);
                    for alert in &fired_alerts {
                        let _ = publish_tx.send(PublishEvent::Alert(alert.clone()));
                        // Fire-and-forget DB persistence
                        let pool = pool.clone();
                        let alert = alert.clone();
                        tokio::spawn(async move { persist_alert(&pool, &alert).await });
                    }

                    if spawn_enrichment(
                        &event,
                        &claude_client,
                        &gemini_client,
                        &ollama_client,
                        &budget,
                        &publish_tx,
                        &pool,
                        &metrics,
                        &reingest_tx,
                        &entity_resolver,
                        &entity_graph,
                        &bg_semaphore,
                    ) {
                        continue; // Don't publish below — the spawn handles it
                    }

                    // 5. Important events pass through to SSE individually
                    let _ = publish_tx.send(PublishEvent::Event { event });
                    metrics.events_published.fetch_add(1, Ordering::Relaxed);
                }
                // Everything else: in the correlation window (for rules) and DB, but silent on SSE
            }
            _ = prune_interval.tick() => {
                let before = window.len();
                window.prune();
                let after = window.len();
                if before != after {
                    debug!("Window pruned: {before} → {after} events");
                }

                // Expire old incidents
                let now = chrono::Utc::now();
                active_incidents.retain(|i| now - i.first_seen < incident_max_age);
            }
            _ = summary_interval.tick() => {
                tick_summaries(&mut buckets, &shared_summaries);
            }
            _ = situation_publish_interval.tick() => {
                tick_situations(
                    &mut core,
                    &ollama_client,
                    &gemini_client,
                    &merge_audit_tx,
                    &claude_client,
                    &budget,
                    &title_tx,
                    &mut search_pending,
                    &search_rate_limiter,
                    &search_http,
                    &search_tx,
                    &narrative_tx,
                    &entity_resolver,
                    &entity_graph,
                    &pool,
                    &bg_semaphore,
                    &publish_tx,
                    &metrics,
                );
            }
            Some(result) = embed_result_rx.recv() => {
                core.embedding_cache.insert(result.key.clone(), result.embedding);
                // Check if an enriched event was waiting for this embedding
                if let Some(pending) = pending_reingest.remove(&result.key) {
                    debug!(key = %result.key, waited_ms = pending.queued_at.elapsed().as_millis(), "Re-ingesting enriched event with embedding");
                    core.ingest_event(&pending.event);
                }
            }
            Some(result) = title_rx.recv() => {
                info!(cluster_id = %result.cluster_id, title = %result.title, "AI-generated situation title");
                core.title_pending.remove(&result.cluster_id);
                core.graph.update_cluster_title(result.cluster_id, result.title);
            }
            Some(result) = search_rx.recv() => {
                search_pending.remove(&result.cluster_id);

                // Record gap search in history + persist to DB
                let is_empty = result.data.is_none();
                if let Some(gap) = result.gap_type {
                    core.graph.record_gap_searched(result.cluster_id, gap);

                    // Persist search history to DB
                    let persist_pool = pool.clone();
                    let persist_sit_id = result.cluster_id;
                    let persist_gap = gap.to_string();
                    tokio::spawn(async move {
                        let _ = sqlx::query(
                            "INSERT INTO situation_search_history (situation_id, gap_type, last_searched_at, total_searches, empty_searches) \
                             VALUES ($1, $2, NOW(), 1, $3) \
                             ON CONFLICT (situation_id, gap_type) DO UPDATE SET \
                             last_searched_at = NOW(), \
                             total_searches = situation_search_history.total_searches + 1, \
                             empty_searches = CASE WHEN $3 > 0 THEN situation_search_history.empty_searches + 1 ELSE situation_search_history.empty_searches END"
                        )
                        .bind(persist_sit_id)
                        .bind(&persist_gap)
                        .bind(if is_empty { 1i32 } else { 0 })
                        .execute(&persist_pool)
                        .await;
                    });
                }

                match result.data {
                    Some(data) => {
                        info!(
                            cluster_id = %result.cluster_id,
                            articles = data.articles.len(),
                            gap_type = ?result.gap_type,
                            "Supplementary search data received"
                        );
                        core.graph.update_cluster_supplementary(result.cluster_id, data);
                    }
                    None => {
                        debug!(
                            cluster_id = %result.cluster_id,
                            gap_type = ?result.gap_type,
                            "Search returned no results"
                        );
                        core.graph.record_empty_search(result.cluster_id);
                    }
                }
            }
            Some(enriched_event) = reingest_rx.recv() => {
                // Re-ingest enriched news articles — now they have entities/topics from Haiku.
                // Wait for the embedding to be cached first so the cosine gate + similarity
                // bonus are applied during clustering (fixes race condition).
                if embeddings_enabled {
                    let key = embed_key(&enriched_event);
                    if core.embedding_cache.get(&key).is_some() {
                        // Embedding already cached — ingest immediately
                        core.ingest_event(&enriched_event);
                    } else {
                        // Queue until embedding arrives
                        debug!(key = %key, "Queuing enriched event pending embedding");
                        pending_reingest.insert(key, PendingReingest {
                            event: enriched_event,
                            queued_at: tokio::time::Instant::now(),
                        });
                    }
                } else {
                    core.ingest_event(&enriched_event);
                }
            }
            _ = analysis_check.tick() => {
                if metrics.is_gpu_paused() { continue; }
                tick_analysis(
                    &mut event_count_5min,
                    &mut tempo_window_start,
                    &mut last_analysis,
                    &core.graph,
                    &window,
                    &claude_client,
                    &ollama_client,
                    &gemini_client,
                    &budget,
                    &shared_analysis,
                    &publish_tx,
                    &pool,
                );
            }
            _ = alert_refresh_interval.tick() => {
                // Refresh alert rules from DB every 2 minutes
                alert_rules = load_alert_rules(&pool).await;
                alert_tracker.cleanup();
                if !alert_rules.is_empty() {
                    debug!(count = alert_rules.len(), "Alert rules refreshed from DB");
                }
            }
            Some(result) = narrative_rx.recv() => {
                info!(
                    situation_id = %result.situation_id,
                    version = result.narrative.version,
                    tokens = result.narrative.tokens_used,
                    "Narrative generated"
                );
                // Update narrative state — snapshot current signal count so delta tracking works
                if let Some(ns) = core.narrative_state.get_mut(&result.situation_id) {
                    ns.version = result.narrative.version;
                    ns.last_generated = Some(result.narrative.generated_at);
                    // Snapshot the cluster's current signal_event_count for delta tracking
                    if let Some(cluster) = core.graph.get_cluster(&result.situation_id) {
                        ns.signal_count_at_gen = cluster.signal_event_count;
                    }
                    ns.last_narrative = Some(result.narrative.narrative_text.clone());
                }
                // Check if cumulative summary needs regeneration
                let event_count = core.graph.get_cluster(&result.situation_id)
                    .map(|c| c.event_count).unwrap_or(0);
                let (ec_at_summary, last_summary_gen) = core.narrative_state.get(&result.situation_id)
                    .map(|ns| (ns.event_count_at_summary, ns.last_summary_generated))
                    .unwrap_or((0, None));
                if sr_intel::should_regenerate_summary(event_count, ec_at_summary, last_summary_gen) {
                    let gemini = gemini_client.clone();
                    let ollama = ollama_client.clone();
                    let budget_clone = budget.clone();
                    let stx = summary_tx.clone();
                    let sid = result.situation_id;
                    let narrative_text = result.narrative.narrative_text.clone();
                    let prev_summary = core.narrative_state.get(&sid)
                        .and_then(|ns| ns.previous_summary.clone());
                    let sit_title = core.graph.get_cluster(&sid)
                        .map(|c| c.title.clone()).unwrap_or_default();
                    let entities: Vec<String> = core.graph.get_cluster(&sid)
                        .map(|c| c.entities.iter().cloned().collect())
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        match sr_intel::generate_summary(
                            gemini.as_deref(), ollama.as_deref(), &budget_clone,
                            &narrative_text, prev_summary.as_deref(), &sit_title,
                            event_count, &entities,
                        ).await {
                            Ok(Some((summary_text, key_entities, key_dates))) => {
                                let _ = stx.send(SummaryResult {
                                    situation_id: sid, summary_text, key_entities, key_dates,
                                }).await;
                            }
                            Ok(None) => { tracing::debug!("Summary generation returned no result"); }
                            Err(e) => { tracing::debug!("Summary generation failed: {e}"); }
                        }
                    });
                }

                // Persist to DB
                let pool = pool.clone();
                let narrative = result.narrative;
                tokio::spawn(async move { persist_narrative(&pool, &narrative).await });
            }
            Some(result) = summary_rx.recv() => {
                info!(
                    situation_id = %result.situation_id,
                    summary_len = result.summary_text.len(),
                    "Cumulative summary generated"
                );
                // Update narrative state with new summary
                if let Some(ns) = core.narrative_state.get_mut(&result.situation_id) {
                    ns.previous_summary = Some(result.summary_text.clone());
                    ns.last_summary_generated = Some(chrono::Utc::now());
                    if let Some(cluster) = core.graph.get_cluster(&result.situation_id) {
                        ns.event_count_at_summary = cluster.event_count;
                    }
                }
                // Persist to DB
                let pool = pool.clone();
                let sid = result.situation_id;
                let summary = result.summary_text;
                let entities = result.key_entities;
                let dates = result.key_dates;
                tokio::spawn(async move {
                    if let Err(e) = sr_sources::db::queries::upsert_situation_summary(
                        &pool, sid, &summary, &entities, &dates,
                    ).await {
                        tracing::warn!(situation_id = %sid, "Failed to persist summary: {e}");
                    }
                });
            }
            _ = timeline_interval.tick() => {
                tick_timeline(&core, &pool, &bg_semaphore);
            }
            Some(result) = merge_audit_rx.recv() => {
                core.graph.unmerge(result.parent_id, result.child_id);
            }
            // Ollama health check moved to background task (see above) — no longer blocks select! loop
            _ = retro_sweep_interval.tick() => {
                // Retroactive sweep: find old events that match current situations.
                // Phase 1 (sync): extract query params from clusters.
                // Phase 2 (async, spawned): query DB, send results back via channel.
                let max_clusters = core.graph.config.sweep.retro_sweep_max_clusters;
                let max_per = core.graph.config.sweep.retro_sweep_max_per_cluster;
                let candidates = core.graph.retro_sweep_candidates(max_clusters);
                for params in candidates {
                    let pool = pool.clone();
                    let tx = retro_tx.clone();
                    let cluster_id = params.cluster_id;
                    tokio::spawn(async move {
                        match query_retro_events(&pool, &params, max_per).await {
                            Ok(events) if !events.is_empty() => {
                                let _ = tx.send((cluster_id, events)).await;
                            }
                            Err(e) => {
                                debug!(
                                    %cluster_id,
                                    error = %e,
                                    "Retroactive sweep query failed"
                                );
                            }
                            _ => {}
                        }
                    });
                }
            }
            Some((cluster_id, events)) = retro_rx.recv() => {
                let count = events.len();
                let added = core.graph.retroactive_add(cluster_id, &events);
                if added > 0 {
                    info!(
                        %cluster_id,
                        queried = count,
                        linked = added,
                        "Retroactive sweep linked historical events"
                    );
                }
            }
            _ = reingest_sweep.tick() => {
                // Flush enriched events that have been waiting too long for their embedding.
                // This is a safety net — normally the embed_result_rx arm handles re-ingest.
                if !pending_reingest.is_empty() {
                    let now = tokio::time::Instant::now();
                    let expired: Vec<String> = pending_reingest.iter()
                        .filter(|(_, p)| now.duration_since(p.queued_at) > Duration::from_secs(10))
                        .map(|(k, _)| k.clone())
                        .collect();
                    for key in expired {
                        if let Some(pending) = pending_reingest.remove(&key) {
                            debug!(key = %key, "Flushing expired pending reingest (no embedding after 10s)");
                            core.ingest_event(&pending.event);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Retroactive sweep — query DB for old events matching situation entities
// ---------------------------------------------------------------------------

/// Query the events table for historical events that match a situation's
/// entities but aren't already linked. Processes oldest-to-newest so temporal
/// context builds naturally. Only returns high-signal event types — text data
/// (titles, descriptions, payloads) is fully preserved in the DB rows.
async fn query_retro_events(
    pool: &PgPool,
    params: &crate::situation_graph::RetroSweepParams,
    max_per_cluster: i64,
) -> anyhow::Result<Vec<InsertableEvent>> {
    let exclude_sids: Vec<String> = params.exclude_source_ids.iter().cloned().collect();

    let rows = sqlx::query_as::<_, sr_sources::db::models::Event>(
        r#"
        SELECT event_time, ingested_at, source_type, source_id,
               ST_Y(location::geometry) as latitude,
               ST_X(location::geometry) as longitude,
               region_code, entity_id, entity_name, event_type,
               severity, confidence, tags, title, description, payload
        FROM events
        WHERE event_time BETWEEN $1 AND $2
          AND (
              entity_name = ANY($3)
              OR title ILIKE ANY($4)
          )
          AND (source_id IS NULL OR NOT source_id = ANY($5))
          AND event_type NOT IN (
              'flight_position', 'vessel_position', 'cert_issued',
              'shodan_banner', 'bgp_anomaly'
          )
        ORDER BY event_time ASC
        LIMIT $6
        "#,
    )
    .bind(params.lookback_from)
    .bind(params.lookback_to)
    .bind(&params.entity_names)
    .bind(&params.entity_patterns)
    .bind(&exclude_sids)
    .bind(max_per_cluster)
    .fetch_all(pool)
    .await?;

    let events: Vec<InsertableEvent> = rows
        .iter()
        .filter_map(db_event_to_insertable)
        .collect();

    Ok(events)
}

// ---------------------------------------------------------------------------
// Extracted sub-tasks — called from the main tokio::select! loop
// ---------------------------------------------------------------------------

/// Flush summary buckets into shared state (for REST endpoint, not SSE).
fn tick_summaries(
    buckets: &mut [SummaryBucket],
    shared_summaries: &SharedSummaries,
) {
    let mut updated = HashMap::new();
    for bucket in buckets.iter_mut() {
        if bucket.should_flush() {
            let summary = bucket.flush();
            debug!(
                event_type = summary.event_type.as_str(),
                count = summary.count,
                "Summary updated"
            );
            updated.insert(summary.event_type, summary);
        }
    }
    if !updated.is_empty()
        && let Ok(mut lock) = shared_summaries.write()
    {
        lock.extend(updated);
    }
}

/// Check tempo and spawn a periodic intelligence analysis if due.
#[allow(clippy::too_many_arguments)]
fn tick_analysis(
    event_count_5min: &mut u64,
    tempo_window_start: &mut tokio::time::Instant,
    last_analysis: &mut tokio::time::Instant,
    situation_graph: &SituationGraph,
    window: &CorrelationWindow,
    claude_client: &Option<Arc<ClaudeClient>>,
    ollama_client: &Option<Arc<OllamaClient>>,
    gemini_client: &Option<Arc<GeminiClient>>,
    budget: &Arc<BudgetManager>,
    shared_analysis: &SharedAnalysis,
    publish_tx: &broadcast::Sender<PublishEvent>,
    pool: &PgPool,
) {
    // Update tempo from rolling event rate
    let elapsed = tempo_window_start.elapsed().as_secs_f64().max(1.0);
    let epm = (*event_count_5min as f64) / (elapsed / 60.0);
    let current_tempo = tempo_label(epm);

    // Reset 5-min window if elapsed
    if tempo_window_start.elapsed() >= Duration::from_secs(300) {
        *event_count_5min = 0;
        *tempo_window_start = tokio::time::Instant::now();
    }

    // Check if it's time for an analysis run
    let interval = analysis_interval_secs(current_tempo);
    let has_analysis_backend = claude_client.is_some() || ollama_client.is_some() || gemini_client.is_some();
    if last_analysis.elapsed() >= Duration::from_secs(interval)
        && has_analysis_backend
    {
        let claude = claude_client.clone();
        let ollama = ollama_client.clone();
        let gemini = gemini_client.clone();
        let budget = Arc::clone(budget);
        let shared = shared_analysis.clone();
        let publish = publish_tx.clone();
        let report_pool = pool.clone();

        // Build analysis input from situation graph clusters
        let situations: Vec<sr_intel::types::SituationSummary> = situation_graph
            .active_clusters()
            .iter()
            .map(|c| sr_intel::types::SituationSummary {
                id: c.id.to_string(),
                title: c.title.clone(),
                severity: c.severity,
                region: c.region_codes.first().cloned(),
                event_count: c.event_count as u32,
                source_types: c.source_types.clone(),
                last_updated: c.last_updated,
                web_context: c.supplementary.as_ref().map(|s| s.context.clone()),
            })
            .collect();

        // Collect recent important events from the window
        let recent_events: Vec<sr_intel::types::EventSummary> = window
            .recent(100)
            .iter()
            .filter(|e| is_important(e))
            .take(50)
            .map(|e| sr_intel::types::EventSummary {
                event_type: e.event_type,
                title: e.title.clone(),
                severity: e.severity,
                region: e.region_code.clone(),
                entity_name: e.entity_name.clone(),
                event_time: e.event_time,
            })
            .collect();

        let tempo = current_tempo.to_string();
        tokio::spawn(async move {
            let input = AnalysisInput {
                situations,
                recent_events,
                tempo,
            };
            match analyze_tiered(claude.as_deref(), gemini.as_deref(), ollama.as_deref(), &budget, &input).await {
                Ok((report, escalate)) => {
                    info!(
                        merges = report.suggested_merges.len(),
                        assessment = %report.escalation_assessment,
                        model = %report.model,
                        escalate,
                        "Intelligence analysis complete"
                    );
                    let _ = publish.send(PublishEvent::Analysis(report.clone()));
                    if let Ok(mut lock) = shared.write() {
                        *lock = Some(report.clone());
                    }
                    let content = serde_json::to_value(&report).unwrap_or_default();
                    let title = format!("Intelligence Brief — {}", report.escalation_assessment);
                    let _ = sqlx::query(
                        "INSERT INTO intel_reports (id, report_type, title, content_json, model, tokens_used, generated_at) \
                         VALUES ($1, 'analysis', $2, $3, $4, $5, NOW())"
                    )
                    .bind(uuid::Uuid::new_v4())
                    .bind(&title)
                    .bind(&content)
                    .bind(&report.model)
                    .bind(report.tokens_used as i32)
                    .execute(&report_pool)
                    .await;

                    if escalate {
                        if let Some(ref client) = claude {
                            info!("Qwen flagged escalation — re-running with Sonnet");
                            match analyze_current_state(client, &budget, &input).await {
                                Ok(sonnet_report) => {
                                    let _ = publish.send(PublishEvent::Analysis(sonnet_report.clone()));
                                    if let Ok(mut lock) = shared.write() {
                                        *lock = Some(sonnet_report.clone());
                                    }
                                    let content = serde_json::to_value(&sonnet_report).unwrap_or_default();
                                    let title = format!("Intelligence Brief (Escalated) — {}", sonnet_report.escalation_assessment);
                                    let _ = sqlx::query(
                                        "INSERT INTO intel_reports (id, report_type, title, content_json, model, tokens_used, generated_at) \
                                         VALUES ($1, 'analysis', $2, $3, $4, $5, NOW())"
                                    )
                                    .bind(uuid::Uuid::new_v4())
                                    .bind(&title)
                                    .bind(&content)
                                    .bind(&sonnet_report.model)
                                    .bind(sonnet_report.tokens_used as i32)
                                    .execute(&report_pool)
                                    .await;
                                }
                                Err(e) => {
                                    warn!("Escalated Sonnet analysis failed: {e:#}");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Analysis failed: {e:#}");
                }
            }
        });

        *last_analysis = tokio::time::Instant::now();
    }
}

/// Materialize timeline buckets for all active situations.
/// Runs every 5 minutes, computes hourly bucket stats, and upserts to DB.
fn tick_timeline(
    core: &PipelineCore,
    pool: &PgPool,
    bg_semaphore: &Arc<tokio::sync::Semaphore>,
) {
    let clusters = core.active_clusters();
    if clusters.is_empty() {
        return;
    }

    let now = chrono::Utc::now();
    // Truncate to the current hour for bucketing
    use chrono::Timelike;
    let bucket = now
        .date_naive()
        .and_hms_opt(now.time().hour(), 0, 0)
        .map(|naive| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc));
    let bucket = match bucket {
        Some(b) => b,
        None => return,
    };

    for cluster in &clusters {
        let pool = pool.clone();
        let sem = bg_semaphore.clone();
        let sit_id = cluster.id;
        let event_count = cluster.event_count as i32;
        let source_count = cluster.source_count as i32;
        let max_severity = cluster.severity.as_str().to_string();
        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            if let Err(e) = sr_sources::db::queries::upsert_timeline_bucket(
                &pool, sit_id, bucket, event_count, source_count, &max_severity,
            ).await {
                tracing::debug!(situation_id = %sit_id, "Failed to upsert timeline bucket: {e}");
            }
        });
    }

    tracing::debug!(situations = clusters.len(), "Timeline buckets materialized");
}

/// Situation graph periodic tick: clustering via PipelineCore, then AI dispatch,
/// search, narrative generation, and publish clusters via SSE.
#[allow(clippy::too_many_arguments)]
fn tick_situations(
    core: &mut PipelineCore,
    ollama_client: &Option<Arc<OllamaClient>>,
    gemini_client: &Option<Arc<GeminiClient>>,
    merge_audit_tx: &mpsc::Sender<MergeAuditResult>,
    claude_client: &Option<Arc<ClaudeClient>>,
    budget: &Arc<BudgetManager>,
    title_tx: &mpsc::Sender<TitleResult>,
    search_pending: &mut HashSet<uuid::Uuid>,
    search_rate_limiter: &Arc<SearchRateLimiter>,
    search_http: &reqwest::Client,
    search_tx: &mpsc::Sender<SearchResult>,
    narrative_tx: &mpsc::Sender<NarrativeResult>,
    entity_resolver: &SharedEntityResolver,
    entity_graph: &SharedEntityGraph,
    pool: &PgPool,
    bg_semaphore: &Arc<tokio::sync::Semaphore>,
    publish_tx: &broadcast::Sender<PublishEvent>,
    metrics: &Arc<PipelineMetrics>,
) {
    // ── Core clustering (shared with replay) ─────────────────────────────
    // Clustering is CPU-only and always runs, even when GPU is paused.
    let output = core.tick_clustering();

    let gpu_paused = metrics.is_gpu_paused();

    // ── Merge audit (production-only, requires Ollama GPU) ───────────────
    if !output.merges.is_empty() && !gpu_paused {
        if let Some(ollama) = ollama_client {
            let mut merge_info: Vec<(uuid::Uuid, uuid::Uuid, String, Vec<String>, String, Vec<String>)> = Vec::new();
            for &(parent_id, child_id, skip_audit) in output.merges.iter().take(10) {
                if skip_audit {
                    debug!(%parent_id, %child_id, "Merge audit: skipped (forced consolidation)");
                    continue;
                }
                let parent = core.graph.get_cluster(&parent_id);
                let child = core.graph.get_cluster(&child_id);
                if let (Some(p), Some(c)) = (parent, child) {
                    merge_info.push((
                        parent_id, child_id,
                        p.title.clone(), p.topics.iter().cloned().collect::<Vec<_>>(),
                        c.title.clone(), c.topics.iter().cloned().collect::<Vec<_>>(),
                    ));
                }
            }
            if !merge_info.is_empty() {
                let ollama = Arc::clone(ollama);
                let tx = merge_audit_tx.clone();
                tokio::spawn(async move {
                    for (parent_id, child_id, parent_title, parent_topics, child_title, child_topics) in merge_info {
                        match ollama.audit_merge(&parent_title, &parent_topics, &child_title, &child_topics).await {
                            Ok(true) => { debug!(%parent_id, %child_id, "Merge audit: confirmed"); }
                            Ok(false) => {
                                info!(%parent_id, %child_id, "Merge audit: rejected, requesting unmerge");
                                let _ = tx.send(MergeAuditResult { parent_id, child_id }).await;
                            }
                            Err(e) => { warn!(%parent_id, %child_id, error = %e, "Merge audit failed, skipping"); }
                        }
                    }
                });
            }
        }
    }

    // ── AI title generation (production-only: fire-and-forget) ───────────
    if !gpu_paused && (claude_client.is_some() || ollama_client.is_some() || gemini_client.is_some()) {
        let needing_titles = core.graph.clusters_needing_titles(&core.title_pending);
        for cluster in needing_titles {
            info!(cluster_id = %cluster.id, event_count = cluster.event_count, current_title = %cluster.title, "Requesting AI title for cluster");
            core.title_pending.insert(cluster.id);
            let claude = claude_client.clone();
            let ollama = ollama_client.clone();
            let gemini = gemini_client.clone();
            let budget = Arc::clone(budget);
            let tx = title_tx.clone();
            let cid = cluster.id;
            let entities: Vec<String> = cluster.entities.iter().cloned().collect();
            let topics: Vec<String> = cluster.topics.iter().cloned().collect();
            let regions: Vec<String> = cluster.region_codes.iter().cloned().collect();
            let event_titles = cluster.event_titles.clone();
            let event_count = cluster.event_count;
            let source_count = cluster.source_types.len();
            let fallback_title = cluster.title.clone();
            tokio::spawn(async move {
                if let Some(title) = generate_situation_title(
                    claude.as_deref(), gemini.as_deref(), ollama.as_deref(), &budget, &entities, &topics, &regions,
                    &event_titles, event_count, source_count, None, None, None, &[],
                ).await {
                    let _ = tx.send(TitleResult { cluster_id: cid, title }).await;
                } else {
                    let _ = tx.send(TitleResult { cluster_id: cid, title: fallback_title }).await;
                }
            });
        }
    }

    // Spawn Exa web search for clusters due for (re-)research
    for analysis in core.graph.clusters_needing_search_with_gaps(search_pending) {
        if !search_rate_limiter.try_acquire() { break; }
        search_pending.insert(analysis.cluster_id);
        let tx = search_tx.clone();
        let cid = analysis.cluster_id;
        let query = analysis.recommended_query.clone();
        let gap = analysis.recommended_gap;
        let http = search_http.clone();
        let limiter = Arc::clone(search_rate_limiter);
        let gap_type_str = format!("{gap:?}");
        let since_str = gap.search_lookback().map(|lookback| (chrono::Utc::now() - lookback).to_rfc3339());
        debug!(cluster_id = %cid, gap_type = ?gap, priority = analysis.priority_score, query = %query, since = ?since_str, "Spawning gap-driven Exa search");
        tokio::spawn(async move {
            let data = search_situation_context(&http, &query, &[], &[], &[], &limiter, Some(gap_type_str.as_str()), since_str.as_deref()).await;
            let _ = tx.send(SearchResult { cluster_id: cid, data, gap_type: Some(gap) }).await;
        });
    }

    // ── Phase transitions + severity escalations ──────────────────────────
    for (cid, transition) in &output.phase_transitions {
        info!(cluster_id = %cid, from = ?transition.from_phase, to = ?transition.to_phase, reason = %transition.trigger_reason, "Situation phase transition");
    }

    for cid in &output.severity_escalations {
        if search_pending.contains(cid) || !search_rate_limiter.try_acquire() { continue; }
        if let Some(cluster) = core.graph.get_cluster(cid) {
            search_pending.insert(*cid);
            let tx = search_tx.clone();
            let cluster_id = *cid;
            let query = build_search_query(
                &cluster.title,
                &cluster.entities.iter().cloned().collect::<Vec<_>>(),
                &cluster.topics.iter().cloned().collect::<Vec<_>>(),
                &cluster.region_codes.iter().cloned().collect::<Vec<_>>(),
            );
            let http = search_http.clone();
            let limiter = Arc::clone(search_rate_limiter);
            let since_str = (chrono::Utc::now() - chrono::Duration::hours(48)).to_rfc3339();
            info!(cluster_id = %cluster_id, severity = %cluster.severity, query = %query, "Proactive search triggered by severity escalation");
            tokio::spawn(async move {
                let data = search_situation_context(&http, &query, &[], &[], &[], &limiter, Some("NewsCoverage"), Some(&since_str)).await;
                let _ = tx.send(SearchResult { cluster_id, data, gap_type: Some(GapType::NewsCoverage) }).await;
            });
        }
    }

    let mut clusters = core.active_clusters();

    for cluster in &mut clusters {
        if let Some(ns) = core.narrative_state.get(&cluster.id) {
            cluster.narrative_text = ns.last_narrative.clone();
        }
    }

    for cluster in &clusters {
        let pool = pool.clone();
        let cluster = cluster.clone();
        let sem = bg_semaphore.clone();
        let centroid = core.embedding_cache.get_cluster_centroid(&cluster.id).cloned();
        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            upsert_situation(&pool, &cluster, centroid).await;
        });
    }

    // Update narrative tracking + spawn regeneration
    let mut narratives_spawned_this_tick = 0;
    const MAX_NARRATIVES_PER_TICK: usize = 3;
    for cluster in &clusters {
        let ns = core.narrative_state.entry(cluster.id).or_insert(NarrativeState {
            version: 0, last_generated: None, signal_count_at_gen: 0, last_narrative: None,
            previous_summary: None, event_count_at_summary: 0, last_summary_generated: None,
        });
        let signal_count = core.graph.get_cluster(&cluster.id).map(|c| c.signal_event_count).unwrap_or(0);
        let events_since = signal_count.saturating_sub(ns.signal_count_at_gen);

        if gpu_paused || narratives_spawned_this_tick >= MAX_NARRATIVES_PER_TICK { continue; }
        if claude_client.is_some() || gemini_client.is_some() || ollama_client.is_some() {
            if sr_intel::should_regenerate(ns.version, ns.last_generated, events_since, false, false) {
                let claude = claude_client.clone();
                let gemini = gemini_client.clone();
                let ollama = ollama_client.clone();
                let budget = Arc::clone(budget);
                let tx = narrative_tx.clone();
                let sid = cluster.id;
                let version = ns.version;
                let prev_narrative = ns.last_narrative.clone();
                let prev_summary = ns.previous_summary.clone();
                let entity_context = build_entity_context(entity_resolver, entity_graph, &cluster.entities);
                let impact_summary = build_impact_summary(entity_resolver, entity_graph, &cluster.entities);

                let context = sr_intel::NarrativeContext {
                    situation_title: cluster.title.clone(), situation_id: sid,
                    phase: cluster.phase.as_str().to_string(), severity: cluster.severity.to_string(),
                    event_count: cluster.event_count,
                    source_types: cluster.source_types.iter().map(|st| st.to_string()).collect(),
                    regions: cluster.region_codes.clone(), entities: cluster.entities.clone(),
                    topics: cluster.topics.clone(), recent_events: Vec::new(),
                    entity_context, previous_narrative: prev_narrative, current_version: version,
                    has_state_change: false, phase_history: vec![],
                    event_rate_trend: "steady".to_string(), hours_since_last_event: 0.0,
                    similar_historical: None, impact_summary,
                    previous_summary: prev_summary,
                };

                tokio::spawn(async move {
                    match generate_narrative_tiered(claude.as_deref(), gemini.as_deref(), ollama.as_deref(), &budget, &context).await {
                        Ok(Some(narrative)) => { let _ = tx.send(NarrativeResult { situation_id: sid, narrative }).await; }
                        Ok(None) => { debug!("Narrative generation skipped (budget)"); }
                        Err(e) => { debug!("Narrative generation failed: {e}"); }
                    }
                });
                narratives_spawned_this_tick += 1;
            }
        }
    }

    debug!(count = clusters.len(), "Publishing situation clusters");
    let _ = publish_tx.send(PublishEvent::Situations { clusters });
}

/// Build entity context string from shared graph for narrative generation.
fn build_entity_context(
    entity_resolver: &SharedEntityResolver,
    entity_graph: &SharedEntityGraph,
    entities: &[String],
) -> Option<String> {
    let mut ctx = String::new();
    if let Ok(resolver) = entity_resolver.read() {
        if let Ok(graph) = entity_graph.read() {
            for entity_name in entities.iter().take(5) {
                if let Some(ent) = resolver.find_by_name(entity_name) {
                    let neighbors = graph.neighbors(&ent.id);
                    if !neighbors.is_empty() {
                        ctx.push_str(&format!("- {} ({}): ", ent.canonical_name, ent.entity_type));
                        for (nid, rel_type, _dir) in neighbors.iter().take(3) {
                            if let Some(neighbor) = resolver.get(nid) {
                                ctx.push_str(&format!("{} {} ", rel_type, neighbor.canonical_name));
                            }
                        }
                        ctx.push('\n');
                    }
                }
            }
        }
    }
    if ctx.is_empty() { None } else { Some(ctx) }
}

/// Build impact summary from entity graph propagation for narrative generation.
fn build_impact_summary(
    entity_resolver: &SharedEntityResolver,
    entity_graph: &SharedEntityGraph,
    entities: &[String],
) -> Option<String> {
    let mut all_assessments = Vec::new();
    let mut seen_entities = std::collections::HashSet::new();
    if let Ok(resolver) = entity_resolver.read() {
        if let Ok(graph) = entity_graph.read() {
            for entity_name in entities.iter().take(3) {
                if let Some(ent) = resolver.find_by_name(entity_name) {
                    let assessments = graph.propagate_impact(ent.id, 2);
                    for a in assessments {
                        if seen_entities.insert(a.affected_entity) {
                            all_assessments.push(a);
                        }
                    }
                }
            }
            if !all_assessments.is_empty() {
                let summary = graph.format_impact_summary(&all_assessments);
                if !summary.is_empty() {
                    return Some(summary);
                }
            }
        }
    }
    None
}

/// Try to spawn an enrichment task for an enrichable event. Returns `true` if
/// a background task was spawned (caller should `continue` to skip normal publish),
/// or `false` if enrichment is not applicable (caller should publish normally).
#[allow(clippy::too_many_arguments)]
fn spawn_enrichment(
    event: &InsertableEvent,
    claude_client: &Option<Arc<ClaudeClient>>,
    gemini_client: &Option<Arc<GeminiClient>>,
    ollama_client: &Option<Arc<OllamaClient>>,
    budget: &Arc<BudgetManager>,
    publish_tx: &broadcast::Sender<PublishEvent>,
    pool: &PgPool,
    metrics: &Arc<PipelineMetrics>,
    reingest_tx: &mpsc::Sender<InsertableEvent>,
    entity_resolver: &SharedEntityResolver,
    entity_graph: &SharedEntityGraph,
    bg_semaphore: &Arc<tokio::sync::Semaphore>,
) -> bool {
    // Skip enrichment when GPU processing is paused
    if metrics.is_gpu_paused() {
        return false;
    }
    let has_enrichment_backend = ollama_client.is_some() || gemini_client.is_some() || claude_client.is_some();
    let is_enrichable = matches!(
        event.event_type,
        EventType::NewsArticle | EventType::TelegramMessage | EventType::GeoNews | EventType::BlueskyPost
    );
    if !is_enrichable || !has_enrichment_backend {
        return false;
    }
    let article_input = match article_from_event(event) {
        Some(a) => a,
        None => return false,
    };

    let claude = claude_client.clone();
    let gemini = gemini_client.clone();
    let ollama = ollama_client.clone();
    let budget = Arc::clone(budget);
    let publish_tx = publish_tx.clone();
    let pool = pool.clone();
    let metrics_spawn = metrics.clone();
    let reingest = reingest_tx.clone();
    let resolver = entity_resolver.clone();
    let graph = entity_graph.clone();
    let mut enrichable_event = event.clone();
    let sem = bg_semaphore.clone();
    tokio::spawn(async move {
        let _permit = sem.acquire().await;
        if let Some(ref sid) = enrichable_event.source_id {
            if let Ok(true) = sr_sources::db::queries::event_has_enrichment(
                &pool, enrichable_event.source_type.as_str(), sid, enrichable_event.event_time,
            ).await {
                return;
            }
        }

        // OCR: extract text from Bluesky post images before enrichment
        let mut article_input = article_input;
        if enrichable_event.event_type == EventType::BlueskyPost {
            if let Some(ocr_text) = ocr_bluesky_images(
                &enrichable_event,
                gemini.as_deref(),
                &budget,
            ).await {
                // Append OCR text to the description for enrichment context
                if !article_input.description.is_empty() {
                    article_input.description.push_str("\n\n[OCR from image] ");
                }
                article_input.description.push_str(&ocr_text);

                // Also update the event description
                let desc = enrichable_event.description.get_or_insert_with(String::new);
                if !desc.is_empty() {
                    desc.push_str("\n\n[OCR from image] ");
                }
                desc.push_str(&ocr_text);

                // Store OCR text in payload
                if let Some(obj) = enrichable_event.payload.as_object_mut() {
                    obj.insert("ocr_text".to_string(), serde_json::Value::String(ocr_text));
                }
            }
        }

        // Tiered enrichment: Ollama -> Gemini Flash-Lite -> Claude
        let enrichment_result = enrich_article_tiered(
            claude.as_deref(),
            gemini.as_deref(),
            ollama.as_deref(),
            &budget,
            &article_input,
        )
        .await;

        match enrichment_result {
            Ok(enriched) => {
                metrics_spawn.events_enriched.fetch_add(1, Ordering::Relaxed);
                if let Ok(enrichment_json) = serde_json::to_value(&enriched) {
                    if let Some(ref sid) = enrichable_event.source_id {
                        let _ = sr_sources::db::queries::update_event_enrichment(
                            &pool, enrichable_event.source_type.as_str(), sid,
                            enrichable_event.event_time, &enrichment_json,
                        ).await;
                    }
                    if let Some(obj) = enrichable_event.payload.as_object_mut() {
                        obj.insert("enrichment".to_string(), enrichment_json);
                    }
                }

                // Geocode upgrade: if event has no location OR is at a generic
                // region centroid, try to pin it to a more specific coordinate
                // using the AI-inferred location or location entities from enrichment.
                let needs_geocode = enrichable_event.latitude.is_none()
                    || enrichable_event.longitude.is_none()
                    || matches!(
                        (enrichable_event.latitude, enrichable_event.longitude),
                        (Some(lat), Some(lon)) if sr_sources::common::is_region_centroid(lat, lon)
                    );
                if needs_geocode {
                    // Prefer the AI's inferred_location (city-level coords from the LLM)
                    let upgraded_coords = enriched.inferred_location.as_ref().map(|loc| {
                        (loc.lat, loc.lon, loc.name.as_str())
                    }).or_else(|| {
                        // Fallback: resolve location entities via geocode lookup
                        // (covers cities, governorates, and countries)
                        enriched.entities.iter()
                            .filter(|e| e.entity_type == "location")
                            .find_map(|e| {
                                sr_sources::common::geocode_entity(&e.name)
                                    .map(|(lat, lon)| (lat, lon, e.name.as_str()))
                            })
                    });
                    if let Some((lat, lon, name)) = upgraded_coords {
                        let was_centroid = enrichable_event.latitude.is_some();
                        enrichable_event.latitude = Some(lat);
                        enrichable_event.longitude = Some(lon);
                        // Update region_code based on new coords
                        let new_region = sr_sources::common::region_from_coords(lat, lon)
                            .map(String::from);
                        if let Some(ref region) = new_region {
                            enrichable_event.region_code = Some(region.clone());
                        }
                        if was_centroid {
                            debug!(
                                name, lat, lon,
                                title = ?enrichable_event.title,
                                "Upgraded region centroid to enrichment-derived location"
                            );
                            // Overwrite the centroid in the DB
                            if let Some(ref sid) = enrichable_event.source_id {
                                let _ = sr_sources::db::queries::update_event_location_upgrade(
                                    &pool, enrichable_event.source_type.as_str(), sid,
                                    enrichable_event.event_time, lat, lon,
                                    new_region.as_deref(),
                                ).await;
                            }
                        } else {
                            debug!(
                                name, lat, lon,
                                title = ?enrichable_event.title,
                                "Applied AI-inferred location"
                            );
                            // Insert location for events that had none
                            if let Some(ref sid) = enrichable_event.source_id {
                                let _ = sr_sources::db::queries::update_event_location(
                                    &pool, enrichable_event.source_type.as_str(), sid,
                                    enrichable_event.event_time, lat, lon,
                                    new_region.as_deref(),
                                ).await;
                            }
                        }
                    }
                }

                if !enriched.state_changes.is_empty() && enriched.relevance_score >= 0.7 {
                    let confirmed_changes: Vec<_> = enriched.state_changes.iter()
                        .filter(|sc| !matches!(sc.certainty.as_str(), "rumored" | "denied")).collect();
                    let has_lethal = confirmed_changes.iter().any(|sc| matches!(sc.to.as_str(), "killed" | "dead" | "destroyed"));
                    let has_injury = confirmed_changes.iter().any(|sc| matches!(sc.to.as_str(), "wounded" | "injured" | "seriously_wounded"));
                    let has_detained = confirmed_changes.iter().any(|sc| matches!(sc.to.as_str(), "arrested" | "detained" | "captured"));
                    let escalated = if has_lethal || has_injury || has_detained { Severity::High }
                        else if !confirmed_changes.is_empty() { Severity::Medium }
                        else { enrichable_event.severity };
                    if escalated > enrichable_event.severity {
                        info!(old = enrichable_event.severity.as_str(), new = escalated.as_str(), title = ?enrichable_event.title, "Escalating severity based on enrichment state changes");
                        enrichable_event.severity = escalated;
                        if let Some(ref sid) = enrichable_event.source_id {
                            let _ = sr_sources::db::queries::update_event_severity(
                                &pool, enrichable_event.source_type.as_str(), sid,
                                enrichable_event.event_time, escalated.as_str(),
                            ).await;
                        }
                    }
                }

                // Process V2 enrichment data through entity graph
                {
                    let entity_pool = pool.clone();
                    let enriched_clone = enriched.clone();
                    let resolver_clone = resolver.clone();
                    let graph_clone = graph.clone();
                    tokio::spawn(async move {
                        let mut resolved_ids = HashMap::new();
                        let mut entities_to_persist = Vec::new();
                        if let Ok(mut resolver) = resolver_clone.write() {
                            for entity in &enriched_clone.entities {
                                let mention = crate::entity_graph::model::EntityMention {
                                    name: entity.name.clone(),
                                    entity_type: Some(entity.entity_type.clone()),
                                    wikidata_qid: entity.wikidata_qid.clone(),
                                    role: entity.role.clone(),
                                };
                                let (id, _created) = resolver.resolve(&mention);
                                resolved_ids.insert(entity.name.clone(), id);
                            }
                            for &eid in resolved_ids.values() {
                                if let Some(entity) = resolver.get(&eid) {
                                    entities_to_persist.push(entity.clone());
                                }
                            }
                        }
                        for entity in &entities_to_persist {
                            if let Err(e) = crate::entity_graph::queries::upsert_entity(&entity_pool, entity).await {
                                tracing::debug!("Entity upsert failed for {}: {e}", entity.canonical_name);
                            }
                        }
                        let mut rels_to_persist = Vec::new();
                        for rel in &enriched_clone.relationships {
                            let source_id = resolved_ids.get(&rel.source);
                            let target_id = resolved_ids.get(&rel.target);
                            if let (Some(&src), Some(&tgt)) = (source_id, target_id) {
                                let rel_type = crate::entity_graph::RelationshipType::from_str(&rel.rel_type)
                                    .unwrap_or(crate::entity_graph::RelationshipType::Alliance);
                                if let Ok(mut graph) = graph_clone.write() {
                                    graph.add_relationship(src, tgt, rel_type.clone());
                                }
                                rels_to_persist.push(crate::entity_graph::EntityRelationship::new(src, tgt, rel_type));
                            }
                        }
                        for db_rel in &rels_to_persist {
                            if let Err(e) = crate::entity_graph::queries::upsert_relationship(&entity_pool, db_rel).await {
                                tracing::debug!("Relationship upsert failed: {e}");
                            }
                        }
                        let state_mentions: Vec<crate::entity_graph::model::StateChangeMention> =
                            enriched_clone.state_changes.iter().map(|sc| {
                                crate::entity_graph::model::StateChangeMention {
                                    entity: sc.entity.clone(), attribute: sc.attribute.clone(),
                                    from: sc.from.clone(), to: sc.to.clone(),
                                    certainty: Some(sc.certainty.clone()),
                                }
                            }).collect();
                        let changes = crate::entity_graph::StateDetector::detect_from_mentions(
                            &state_mentions, &|name| resolved_ids.get(name).copied(),
                        );
                        for change in &changes {
                            if let Err(e) = crate::entity_graph::queries::insert_state_change(&entity_pool, change).await {
                                tracing::debug!("State change insert failed: {e}");
                            }
                        }
                    });
                }

                let _ = reingest.try_send(enrichable_event.clone());
                let _ = publish_tx.send(PublishEvent::Event { event: enrichable_event });
                metrics_spawn.events_published.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                debug!("Enrichment failed (publishing raw): {e}");
                // Event was already ingested unenriched into the graph in the
                // main loop, so no reingest needed here. Just publish to SSE.
                let _ = publish_tx.send(PublishEvent::Event { event: enrichable_event });
                metrics_spawn.events_published.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    true
}

/// Background embedding worker. Receives (event, text) pairs, batches them,
/// runs BGE-M3 inference via spawn_blocking, persists to DB, and sends results
/// back to the pipeline loop.
///
/// When `metrics.gpu_paused` is set, the worker drains the channel but skips
/// inference — events are consumed so the sender doesn't block, but no GPU
/// work is done.
async fn embedding_worker(
    model: Arc<EmbeddingModel>,
    rx: &mut mpsc::Receiver<(InsertableEvent, String)>,
    result_tx: mpsc::Sender<EmbedResult>,
    pool: PgPool,
    metrics: &Arc<PipelineMetrics>,
) {
    const BATCH_SIZE: usize = 8;
    const BATCH_TIMEOUT: Duration = Duration::from_millis(100);

    loop {
        // Collect a batch
        let mut batch: Vec<(InsertableEvent, String)> = Vec::with_capacity(BATCH_SIZE);

        // Wait for first item
        match rx.recv().await {
            Some(item) => batch.push(item),
            None => break, // channel closed
        }

        // Try to fill batch up to BATCH_SIZE with a short timeout
        let deadline = tokio::time::Instant::now() + BATCH_TIMEOUT;
        while batch.len() < BATCH_SIZE {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(item)) => batch.push(item),
                _ => break, // timeout or closed
            }
        }

        // Skip GPU inference when paused — drain the channel to avoid backpressure
        if metrics.is_gpu_paused() {
            continue;
        }

        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let model = Arc::clone(&model);

        // Run inference on blocking thread pool
        let embeddings = match tokio::task::spawn_blocking(move || model.embed(texts)).await {
            Ok(Ok(vecs)) => vecs,
            Ok(Err(e)) => {
                warn!("Embedding batch failed: {e}");
                continue;
            }
            Err(e) => {
                warn!("Embedding task panicked: {e}");
                continue;
            }
        };

        // Process results
        for (i, (event, _text)) in batch.into_iter().enumerate() {
            if let Some(embedding) = embeddings.get(i) {
                let key = embed_key(&event);

                // Persist to DB (non-blocking, fire-and-forget)
                if let Some(ref sid) = event.source_id {
                    let pool = pool.clone();
                    let source_type = event.source_type.to_string();
                    let source_id = sid.clone();
                    let event_time = event.event_time;
                    let emb = embedding.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            sr_embeddings::store_embedding(&pool, &source_type, &source_id, event_time, &emb).await
                        {
                            debug!("Failed to store embedding: {e}");
                        }
                    });
                }

                // Send back to pipeline for cache update
                let _ = result_tx
                    .send(EmbedResult {
                        key,
                        embedding: embedding.clone(),
                    })
                    .await;
            }
        }
    }

    info!("Embedding worker shut down");
}

/// Extract text from Bluesky post images via Gemini Vision OCR.
///
/// Reads `payload.image_urls` (set by the Bluesky source), downloads each image,
/// and sends it to Gemini Flash-Lite for text extraction. Returns the combined
/// OCR text from all images, or `None` if no text was found or OCR is unavailable.
///
/// Constraints: max 3 images per post, 5 MB per image, budget-gated.
async fn ocr_bluesky_images(
    event: &InsertableEvent,
    gemini: Option<&GeminiClient>,
    budget: &BudgetManager,
) -> Option<String> {
    let gemini = gemini?;

    let image_urls = event
        .payload
        .get("image_urls")
        .and_then(|v| v.as_array())?;

    if image_urls.is_empty() {
        return None;
    }

    let mut ocr_parts: Vec<String> = Vec::new();

    for url_val in image_urls.iter().take(3) {
        let url = url_val.as_str().unwrap_or("");
        if url.is_empty() {
            continue;
        }

        // Budget gate before each OCR call
        if !budget.can_afford_gemini().await {
            debug!("Gemini budget exhausted, skipping Bluesky image OCR");
            break;
        }

        match gemini.ocr_image_url(url).await {
            Ok(Some(response)) => {
                budget.record_gemini(sr_intel::GeminiModel::FlashLite, &response);
                let text = response.text.trim().to_string();
                if !text.is_empty() {
                    debug!(
                        url = %url,
                        text_len = text.len(),
                        "Bluesky image OCR extracted text"
                    );
                    ocr_parts.push(text);
                }
            }
            Ok(None) => {
                // No text in image or image too large/unreachable
                debug!(url = %url, "Bluesky image OCR: no text found or image skipped");
            }
            Err(e) => {
                debug!(url = %url, error = %e, "Bluesky image OCR failed");
            }
        }
    }

    if ocr_parts.is_empty() {
        None
    } else {
        Some(ocr_parts.join("\n\n"))
    }
}
