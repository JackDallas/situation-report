use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_intel::AnalysisReport;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity, SourceType};
use ts_rs::TS;
use uuid::Uuid;

/// Shared entity resolver -- held by pipeline + AppState.
pub type SharedEntityResolver = Arc<RwLock<crate::entity_graph::EntityResolver>>;
/// Shared entity graph -- held by pipeline + AppState.
pub type SharedEntityGraph = Arc<RwLock<crate::entity_graph::EntityGraph>>;

use crate::situation_graph::SituationClusterDTO;

/// Four things the SSE stream can emit.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
#[serde(tag = "kind")]
pub enum PublishEvent {
    /// A single noteworthy event (passes importance filter)
    #[serde(rename = "event")]
    Event {
        #[serde(flatten)]
        event: InsertableEvent,
    },
    /// Multi-source correlated incident
    #[serde(rename = "incident")]
    Incident(Incident),
    /// Periodic digest of high-volume event types
    #[serde(rename = "summary")]
    Summary(Summary),
    /// Periodic intelligence analysis report
    #[serde(rename = "analysis")]
    Analysis(AnalysisReport),
    /// Current situation clusters (published periodically)
    #[serde(rename = "situations")]
    Situations { clusters: Vec<SituationClusterDTO> },
    /// A fired alert (keyword/entity/anomaly match)
    #[serde(rename = "alert")]
    Alert(crate::alerts::FiredAlert),
    /// Source health status change (healthy/degraded/error/rate_limited)
    #[serde(rename = "source_health")]
    SourceHealthChange {
        source_id: String,
        status: String,
        consecutive_failures: u32,
        last_error: Option<String>,
        last_success: Option<DateTime<Utc>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct Incident {
    pub id: Uuid,
    pub rule_id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    /// 0.0–1.0
    pub confidence: f32,
    pub first_seen: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub region_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub tags: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    /// Parent incident ID (this incident is a sub-incident)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    /// Related incident IDs (for cross-references)
    #[serde(default)]
    pub related_ids: Vec<Uuid>,
    /// IDs of incidents that were merged into this one
    #[serde(default)]
    pub merged_from: Vec<Uuid>,
    /// AI-generated display title (clearer than rule-generated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
}

impl Default for Incident {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            rule_id: String::new(),
            title: String::new(),
            description: String::new(),
            severity: Severity::Info,
            confidence: 0.0,
            first_seen: now,
            last_updated: now,
            region_code: None,
            latitude: None,
            longitude: None,
            tags: vec![],
            evidence: vec![],
            parent_id: None,
            related_ids: vec![],
            merged_from: vec![],
            display_title: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct EvidenceRef {
    pub source_type: SourceType,
    pub event_type: EventType,
    pub event_time: DateTime<Utc>,
    pub entity_id: Option<String>,
    pub title: Option<String>,
    pub role: EvidenceRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct Summary {
    pub event_type: EventType,
    #[ts(type = "number")]
    pub window_secs: u64,
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub unique_entities: u64,
    pub regions: Vec<String>,
    pub highlight: Option<InsertableEvent>,
}
