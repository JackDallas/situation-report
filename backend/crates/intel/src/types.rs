use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_types::{EventType, Severity};
use ts_rs::TS;
use uuid::Uuid;

/// An extracted entity from a news article.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub name: String,
    /// person, organization, location, weapon_system, military_unit
    pub entity_type: String,
    /// actor, target, location, mentioned
    pub role: Option<String>,
    /// Wikidata QID (e.g. Q12345) if recognized by the model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wikidata_qid: Option<String>,
}

/// Input for article enrichment — extracted from InsertableEvent.
#[derive(Debug, Clone)]
pub struct ArticleInput {
    pub title: String,
    pub description: String,
    pub source_url: Option<String>,
    pub source_country: Option<String>,
    pub language_hint: Option<String>,
    /// Source type for disambiguation context (e.g., "notam" so LLM knows FIR = Flight Information Region)
    pub source_type: Option<String>,
}

/// Result of a periodic Sonnet analysis of the current intelligence picture.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct AnalysisReport {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    /// 2-3 paragraph intelligence summary narrative
    pub narrative: String,
    /// Suggested merges between related situations
    pub suggested_merges: Vec<SuggestedMerge>,
    /// Topic clusters detected across recent events
    pub topic_clusters: Vec<TopicCluster>,
    /// Overall escalation assessment
    pub escalation_assessment: String,
    /// Key entity connections across sources
    pub key_entities: Vec<EntityConnection>,
    /// Model used for analysis
    pub model: String,
    /// Total tokens consumed
    pub tokens_used: u32,
    /// Tempo level at time of analysis (HIGH/ELEVATED/NORMAL)
    pub tempo: String,
}

/// A suggested merge between two related situations/incidents.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct SuggestedMerge {
    pub incident_a_id: String,
    pub incident_b_id: String,
    /// 0.0–1.0 confidence that these should be merged
    pub confidence: f32,
    /// Why these are related
    pub reason: String,
    /// Suggested combined title
    pub suggested_title: Option<String>,
}

/// A cluster of related topics across recent events.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct TopicCluster {
    pub label: String,
    pub topics: Vec<String>,
    pub event_count: u32,
    pub regions: Vec<String>,
}

/// A connection between entities seen across multiple sources.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct EntityConnection {
    pub entity_name: String,
    pub entity_type: String,
    pub source_count: u32,
    pub context: String,
}

/// Summary of a situation for input to analysis prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationSummary {
    pub id: String,
    pub title: String,
    pub severity: Severity,
    pub region: Option<String>,
    pub event_count: u32,
    pub source_types: Vec<String>,
    pub last_updated: DateTime<Utc>,
    /// Web search context from Exa (if available from cluster supplementary data)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_context: Option<String>,
}

/// Summary of a recent event for input to analysis prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSummary {
    pub event_type: EventType,
    pub title: Option<String>,
    pub severity: Severity,
    pub region: Option<String>,
    pub entity_name: Option<String>,
    pub event_time: DateTime<Utc>,
}

/// Budget status for the /api/intel/budget endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct BudgetStatus {
    pub daily_budget_usd: f64,
    pub spent_today_usd: f64,
    pub remaining_usd: f64,
    pub haiku_tokens_today: u64,
    pub sonnet_tokens_today: u64,
    pub budget_exhausted: bool,
    pub degraded: bool,
    /// Gemini spend this calendar month (invoice billing).
    #[serde(default)]
    pub gemini_spent_month_usd: f64,
    /// Hard monthly cap for Gemini ($30 default).
    #[serde(default)]
    pub gemini_month_limit_usd: f64,
}

// ---------------------------------------------------------------------------
// Extended enrichment types (B3: relationship + state change extraction)
// ---------------------------------------------------------------------------

/// A relationship between two entities extracted during enrichment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub rel_type: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    0.5
}

/// A state change detected during enrichment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedStateChange {
    pub entity: String,
    pub attribute: String,
    pub from: Option<String>,
    pub to: String,
    #[serde(default = "default_certainty")]
    pub certainty: String,
}

fn default_certainty() -> String {
    "alleged".to_string()
}

/// AI-inferred geographic location for an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredLocation {
    /// Human-readable place name (e.g. "Damascus, Syria")
    pub name: String,
    /// Latitude
    pub lat: f64,
    /// Longitude
    pub lon: f64,
}

/// Extended enrichment result including relationships and state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedArticleV2 {
    /// All fields from V1
    pub translated_title: String,
    pub summary: String,
    pub entities: Vec<Entity>,
    pub topics: Vec<String>,
    pub relevance_score: f32,
    pub sentiment: f32,
    pub original_language: String,
    /// New: extracted relationships between entities
    #[serde(default)]
    pub relationships: Vec<ExtractedRelationship>,
    /// New: detected entity state changes
    #[serde(default)]
    pub state_changes: Vec<ExtractedStateChange>,
    /// AI-inferred primary location where the event takes place
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_location: Option<InferredLocation>,
    /// Model used for enrichment
    #[serde(skip_deserializing)]
    pub model: String,
    /// Total tokens consumed
    #[serde(skip_deserializing)]
    pub tokens_used: u32,
}
