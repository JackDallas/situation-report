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

    /// Coordinate buffer for median centroid calculation (last 30 coords).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub coord_buffer: Vec<(f64, f64)>,
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
    /// Total events ever ingested (survives shedding).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub total_events_ingested: usize,
    /// Count of events directly ingested into this cluster (not inherited from merges).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub direct_event_count: usize,
    /// Source types from directly ingested events only (not inherited from merges).
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub direct_source_types: Vec<String>,
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
    /// Return ALL internal clusters as DTOs without quality gating.
    /// Used by the replay harness for diagnostics — bypasses the production
    /// filters (min events, source diversity, garbage title checks).
    pub fn raw_clusters(&self) -> Vec<SituationClusterDTO> {
        let children_map: HashMap<Uuid, Vec<Uuid>> = {
            let mut m: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
            for c in self.clusters.values() {
                if let Some(pid) = c.parent_id {
                    m.entry(pid).or_default().push(c.id);
                }
            }
            m
        };

        let mut dtos: Vec<SituationClusterDTO> = self
            .clusters
            .values()
            .map(|c| self.cluster_to_dto(c, &children_map))
            .collect();

        dtos.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
        dtos
    }

    /// Convert a single internal cluster to a DTO (shared by active_clusters and raw_clusters).
    fn cluster_to_dto(
        &self,
        c: &super::SituationCluster,
        children_map: &HashMap<Uuid, Vec<Uuid>>,
    ) -> SituationClusterDTO {
        let mut entities: Vec<String> = c.entities.iter().cloned().collect();
        entities.sort();
        entities.truncate(20);
        let mut topics: Vec<String> = c.topics.iter().cloned().collect();
        topics.sort();
        topics.truncate(15);
        let mut region_codes: Vec<String> = c.region_codes.iter().cloned().collect();
        region_codes.sort();
        region_codes.truncate(4);
        let mut source_types: Vec<String> =
            c.source_types.iter().map(|st| st.to_string()).collect();
        source_types.sort();
        let source_count = effective_source_diversity(&c.source_types);

        let child_ids: Vec<String> = children_map
            .get(&c.id)
            .map(|ids| ids.iter().map(|id| id.to_string()).collect())
            .unwrap_or_default();

        let mut certainty = c.certainty;
        if c.topics.len() >= self.config.quality.incoherent_topic_threshold {
            certainty *= self.config.quality.incoherent_topic_penalty as f32;
        }
        if c.region_codes.len() >= 4 {
            certainty *= 0.7;
        }

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
            centroid: c.centroid,
            event_count: c.event_count,
            source_types,
            source_count,
            parent_id: c.parent_id.map(|id| id.to_string()),
            child_ids,
            supplementary: c.supplementary.clone(),
            phase: c.phase,
            phase_changed_at: c.phase_changed_at,
            peak_event_rate: c.peak_event_rate,
            certainty,
            anomaly_score: c.anomaly_score,
            event_titles: c.event_titles.clone(),
            narrative_text: None,
            coord_buffer: c.coord_buffer.clone(),
            signal_event_count: c.signal_event_count,
            has_ai_title: c.has_ai_title,
            title_signal_count_at_gen: c.title_signal_count_at_gen,
            last_title_gen: c.last_title_gen,
            event_ids,
            total_events_ingested: c.total_events_ingested,
            direct_event_count: c.direct_event_count,
            direct_source_types: c.direct_source_types.iter().map(|st| st.to_string()).collect(),
        }
    }

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
                    // Parents with garbage titles should not surface either —
                    // their children will be promoted to standalone.
                    if super::SituationGraph::is_garbage_title(&c.title) {
                        return false;
                    }
                    true
                } else if c.parent_id.is_some() {
                    c.event_count >= self.config.quality.min_events_child
                } else {
                    // Standalone (no parent, no children) — strict quality gate
                    let min_events = self.config.quality.min_events_standalone;
                    if c.event_count < min_events {
                        return false;
                    }
                    // Medium-severity standalone needs more evidence to justify a
                    // top-level slot — prevents marginal news clusters from flooding the list.
                    if c.severity == Severity::Medium && c.event_count < min_events + self.config.quality.medium_standalone_penalty as usize {
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
                    // Standalone clusters need a non-empty, non-garbage title to surface
                    if c.title.is_empty() || super::SituationGraph::is_garbage_title(&c.title) {
                        return false;
                    }
                    // Require at least 2 distinct source types for standalone
                    let diversity = effective_source_diversity(&c.source_types);
                    if diversity < 2 {
                        return false;
                    }
                    // Entity-less clusters spanning 4+ regions with topic soup are noise magnets.
                    // They absorb unrelated events across the globe — never surface them.
                    if c.entities.is_empty() && c.region_codes.len() >= 4 {
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
                region_codes.truncate(4);
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

                // Incoherent clusters (too many diverse topics) get severity capped
                // to prevent contaminated mega-clusters from dominating the top.
                let severity = if c.topics.len() >= self.config.quality.incoherent_topic_threshold {
                    std::cmp::min(c.severity, Severity::High)
                } else {
                    c.severity
                };

                // Penalize certainty for incoherent clusters:
                // - 12+ topics: ×0.6 (topic soup)
                // - 4 regions: ×0.7 (geographically incoherent)
                // - Both: ×0.42
                let mut certainty = c.certainty;
                if c.topics.len() >= self.config.quality.incoherent_topic_threshold {
                    certainty *= self.config.quality.incoherent_topic_penalty as f32;
                }
                if c.region_codes.len() >= 4 {
                    certainty *= 0.7;
                }

                SituationClusterDTO {
                    id: c.id,
                    title: c.title.clone(),
                    entities,
                    topics,
                    region_codes,
                    severity,
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
                    certainty,
                    anomaly_score: c.anomaly_score,
                    event_titles: c.event_titles.clone(),
                    narrative_text: None,
                    coord_buffer: c.coord_buffer.clone(),
                    signal_event_count: c.signal_event_count,
                    has_ai_title: c.has_ai_title,
                    title_signal_count_at_gen: c.title_signal_count_at_gen,
                    last_title_gen: c.last_title_gen,
                    event_ids,
                    total_events_ingested: c.total_events_ingested,
                    direct_event_count: c.direct_event_count,
                    direct_source_types: c.direct_source_types.iter().map(|st| st.to_string()).collect(),
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

        // --- Pass A: Collapse 1-child parents where child has same title ---
        let single_child_collapse: HashSet<Uuid> = {
            let dto_map: HashMap<Uuid, &SituationClusterDTO> = dtos.iter().map(|d| (d.id, d)).collect();
            dtos.iter()
                .filter(|d| d.child_ids.len() == 1 && d.parent_id.is_none())
                .filter_map(|d| {
                    let child_id: Uuid = d.child_ids[0].parse().ok()?;
                    let child = dto_map.get(&child_id)?;
                    if d.title.to_lowercase() == child.title.to_lowercase() {
                        Some(child_id)
                    } else {
                        None
                    }
                })
                .collect()
        };
        if !single_child_collapse.is_empty() {
            dtos.retain(|d| !single_child_collapse.contains(&d.id));
            for d in &mut dtos {
                d.child_ids.retain(|cid| {
                    cid.parse::<Uuid>().map_or(true, |id| !single_child_collapse.contains(&id))
                });
            }
        }

        // --- Pass B: Flatten multi-level hierarchy + fix orphans ---
        // Must run before ND cap so promoted clusters get counted.
        {
            let current_ids: HashSet<Uuid> = dtos.iter().map(|d| d.id).collect();
            let parent_map: HashMap<Uuid, Option<Uuid>> = dtos.iter()
                .map(|d| (d.id, d.parent_id.as_ref().and_then(|s| s.parse::<Uuid>().ok())))
                .collect();

            // Walk up chain to find root parent
            let find_root = |start_pid: Uuid| -> Option<Uuid> {
                let mut current = start_pid;
                for _ in 0..10 {
                    match parent_map.get(&current).copied().flatten() {
                        Some(gp) if current_ids.contains(&gp) => current = gp,
                        _ => break,
                    }
                }
                if current != start_pid { Some(current) } else { None }
            };

            let mut reparents: Vec<(Uuid, Uuid)> = Vec::new();
            let mut orphans_promote: Vec<Uuid> = Vec::new();
            let mut orphans_drop: Vec<Uuid> = Vec::new();
            for d in dtos.iter() {
                if let Some(ref pid_str) = d.parent_id {
                    if let Ok(pid) = pid_str.parse::<Uuid>() {
                        if !current_ids.contains(&pid) {
                            if d.event_count >= self.config.quality.min_events_standalone
                                && d.source_count >= 2
                            {
                                orphans_promote.push(d.id);
                            } else {
                                orphans_drop.push(d.id);
                            }
                        } else if let Some(root) = find_root(pid) {
                            reparents.push((d.id, root));
                        }
                    }
                }
            }

            // Apply reparents (grandchild → root)
            let reparent_map: HashMap<Uuid, Uuid> = reparents.iter().copied().collect();
            for d in dtos.iter_mut() {
                if let Some(&new_parent) = reparent_map.get(&d.id) {
                    d.parent_id = Some(new_parent.to_string());
                }
                if orphans_promote.contains(&d.id) {
                    d.parent_id = None;
                }
            }
            // Update child_ids: remove reparented children from old parents, add to roots
            for d in dtos.iter_mut() {
                // Remove children that were reparented away
                d.child_ids.retain(|cid| {
                    if let Ok(cid_uuid) = cid.parse::<Uuid>() {
                        if let Some(&new_parent) = reparent_map.get(&cid_uuid) {
                            return new_parent == d.id; // keep only if we're the new parent
                        }
                    }
                    true
                });
                // Add newly reparented children
                for (child_id, new_parent) in &reparents {
                    if *new_parent == d.id {
                        let child_str = child_id.to_string();
                        if !d.child_ids.contains(&child_str) {
                            d.child_ids.push(child_str);
                        }
                    }
                }
            }

            // Drop orphans that don't pass standalone gate
            dtos.retain(|d| !orphans_drop.contains(&d.id));
        }

        // --- Pass C: Cap natural-disaster top-level situations ---
        // Runs after flatten so all promoted clusters are correctly counted.
        let mut nd_standalone = 0usize;
        let mut nd_parent = 0usize;
        dtos.retain(|d| {
            if d.parent_id.is_some() {
                return true;
            }
            let is_nd = d.topics.iter().any(|t| super::scoring::is_natural_disaster_topic(t))
                || super::scoring::is_natural_disaster_title(&d.title);
            if !is_nd {
                return true;
            }
            if d.child_ids.is_empty() {
                nd_standalone += 1;
                nd_standalone <= self.config.quality.nd_standalone_cap
            } else {
                nd_parent += 1;
                nd_parent <= self.config.quality.nd_parent_cap
            }
        });

        // --- Pass C2: Cap routine-event top-level situations ---
        // Routine diplomatic/institutional/economic events should not flood the board.
        let mut routine_standalone = 0usize;
        let mut routine_parent = 0usize;
        dtos.retain(|d| {
            if d.parent_id.is_some() {
                return true;
            }
            // Skip if it has conflict topics — those are genuine crises that happen to
            // overlap with a routine event context (e.g., attack during a summit).
            let has_conflict = d.topics.iter().any(|t| super::scoring::is_conflict_topic(t));
            if has_conflict {
                return true;
            }
            let is_routine = d.topics.iter().any(|t| super::scoring::is_routine_event_topic(t))
                || super::scoring::is_routine_event_title(&d.title);
            if !is_routine {
                return true;
            }
            if d.child_ids.is_empty() {
                routine_standalone += 1;
                routine_standalone <= self.config.quality.routine_standalone_cap
            } else {
                routine_parent += 1;
                routine_parent <= self.config.quality.routine_parent_cap
            }
        });

        // --- Pass D: Final orphan cleanup after ND/routine caps ---
        {
            let final_ids: HashSet<Uuid> = dtos.iter().map(|d| d.id).collect();
            let mut promote = Vec::new();
            let mut drop = Vec::new();
            for d in dtos.iter() {
                if let Some(ref pid_str) = d.parent_id {
                    if let Ok(pid) = pid_str.parse::<Uuid>() {
                        if !final_ids.contains(&pid) {
                            if d.event_count >= self.config.quality.min_events_standalone
                                && d.source_count >= 2
                            {
                                promote.push(d.id);
                            } else {
                                drop.push(d.id);
                            }
                        }
                    }
                }
            }
            for d in dtos.iter_mut() {
                if promote.contains(&d.id) {
                    d.parent_id = None;
                }
            }
            dtos.retain(|d| !drop.contains(&d.id));
        }

        // --- Pass E: Merge near-duplicate top-level titles ---
        // e.g., "Central Israel Rocket Alerts" + "Israel Central Rocket Alerts" → keep higher-event one
        {
            let top_level_indices: Vec<usize> = dtos.iter().enumerate()
                .filter(|(_, d)| d.parent_id.is_none())
                .map(|(i, _)| i)
                .collect();

            let mut merge_into: HashMap<usize, usize> = HashMap::new(); // victim → winner
            for ii in 0..top_level_indices.len() {
                let idx_a = top_level_indices[ii];
                if merge_into.contains_key(&idx_a) { continue; }
                for jj in (ii + 1)..top_level_indices.len() {
                    let idx_b = top_level_indices[jj];
                    if merge_into.contains_key(&idx_b) { continue; }
                    let jaccard = super::scoring::title_jaccard_filtered(&dtos[idx_a].title, &dtos[idx_b].title);
                    if jaccard >= 0.75 {
                        // Keep the one with more events
                        let (winner, loser) = if dtos[idx_a].event_count >= dtos[idx_b].event_count {
                            (idx_a, idx_b)
                        } else {
                            (idx_b, idx_a)
                        };
                        merge_into.insert(loser, winner);
                    }
                }
            }

            if !merge_into.is_empty() {
                // Pre-compute loser id → winner id mappings
                let reparent_map: HashMap<String, String> = merge_into.iter()
                    .map(|(&loser, &winner)| (dtos[loser].id.to_string(), dtos[winner].id.to_string()))
                    .collect();

                // Move children of loser to winner
                let loser_children: HashMap<usize, Vec<String>> = merge_into.iter()
                    .map(|(&loser, _)| (loser, dtos[loser].child_ids.clone()))
                    .collect();
                for (&loser, &winner) in &merge_into {
                    if let Some(children) = loser_children.get(&loser) {
                        for cid in children {
                            if !dtos[winner].child_ids.contains(cid) {
                                dtos[winner].child_ids.push(cid.clone());
                            }
                        }
                    }
                }
                // Reparent loser's children to winner
                for d in dtos.iter_mut() {
                    if let Some(ref pid) = d.parent_id {
                        if let Some(new_pid) = reparent_map.get(pid) {
                            d.parent_id = Some(new_pid.clone());
                        }
                    }
                }
                let losers: HashSet<usize> = merge_into.keys().copied().collect();
                let mut idx = 0;
                dtos.retain(|_| {
                    let keep = !losers.contains(&idx);
                    idx += 1;
                    keep
                });
            }
        }

        // --- Final safety net: remove any top-level DTO with a garbage title ---
        // This catches titles that slipped through the initial filter (e.g., restored
        // from DB with stale formulaic titles that haven't been AI-regenerated yet).
        dtos.retain(|d| {
            if d.parent_id.is_some() { return true; } // children are allowed
            !super::SituationGraph::is_garbage_title(&d.title)
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
