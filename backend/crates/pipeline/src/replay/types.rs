//! Types for event replay datasets and result snapshots.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_sources::db::models::Event;
use sr_sources::InsertableEvent;
use sr_types::{EventType, Severity, SourceType};

use crate::situation_graph::SituationClusterDTO;

/// A single event in a replay dataset. Preserves all raw fields from the DB
/// plus the original ingestion timestamp for ordering fidelity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvent {
    pub event_time: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub source_type: String,
    pub source_id: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub region_code: Option<String>,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub event_type: Option<String>,
    pub severity: Option<String>,
    pub confidence: Option<f32>,
    pub tags: Option<Vec<String>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub payload: serde_json::Value,
}

impl From<Event> for ReplayEvent {
    fn from(e: Event) -> Self {
        Self {
            event_time: e.event_time,
            ingested_at: e.ingested_at,
            source_type: e.source_type,
            source_id: e.source_id,
            latitude: e.latitude,
            longitude: e.longitude,
            region_code: e.region_code,
            entity_id: e.entity_id,
            entity_name: e.entity_name,
            event_type: e.event_type,
            severity: e.severity,
            confidence: e.confidence,
            tags: e.tags,
            title: e.title,
            description: e.description,
            payload: e.payload,
        }
    }
}

impl ReplayEvent {
    /// Convert to `InsertableEvent` for feeding into `SituationGraph::ingest()`.
    /// Returns `None` if source_type or event_type can't be deserialized
    /// (same safety guard as the production backfill path).
    pub fn to_insertable(&self) -> Option<InsertableEvent> {
        let source_type: SourceType = serde_json::from_value(
            serde_json::Value::String(self.source_type.clone()),
        )
        .ok()?;
        let event_type: EventType = self
            .event_type
            .as_ref()
            .and_then(|et| {
                serde_json::from_value(serde_json::Value::String(et.clone())).ok()
            })?;
        let severity = self
            .severity
            .as_ref()
            .map(|s| Severity::from_str_lossy(s))
            .unwrap_or_default();

        Some(InsertableEvent {
            event_time: self.event_time,
            source_type,
            source_id: self.source_id.clone(),
            longitude: self.longitude,
            latitude: self.latitude,
            region_code: self.region_code.clone(),
            entity_id: self.entity_id.clone(),
            entity_name: self.entity_name.clone(),
            event_type,
            severity,
            confidence: self.confidence,
            tags: self.tags.clone().unwrap_or_default(),
            title: self.title.clone(),
            description: self.description.clone(),
            payload: self.payload.clone(),
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

/// Metadata about how and when a dataset was captured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetadata {
    /// Human-readable name for this dataset.
    pub name: String,
    /// When this dataset was exported.
    pub exported_at: DateTime<Utc>,
    /// Start of the event time range (inclusive).
    pub time_range_start: DateTime<Utc>,
    /// End of the event time range (exclusive).
    pub time_range_end: DateTime<Utc>,
    /// Total number of events in this dataset.
    pub event_count: usize,
    /// Breakdown of events by source_type.
    pub source_counts: std::collections::HashMap<String, usize>,
    /// Git commit hash of the code that exported the dataset, if available.
    pub git_hash: Option<String>,
}

/// A complete replay dataset: metadata + ordered events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDataset {
    pub metadata: ReplayMetadata,
    pub events: Vec<ReplayEvent>,
}

impl ReplayDataset {
    /// Build from raw events, computing metadata automatically.
    pub fn from_events(
        name: String,
        events: Vec<ReplayEvent>,
        time_range_start: DateTime<Utc>,
        time_range_end: DateTime<Utc>,
        git_hash: Option<String>,
    ) -> Self {
        let mut source_counts = std::collections::HashMap::new();
        for e in &events {
            *source_counts.entry(e.source_type.clone()).or_insert(0usize) += 1;
        }
        Self {
            metadata: ReplayMetadata {
                name,
                exported_at: Utc::now(),
                time_range_start,
                time_range_end,
                event_count: events.len(),
                source_counts,
                git_hash,
            },
            events,
        }
    }
}

/// A point-in-time snapshot of the situation graph state during replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySnapshot {
    /// Wall-clock time this snapshot represents.
    pub time: DateTime<Utc>,
    /// How many events have been ingested so far.
    pub events_ingested: usize,
    /// The active clusters at this point.
    pub clusters: Vec<SituationClusterDTO>,
}

/// Summary metrics from a completed replay run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetrics {
    /// Total events fed to the pipeline.
    pub total_events: usize,
    /// Events that passed the pipeline's internal relevance filter.
    pub events_accepted: usize,
    /// Number of internal clusters (before DTO quality gating).
    pub raw_cluster_count: usize,
    /// Number of clusters that pass production quality gates.
    pub final_cluster_count: usize,
    /// Peak number of clusters alive at any snapshot.
    pub peak_cluster_count: usize,
    /// Average certainty of final clusters (0.0-1.0).
    pub avg_certainty: f32,
    /// Number of clusters with AI-quality titles vs total.
    pub titled_clusters: usize,
    /// Duration of the replay in wall-clock milliseconds.
    pub replay_duration_ms: u64,
    /// Git commit hash of the code that produced this run.
    #[serde(default)]
    pub git_hash: Option<String>,
    /// Human-readable label for this run (e.g., "baseline", "wider-merge-radius").
    #[serde(default)]
    pub label: Option<String>,
    /// When this replay was executed.
    #[serde(default)]
    pub run_at: Option<DateTime<Utc>>,
    /// Snapshot history (uses raw clusters for full visibility).
    pub snapshots: Vec<ReplaySnapshot>,
    /// Pipeline config used for this run (for reproducibility).
    pub config: sr_config::PipelineConfig,
}
