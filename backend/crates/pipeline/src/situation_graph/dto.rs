//! Data transfer objects and API conversion logic.
//!
//! Contains `SituationClusterDTO`, `ClusterGapAnalysis`, and the `active_clusters()`
//! conversion from internal `SituationCluster` to sorted API output.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_types::{Severity, SourceType};
use ts_rs::TS;
use uuid::Uuid;

use sr_intel::search::{GapType, InformationGap};

use super::lifecycle::SituationPhase;
use super::scoring::effective_source_diversity;
use super::SituationGraph;

/// DTO for SSE / API output -- all collection fields are sorted Vecs.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "SituationCluster", export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct SituationClusterDTO {
    pub id: Uuid,
    pub title: String,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    pub region_codes: Vec<String>,
    pub severity: Severity,
    pub first_seen: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub centroid: Option<(f64, f64)>,
    pub event_count: usize,
    pub source_types: Vec<String>,
    pub source_count: usize,
    pub parent_id: Option<String>,
    pub child_ids: Vec<String>,
    pub supplementary: Option<sr_intel::search::SupplementaryData>,
    pub phase: SituationPhase,
    pub phase_changed_at: DateTime<Utc>,
    pub peak_event_rate: f64,
    /// Certainty score (0.0-1.0) based on source diversity, event count, entities, and enrichment.
    #[serde(default)]
    pub certainty: f32,
    /// Composite anomaly score (0.0-1.0) based on burst detection across topics.
    #[serde(default)]
    pub anomaly_score: f64,
    /// Recent event titles for display (capped at 10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_titles: Vec<String>,
    /// Latest narrative text for this situation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative_text: Option<String>,

    // -- Fields for DB persistence only (not sent to frontend) --

    /// Count of high-signal events (conflict, news, geo).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub signal_event_count: usize,
    /// Whether this cluster has received an AI-generated title.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub has_ai_title: bool,
    /// Signal event count when the title was last generated.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub title_signal_count_at_gen: usize,
    /// When the AI title was last generated.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub last_title_gen: DateTime<Utc>,
    /// Recent (event_time, source_id) pairs for DB persistence (last 100).
    #[serde(skip)]
    #[ts(skip)]
    pub event_ids: Vec<(DateTime<Utc>, String)>,
}

/// Result of gap analysis for a cluster -- used to drive intelligent search selection.
#[derive(Debug, Clone)]
pub struct ClusterGapAnalysis {
    pub cluster_id: Uuid,
    pub gaps: Vec<InformationGap>,
    pub priority_score: u32,
    pub recommended_query: String,
    pub recommended_gap: GapType,
    /// When this situation was first seen -- used to set `startPublishedDate` on Exa queries.
    pub first_seen: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// SituationGraph DTO methods
// ---------------------------------------------------------------------------

impl SituationGraph {
    /// Return all active clusters as DTOs, sorted by `last_updated` descending.
    /// Quality gate: clusters must have >= 3 events OR >= 2 distinct source types.
    pub fn active_clusters(&self) -> Vec<SituationClusterDTO> {
        // Build a child lookup: parent_id -> Vec<child_id>
        let mut children_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                children_map.entry(pid).or_default().push(c.id);
            }
        }

        let mut dtos: Vec<SituationClusterDTO> = self
            .clusters
            .values()
            .filter(|c| {
                let has_children = children_map.contains_key(&c.id);
                if has_children {
                    true
                } else if c.parent_id.is_some() {
                    c.event_count >= self.config.quality.min_events_child
                } else {
                    // Standalone (no parent, no children) — strict quality gate
                    if c.event_count < self.config.quality.min_events_standalone {
                        return false;
                    }
                    // Single-source telemetry clusters never surface as standalone top-level.
                    // FIRMS/aviation create too much noise — they belong as children only.
                    let is_telemetry_only = c.source_types.iter().all(|s| matches!(s,
                        SourceType::Firms | SourceType::Opensky
                        | SourceType::AirplanesLive | SourceType::AdsbFi | SourceType::AdsbLol));
                    if is_telemetry_only {
                        return false;
                    }
                    // Standalone clusters need a generated title to surface
                    if !c.has_ai_title && c.title.is_empty() {
                        return false;
                    }
                    // Require at least 2 distinct source types for standalone
                    let diversity = effective_source_diversity(&c.source_types);
                    if diversity < 2 {
                        return false;
                    }
                    true
                }
            })
            .map(|c| {
                let mut entities: Vec<String> = c.entities.iter().cloned().collect();
                entities.sort();
                entities.truncate(20);
                let mut topics: Vec<String> = c.topics.iter().cloned().collect();
                topics.sort();
                topics.truncate(15);
                let mut region_codes: Vec<String> = c.region_codes.iter().cloned().collect();
                region_codes.sort();
                let mut source_types: Vec<String> = c.source_types.iter().map(|st| st.to_string()).collect();
                source_types.sort();
                let source_count = effective_source_diversity(&c.source_types);

                let child_ids: Vec<String> = children_map
                    .get(&c.id)
                    .map(|ids| ids.iter().map(|id| id.to_string()).collect())
                    .unwrap_or_default();

                let centroid = c.centroid;

                let event_ids: Vec<(DateTime<Utc>, String)> = c
                    .event_ids
                    .iter()
                    .rev()
                    .take(100)
                    .cloned()
                    .collect();

                SituationClusterDTO {
                    id: c.id,
                    title: c.title.clone(),
                    entities,
                    topics,
                    region_codes,
                    severity: c.severity,
                    first_seen: c.first_seen,
                    last_updated: c.last_updated,
                    centroid,
                    event_count: c.event_count,
                    source_types,
                    source_count,
                    parent_id: c.parent_id.map(|id| id.to_string()),
                    child_ids,
                    supplementary: c.supplementary.clone(),
                    phase: c.phase,
                    phase_changed_at: c.phase_changed_at,
                    peak_event_rate: c.peak_event_rate,
                    certainty: c.certainty,
                    anomaly_score: c.anomaly_score,
                    event_titles: c.event_titles.clone(),
                    narrative_text: None,
                    signal_event_count: c.signal_event_count,
                    has_ai_title: c.has_ai_title,
                    title_signal_count_at_gen: c.title_signal_count_at_gen,
                    last_title_gen: c.last_title_gen,
                    event_ids,
                }
            })
            .collect();

        // Fix orphans: promoted children whose parent was pruned must pass standalone quality gate.
        let output_ids: HashSet<Uuid> = dtos.iter().map(|d| d.id).collect();
        dtos.retain_mut(|dto| {
            if let Some(ref pid_str) = dto.parent_id {
                if let Ok(pid) = pid_str.parse::<Uuid>() {
                    if !output_ids.contains(&pid) {
                        // Parent gone — apply standalone quality gate before promoting
                        if dto.event_count >= self.config.quality.min_events_standalone
                            && dto.source_count >= 2
                        {
                            dto.parent_id = None;
                            return true;
                        } else {
                            return false; // drop the orphan
                        }
                    }
                }
            }
            true
        });

        dtos.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));

        // Cap natural-disaster standalone situations at 2 top-level
        let mut nd_count = 0usize;
        dtos.retain(|d| {
            if d.parent_id.is_some() || !d.child_ids.is_empty() {
                return true;
            }
            let is_nd = d.topics.iter().any(|t| super::scoring::is_natural_disaster_topic(t));
            if is_nd {
                nd_count += 1;
                nd_count <= 2
            } else {
                true
            }
        });

        dtos
    }

    /// Return clusters that have AI titles and are due for (re-)search.
    pub fn clusters_needing_search(&self, pending: &HashSet<Uuid>) -> Vec<&super::SituationCluster> {
        let now = Utc::now();
        self.clusters
            .values()
            .filter(|c| {
                if !c.has_ai_title || pending.contains(&c.id) {
                    return false;
                }
                if c.event_count < 3 {
                    return false;
                }

                match c.last_searched {
                    None => true,
                    Some(last) => {
                        let elapsed = now.signed_duration_since(last);
                        let interval = if c.severity.rank() >= Severity::High.rank() {
                            chrono::Duration::minutes(30)
                        } else {
                            chrono::Duration::hours(2)
                        };
                        elapsed >= interval
                    }
                }
            })
            .collect()
    }

    /// Perform gap analysis on all eligible clusters and return them sorted by
    /// search priority (highest first).
    pub fn clusters_needing_search_with_gaps(&self, pending: &HashSet<Uuid>) -> Vec<ClusterGapAnalysis> {
        let now = Utc::now();
        let mut analyses: Vec<ClusterGapAnalysis> = self
            .clusters
            .values()
            .filter(|c| {
                if !c.has_ai_title || pending.contains(&c.id) {
                    return false;
                }
                if c.event_count < 3 {
                    return false;
                }
                match c.last_searched {
                    None => true,
                    Some(last) => {
                        let elapsed = now.signed_duration_since(last);
                        let interval = if c.severity.rank() >= Severity::High.rank() {
                            chrono::Duration::minutes(30)
                        } else {
                            chrono::Duration::hours(2)
                        };
                        elapsed >= interval
                    }
                }
            })
            .map(|c| {
                let has_enrichment = c.supplementary.is_some();
                let supplementary_age_secs = c.last_searched
                    .map(|last| now.signed_duration_since(last).num_seconds());

                let gap_input = sr_intel::GapAnalysisInput {
                    source_types: c.source_types.iter().map(|st| st.to_string()).collect(),
                    entities: c.entities.clone(),
                    topics: c.topics.clone(),
                    region_codes: c.region_codes.clone(),
                    severity: c.severity,
                    event_count: c.event_count,
                    centroid: c.centroid,
                    has_supplementary: c.supplementary.is_some(),
                    supplementary_age_secs,
                    search_history: c.search_history.clone(),
                    has_enrichment,
                    last_updated: c.last_updated,
                    first_seen: c.first_seen,
                };

                let gaps = sr_intel::analyze_gaps(&gap_input);
                if gaps.is_empty() {
                    let entities: Vec<String> = c.entities.iter().take(5).cloned().collect();
                    let topics: Vec<String> = c.topics.iter().take(5).cloned().collect();
                    let regions: Vec<String> = c.region_codes.iter().take(3).cloned().collect();
                    return ClusterGapAnalysis {
                        cluster_id: c.id,
                        gaps: Vec::new(),
                        priority_score: 1,
                        recommended_query: sr_intel::build_search_query(
                            &c.title, &entities, &topics, &regions,
                        ),
                        recommended_gap: GapType::Corroboration,
                        first_seen: c.first_seen,
                    };
                }

                let priority = sr_intel::compute_search_priority(&gap_input, &gaps);
                let recommended_gap = gaps[0].gap_type;

                let entities: Vec<String> = c.entities.iter().take(5).cloned().collect();
                let topics: Vec<String> = c.topics.iter().take(5).cloned().collect();
                let regions: Vec<String> = c.region_codes.iter().take(3).cloned().collect();

                let recommended_query = sr_intel::build_gap_query(
                    recommended_gap,
                    &c.title,
                    &entities,
                    &topics,
                    &regions,
                );

                ClusterGapAnalysis {
                    cluster_id: c.id,
                    gaps,
                    priority_score: priority,
                    recommended_query,
                    recommended_gap,
                    first_seen: c.first_seen,
                }
            })
            .collect();

        analyses.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));
        analyses
    }
}
