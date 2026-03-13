//! Situation lifecycle phases, transitions, pruning, and sweep logic.
//!
//! Contains `PhaseTransition`, `PhaseMetrics`, the `SituationPhase` FSM evaluation,
//! gap tolerance computation, severity recomputation, and periodic sweep/prune operations.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_types::{Severity, SourceType};
use uuid::Uuid;

use sr_embeddings::EmbeddingCache;
use tracing::{debug, info};

use super::scoring::{effective_source_diversity, is_conflict_source, is_conflict_topic, is_cyber_source, is_natural_disaster_topic};
use super::{SituationCluster, SituationGraph};

// Re-export the shared enum from sr-types.
pub use sr_types::SituationPhase;

/// A recorded phase transition for audit/history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub from_phase: SituationPhase,
    pub to_phase: SituationPhase,
    pub trigger_reason: String,
    pub metrics_snapshot: serde_json::Value,
    pub transitioned_at: DateTime<Utc>,
}

/// Metrics used to evaluate phase transitions.
#[derive(Debug, Clone, Default)]
pub(crate) struct PhaseMetrics {
    /// Events in last 5 minutes.
    pub event_velocity_5m: usize,
    /// Events in last 30 minutes.
    pub event_velocity_30m: usize,
    /// Peak 5-minute event rate seen.
    pub peak_rate: f64,
    /// Current 5-minute event rate.
    pub current_rate: f64,
    /// Number of distinct source types.
    pub source_diversity: usize,
    /// Maximum severity rank (0-4).
    pub max_severity_rank: u8,
    /// Hours since last event.
    pub hours_since_last_event: f64,
    /// Total event count.
    pub event_count: usize,
}

impl PhaseMetrics {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "event_velocity_5m": self.event_velocity_5m,
            "event_velocity_30m": self.event_velocity_30m,
            "peak_rate": self.peak_rate,
            "current_rate": self.current_rate,
            "source_diversity": self.source_diversity,
            "max_severity_rank": self.max_severity_rank,
            "hours_since_last_event": self.hours_since_last_event,
            "event_count": self.event_count,
        })
    }
}

/// Compute a dynamic gap tolerance (in hours) for a situation cluster.
pub(crate) fn compute_gap_tolerance(cluster: &SituationCluster, phases: &sr_config::PhaseConfig, now: DateTime<Utc>) -> f64 {
    let base = match cluster.severity {
        Severity::Critical => phases.gap_tolerance_critical_hours,
        Severity::High => phases.gap_tolerance_high_hours,
        Severity::Medium => phases.gap_tolerance_medium_hours,
        Severity::Low | Severity::Info => phases.gap_tolerance_low_hours,
    };

    let diversity = effective_source_diversity(&cluster.source_types);

    // Single-source clusters resolve quickly — no corroboration means lower persistence
    if diversity <= 1 {
        return base.min(phases.gap_tolerance_single_source_max_hours);
    }

    // Use decayed peak rate — the raw peak_event_rate is all-time and never decays,
    // which permanently inflates gap tolerance for clusters that once had a burst.
    let mins_since_peak = (now - cluster.peak_rate_at).num_minutes().max(0) as f64;
    let decay = 0.5_f64.powf(mins_since_peak / phases.peak_decay_half_life_mins);
    let decayed_peak = cluster.peak_event_rate * decay;
    let activity_factor = (decayed_peak / 5.0).clamp(0.0, 3.0);
    let source_factor = (diversity as f64 / 2.0).clamp(1.0, 2.0);
    let raw = base * (1.0 + activity_factor * 0.5) * (1.0 + (source_factor - 1.0) * 0.3);

    // Hard ceiling prevents infinite gap tolerance inflation
    raw.min(phases.gap_tolerance_max_hours)
}

/// Evaluate the next phase for a cluster based on 6 signals.
pub(crate) fn evaluate_phase_transition(
    current: SituationPhase,
    metrics: &PhaseMetrics,
    gap_tolerance: f64,
    phases: &sr_config::PhaseConfig,
) -> Option<(SituationPhase, String)> {
    match current {
        SituationPhase::Emerging => {
            if (metrics.event_count >= phases.emerging_min_events && metrics.source_diversity >= phases.emerging_min_sources)
                || metrics.max_severity_rank >= 3
            {
                return Some((
                    SituationPhase::Developing,
                    format!(
                        "Escalated: {} events from {} sources, severity={}",
                        metrics.event_count, metrics.source_diversity, metrics.max_severity_rank
                    ),
                ));
            }
            if metrics.hours_since_last_event > phases.emerging_stale_hours {
                return Some((
                    SituationPhase::Resolved,
                    format!("Resolved: no activity for >{:.1}h in emerging phase", metrics.hours_since_last_event),
                ));
            }
            None
        }
        SituationPhase::Developing => {
            let signal_count = [
                metrics.event_velocity_5m >= phases.developing_velocity_threshold,
                metrics.source_diversity >= 3,
                metrics.max_severity_rank >= 3,
                metrics.event_count >= 10,
            ]
            .iter()
            .filter(|&&b| b)
            .count();

            if signal_count >= phases.developing_signal_count || metrics.event_velocity_5m >= phases.developing_velocity_threshold {
                return Some((
                    SituationPhase::Active,
                    format!(
                        "Escalated: {signal_count} signals aligned, velocity_5m={}",
                        metrics.event_velocity_5m
                    ),
                ));
            }
            if metrics.hours_since_last_event > gap_tolerance {
                return Some((
                    SituationPhase::Declining,
                    format!("Declining: no activity for >{:.1}h (gap_tolerance={:.1}h)", metrics.hours_since_last_event, gap_tolerance),
                ));
            }
            None
        }
        SituationPhase::Active => {
            if metrics.peak_rate > 0.0 && metrics.current_rate < metrics.peak_rate * phases.active_decline_rate_ratio {
                return Some((
                    SituationPhase::Declining,
                    format!(
                        "Declining: rate {:.1} < {:.0}% of peak {:.1}",
                        metrics.current_rate, phases.active_decline_rate_ratio * 100.0, metrics.peak_rate
                    ),
                ));
            }
            let active_threshold = gap_tolerance * phases.active_decline_gap_factor;
            if metrics.hours_since_last_event > active_threshold {
                return Some((
                    SituationPhase::Declining,
                    format!("Declining: no activity for >{:.1}h in active phase (threshold={:.1}h)", metrics.hours_since_last_event, active_threshold),
                ));
            }
            None
        }
        SituationPhase::Declining => {
            if metrics.peak_rate > 0.0 && metrics.current_rate > metrics.peak_rate * 0.7 {
                return Some((
                    SituationPhase::Active,
                    format!(
                        "Re-escalated: rate {:.1} recovered >70% of peak {:.1}",
                        metrics.current_rate, metrics.peak_rate
                    ),
                ));
            }
            let resolve_threshold = gap_tolerance * phases.declining_resolve_gap_factor;
            if metrics.hours_since_last_event > resolve_threshold {
                return Some((
                    SituationPhase::Resolved,
                    format!("Resolved: no activity for >{:.1}h (threshold={:.1}h)", metrics.hours_since_last_event, resolve_threshold),
                ));
            }
            None
        }
        SituationPhase::Resolved => {
            let historical_threshold = gap_tolerance * phases.resolved_historical_gap_factor;
            if metrics.hours_since_last_event > historical_threshold {
                return Some((
                    SituationPhase::Historical,
                    format!("Archived: >{:.1}h since resolution (threshold={:.1}h)", metrics.hours_since_last_event, historical_threshold),
                ));
            }
            if metrics.event_velocity_5m > 0 {
                return Some((
                    SituationPhase::Developing,
                    "Re-opened: new events detected".to_string(),
                ));
            }
            None
        }
        SituationPhase::Historical => None,
    }
}

/// Recompute cluster severity based on aggregate signals beyond individual event severity.
/// Computes severity from scratch (can both escalate and downgrade).
/// Returns true if severity changed.
pub(crate) fn recompute_cluster_severity(cluster: &mut SituationCluster, severity_config: &sr_config::SeverityConfig, certainty_config: &sr_config::CertaintyConfig, now: DateTime<Utc>) -> bool {
    let old_severity = cluster.severity;
    let is_child = cluster.parent_id.is_some();

    // When a cluster is a child (reparented under a parent), use direct stats
    // to avoid inflated event_count/source_types inherited from prior merges.
    let effective_source_types: &HashSet<SourceType> = if is_child && !cluster.direct_source_types.is_empty() {
        &cluster.direct_source_types
    } else {
        &cluster.source_types
    };
    let effective_event_count = if is_child && cluster.direct_event_count > 0 {
        cluster.direct_event_count
    } else {
        cluster.event_count
    };

    let has_conflict_sources = effective_source_types.iter().any(|s| is_conflict_source(*s));
    let has_conflict_topics = cluster.topics.iter().any(|t| is_conflict_topic(t));
    let is_natural_disaster = cluster.topics.iter().any(|t| is_natural_disaster_topic(t));
    let is_active_or_developing = matches!(
        cluster.phase,
        SituationPhase::Developing | SituationPhase::Active
    );

    // Natural disaster clusters (wildfire, earthquake, flood, etc.) cap at high severity
    // unless they also have genuine conflict signals (e.g., arson in conflict zone).
    let disaster_only = is_natural_disaster && !has_conflict_topics;
    let severity_cap = if disaster_only { Severity::High } else { Severity::Critical };

    let has_cyber_sources = effective_source_types.iter().any(|s| is_cyber_source(*s));
    let source_diversity = super::scoring::effective_source_diversity(effective_source_types);

    // Compute severity from scratch based on current cluster state.
    // Multiple paths to escalation: conflict, cyber, environmental, or sheer corroboration.
    let computed = if cluster.phase == SituationPhase::Active
        && has_conflict_sources
        && has_conflict_topics
        && effective_event_count >= severity_config.critical_min_events
        && source_diversity >= severity_config.critical_min_sources
    {
        // Active armed conflict with heavy multi-source corroboration
        Severity::Critical.min(severity_cap)
    } else if is_active_or_developing
        && has_conflict_topics
        && effective_event_count >= severity_config.high_min_events
        && source_diversity >= severity_config.critical_min_sources.min(3)
    {
        // Developing/active conflict situation with multi-source corroboration
        Severity::High.min(severity_cap)
    } else if is_active_or_developing
        && (has_cyber_sources || is_natural_disaster)
        && source_diversity >= severity_config.medium_min_sources
        && effective_event_count >= severity_config.high_min_events
    {
        // Multi-source cyber or environmental disaster, actively developing
        Severity::High.min(severity_cap)
    } else if (has_conflict_topics || has_cyber_sources || is_natural_disaster)
        && source_diversity >= severity_config.medium_min_sources
    {
        // Corroborated situation (2+ sources) with meaningful topic signal
        Severity::Medium
    } else if source_diversity >= severity_config.critical_min_sources && effective_event_count >= severity_config.medium_min_events {
        // Any situation with 3+ independent sources is noteworthy
        Severity::Medium
    } else {
        Severity::Low
    };

    cluster.severity = computed;

    // Children should never be CRITICAL — only top-level situations warrant
    // the highest severity. Clusters that were previously parents retain
    // inflated event_count/source_types from absorbed children, which can
    // cause false CRITICAL after reparenting.
    if cluster.parent_id.is_some() && cluster.severity == Severity::Critical {
        cluster.severity = Severity::High;
    }

    // Cap severity for stale clusters
    if let Some((most_recent, _)) = cluster.event_ids.last() {
        if (now - *most_recent).num_hours() > 48 {
            cluster.severity = cluster.severity.min(Severity::Medium);
        }
    }

    cluster.certainty = compute_certainty_with_config(cluster, certainty_config);

    cluster.severity != old_severity
}

/// Compute a certainty score (0.0-1.0) for a situation cluster using smooth
/// sigmoid curves instead of stepped thresholds.
pub(crate) fn compute_certainty_with_config(cluster: &SituationCluster, c: &sr_config::CertaintyConfig) -> f32 {
    let sources = cluster.source_types.len() as f32;
    let events = cluster.event_count as f32;
    let entities = cluster.entities.len() as f32;

    let source_score = c.source_max / (1.0 + (-c.source_steepness * (sources - c.source_midpoint)).exp());
    let event_score = c.event_max / (1.0 + (-c.event_steepness * (events - c.event_midpoint)).exp());
    let entity_score = c.entity_max * (1.0 - (-c.entity_rate * entities).exp());
    let ai_bonus = if cluster.has_ai_title { c.ai_title_bonus } else { 0.0 };

    (c.base + source_score + event_score + entity_score + ai_bonus).min(1.0)
}

// ---------------------------------------------------------------------------
// SituationGraph lifecycle methods
// ---------------------------------------------------------------------------

impl SituationGraph {
    /// Remove clusters whose `last_updated` is older than `max_age`, and clean
    /// up the inverted indices.
    pub fn prune_stale(&mut self, max_age: std::time::Duration) {
        self.prune_stale_with_cache(max_age, None);
    }

    /// Remove stale clusters, optionally cleaning embedding centroids.
    pub fn prune_stale_with_cache(&mut self, max_age: std::time::Duration, embedding_cache: Option<&mut EmbeddingCache>) {
        let now = self.now();
        let normal_cutoff = now
            - chrono::Duration::from_std(max_age).unwrap_or_else(|_| chrono::Duration::hours(24));
        let fast_cutoff = now
            - chrono::Duration::from_std(max_age / 4).unwrap_or_else(|_| chrono::Duration::hours(6));

        // Pre-pass: which clusters are parents of non-resolved children?
        let parents_with_live_children: std::collections::HashSet<Uuid> = self
            .clusters
            .values()
            .filter(|c| {
                c.parent_id.is_some()
                    && !matches!(c.phase, SituationPhase::Resolved | SituationPhase::Historical)
            })
            .filter_map(|c| c.parent_id)
            .collect();

        let stale_ids: Vec<Uuid> = self
            .clusters
            .iter()
            .filter(|(id, c)| {
                // Never prune Declining clusters — they need to reach their resolve threshold.
                // Once resolved, they'll be pruned on the next pass.
                if c.phase == SituationPhase::Declining {
                    return false;
                }
                // Never prune parents whose children are still alive
                if parents_with_live_children.contains(id) {
                    return false;
                }
                let is_pure_telemetry = c.signal_event_count == 0 && c.event_count >= 20;
                let cutoff = if is_pure_telemetry { fast_cutoff } else { normal_cutoff };
                c.last_updated < cutoff
            })
            .map(|(&id, _)| id)
            .collect();

        for id in &stale_ids {
            if let Some(cluster) = self.clusters.remove(id) {
                for e in &cluster.entities {
                    if let Some(set) = self.entity_index.get_mut(e) {
                        set.remove(id);
                        if set.is_empty() {
                            self.entity_index.remove(e);
                        }
                    }
                }
                for t in &cluster.topics {
                    if let Some(set) = self.topic_index.get_mut(t) {
                        set.remove(id);
                        if set.is_empty() {
                            self.topic_index.remove(t);
                        }
                    }
                }
            }
        }

        // Cascade: clear parent_id on children whose parent was just pruned
        let stale_set: std::collections::HashSet<Uuid> = stale_ids.iter().copied().collect();
        for cluster in self.clusters.values_mut() {
            if let Some(pid) = cluster.parent_id {
                if stale_set.contains(&pid) {
                    cluster.parent_id = None;
                }
            }
        }

        if let Some(cache) = embedding_cache {
            for id in &stale_ids {
                cache.remove_centroid(id);
            }
        }
    }

    /// Evaluate phase transitions for all clusters. Call periodically (e.g. every 30s).
    pub fn evaluate_phases(&mut self) -> (Vec<(Uuid, PhaseTransition)>, Vec<Uuid>) {
        let now = self.now();
        let mut transitions = Vec::new();
        let mut severity_escalated = Vec::new();

        // Track pre-recompute severity for parent clusters to avoid oscillation log spam
        let parent_set: HashSet<Uuid> = {
            let mut parents = HashSet::new();
            for c in self.clusters.values() {
                if let Some(pid) = c.parent_id {
                    parents.insert(pid);
                }
            }
            parents
        };
        let pre_recompute_severity: std::collections::HashMap<Uuid, Severity> = parent_set.iter()
            .filter_map(|pid| self.clusters.get(pid).map(|c| (*pid, c.severity)))
            .collect();

        let cluster_ids: Vec<Uuid> = self.clusters.keys().copied().collect();
        for cid in cluster_ids {
            let cluster = match self.clusters.get(&cid) {
                Some(c) => c,
                None => continue,
            };

            // Staleness check for news-only clusters
            let is_news_only = cluster.source_types.iter().all(|s|
                matches!(s, SourceType::Gdelt | SourceType::RssNews | SourceType::GdeltGeo)
            );
            let age_hours = (now - cluster.first_seen).num_hours();
            if is_news_only && age_hours > 72 && cluster.severity <= Severity::Medium
                && cluster.phase != SituationPhase::Declining && cluster.phase != SituationPhase::Resolved && cluster.phase != SituationPhase::Historical
            {
                    let transition = PhaseTransition {
                        from_phase: cluster.phase,
                        to_phase: SituationPhase::Declining,
                        trigger_reason: "Stale news cluster (>72h, no high-severity events)".to_string(),
                        metrics_snapshot: serde_json::json!({"age_hours": age_hours}),
                        transitioned_at: now,
                    };
                    let cluster = self.clusters.get_mut(&cid).unwrap();
                    cluster.phase = SituationPhase::Declining;
                    cluster.phase_changed_at = now;
                    cluster.phase_transitions.push(transition.clone());
                    if cluster.phase_transitions.len() > 20 {
                        cluster.phase_transitions.drain(..cluster.phase_transitions.len() - 20);
                    }
                    transitions.push((cid, transition));
                    continue;
            }

            // Compute metrics
            let hours_since_last = (now - cluster.last_updated).num_minutes().max(0) as f64 / 60.0;

            let five_min_ago = now - chrono::Duration::minutes(5);
            let thirty_min_ago = now - chrono::Duration::minutes(30);
            let velocity_5m = cluster.event_ids.iter().filter(|(t, _)| *t >= five_min_ago).count();
            let velocity_30m = cluster.event_ids.iter().filter(|(t, _)| *t >= thirty_min_ago).count();

            let current_rate = velocity_5m as f64;
            let mins_since_peak = (now - cluster.peak_rate_at).num_minutes().max(0) as f64;
            let decay = 0.5_f64.powf(mins_since_peak / 30.0);
            let decayed_peak = cluster.peak_event_rate * decay;
            let effective_peak = decayed_peak.max(current_rate);
            let peak_rate = effective_peak;

            let metrics = PhaseMetrics {
                event_velocity_5m: velocity_5m,
                event_velocity_30m: velocity_30m,
                peak_rate,
                current_rate,
                source_diversity: effective_source_diversity(&cluster.source_types),
                max_severity_rank: cluster.severity.rank(),
                hours_since_last_event: hours_since_last,
                event_count: cluster.event_count,
            };

            let gap_tolerance = compute_gap_tolerance(cluster, &self.config.phases, now);

            // Track whether the phase transition was blocked by dwell time.
            // Severity recompute must ALWAYS run regardless of phase transition outcome.
            let mut phase_blocked = false;

            if let Some((new_phase, reason)) = evaluate_phase_transition(cluster.phase, &metrics, gap_tolerance, &self.config.phases) {
                let is_declining = matches!(
                    new_phase,
                    SituationPhase::Declining | SituationPhase::Resolved | SituationPhase::Historical
                );

                if is_declining {
                    if cluster.severity == Severity::Critical
                        && matches!(new_phase, SituationPhase::Declining | SituationPhase::Resolved | SituationPhase::Historical)
                        && cluster.phase == SituationPhase::Active
                    {
                        let hours_in_active = (now - cluster.phase_changed_at).num_minutes().max(0) as f64 / 60.0;
                        if hours_in_active < 24.0 {
                            debug!(
                                cluster_id = %cid,
                                title = %cluster.title,
                                hours_in_active = format!("{:.1}", hours_in_active),
                                "Phase transition blocked: Critical needs 24h dwell in Active"
                            );
                            phase_blocked = true;
                        }
                    }
                    if !phase_blocked && cluster.severity == Severity::High
                        && matches!(new_phase, SituationPhase::Declining | SituationPhase::Resolved | SituationPhase::Historical)
                        && cluster.phase == SituationPhase::Active
                    {
                        let hours_in_active = (now - cluster.phase_changed_at).num_minutes().max(0) as f64 / 60.0;
                        if hours_in_active < 12.0 {
                            debug!(
                                cluster_id = %cid,
                                title = %cluster.title,
                                hours_in_active = format!("{:.1}", hours_in_active),
                                "Phase transition blocked: High needs 12h dwell in Active"
                            );
                            phase_blocked = true;
                        }
                    }
                }

                if !phase_blocked && cluster.phase == SituationPhase::Active
                    && new_phase == SituationPhase::Declining
                {
                    let hours_in_active = (now - cluster.phase_changed_at).num_minutes().max(0) as f64 / 60.0;
                    if hours_in_active < 6.0 {
                        debug!(
                            cluster_id = %cid,
                            title = %cluster.title,
                            hours_in_active = format!("{:.1}", hours_in_active),
                            "Phase transition blocked: minimum 6h dwell time in Active not met"
                        );
                        phase_blocked = true;
                    }

                    if !phase_blocked {
                        let four_hours_ago = now - chrono::Duration::hours(4);
                        let has_recent_events = cluster.event_ids.iter().any(|(t, _)| *t >= four_hours_ago);
                        if has_recent_events {
                            debug!(
                                cluster_id = %cid,
                                title = %cluster.title,
                                "Phase transition blocked: recent events within last 4h prevent declining"
                            );
                            phase_blocked = true;
                        }
                    }
                }

                if phase_blocked {
                    // Update peak rate even when phase is blocked
                    if let Some(c) = self.clusters.get_mut(&cid) {
                        c.peak_event_rate = peak_rate;
                        if current_rate >= c.peak_event_rate * decay { c.peak_rate_at = now; }
                    }
                } else {
                    let transition = PhaseTransition {
                        from_phase: cluster.phase,
                        to_phase: new_phase,
                        trigger_reason: reason,
                        metrics_snapshot: metrics.to_json(),
                        transitioned_at: now,
                    };

                    let cluster = self.clusters.get_mut(&cid).unwrap();
                    cluster.phase = new_phase;
                    cluster.phase_changed_at = now;
                    cluster.peak_event_rate = peak_rate;
                    if current_rate >= cluster.peak_event_rate * decay {
                        cluster.peak_rate_at = now;
                    }
                    cluster.phase_transitions.push(transition.clone());
                    if cluster.phase_transitions.len() > 20 {
                        cluster.phase_transitions.drain(..cluster.phase_transitions.len() - 20);
                    }

                    transitions.push((cid, transition));
                }
            } else {
                if let Some(cluster) = self.clusters.get_mut(&cid) {
                    cluster.peak_event_rate = peak_rate;
                    if current_rate >= cluster.peak_event_rate * decay {
                        cluster.peak_rate_at = now;
                    }
                }
            }

            // Recompute cluster severity — runs unconditionally, even when phase transition
            // is blocked by dwell time. Previously, dwell-time `continue` statements skipped
            // this, causing stale HIGH severity to persist indefinitely.
            if let Some(cluster) = self.clusters.get_mut(&cid) {
                let old_sev = cluster.severity;
                if recompute_cluster_severity(cluster, &self.config.severity, &self.config.certainty, now) {
                    // Only log for non-parent clusters; parent severity is logged
                    // after propagation to avoid oscillation spam.
                    if !parent_set.contains(&cid) {
                        info!(
                            cluster_id = %cid,
                            title = %cluster.title,
                            from = %old_sev,
                            to = %cluster.severity,
                            "Cluster severity changed"
                        );
                    }
                    if cluster.severity > old_sev {
                        severity_escalated.push(cid);
                    }
                }
            }
        }

        // Propagate severity from children to parents using proportional threshold.
        // A severity level propagates only when >= threshold fraction of substantial
        // children are at or above that level.
        // IMPORTANT: never lower below the parent's own direct-computation result,
        // otherwise child noise (many low-severity children) can override genuine
        // parent-level signals (e.g., active multi-source conflict).
        let min_events_for_propagation = self.config.quality.min_events_standalone;
        let threshold = self.config.sweep.severity_propagation_threshold;
        let parent_ids: Vec<Uuid> = self.clusters.values()
            .filter(|c| c.parent_id.is_none())
            .map(|c| c.id)
            .collect();
        for pid in parent_ids {
            let child_severities: Vec<Severity> = self.clusters.values()
                .filter(|c| c.parent_id == Some(pid))
                .filter(|c| c.event_count >= min_events_for_propagation)
                .map(|c| c.severity)
                .collect();
            if child_severities.is_empty() {
                continue;
            }
            let total = child_severities.len() as f32;
            // Find the highest severity where enough children agree.
            let candidate = [Severity::Critical, Severity::High, Severity::Medium, Severity::Low, Severity::Info]
                .iter()
                .find(|&&sev| {
                    let at_or_above = child_severities.iter().filter(|s| s.rank() >= sev.rank()).count() as f32;
                    at_or_above / total >= threshold
                })
                .copied()
                .unwrap_or(Severity::Info);
            if let Some(parent) = self.clusters.get_mut(&pid) {
                let _pre_propagation = parent.severity;
                if candidate > parent.severity {
                    parent.severity = candidate;
                }
                // Only log if the final severity differs from what was recorded
                // before recompute (stored in pre_recompute_severity map).
                // This avoids oscillation log spam where recompute→medium then
                // propagation→critical on every cycle.
                if let Some(&pre_recompute) = pre_recompute_severity.get(&pid) {
                    if parent.severity != pre_recompute {
                        info!(
                            cluster_id = %pid,
                            title = %parent.title,
                            from = %pre_recompute,
                            to = %parent.severity,
                            "Parent severity changed (recompute + propagation)"
                        );
                    }
                }
            }
        }

        (transitions, severity_escalated)
    }

    /// Run periodic sweep passes over all clusters.
    pub fn sweep(&mut self, embedding_cache: Option<&EmbeddingCache>) {
        let sweep_cfg = &self.config.sweep;
        let mut pruned_topics = 0usize;
        let mut pruned_entities = 0usize;
        let mut shed_events = 0usize;
        let mut removed_orphans = 0usize;

        // --- Pass 1: Prune metadata (entities/topics exceeding caps) ---
        let cluster_ids: Vec<Uuid> = self.clusters.keys().copied().collect();
        for cid in &cluster_ids {
            if let Some(cluster) = self.clusters.get_mut(cid) {
                if cluster.topics.len() > sweep_cfg.topic_max_after_prune {
                    let mut scored: Vec<(String, f64)> = cluster.topics.iter()
                        .map(|t| (t.clone(), self.topic_idf.score(t)))
                        .collect();
                    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let keep: HashSet<String> = scored.iter()
                        .take(sweep_cfg.topic_max_after_prune)
                        .map(|(t, _)| t.clone())
                        .collect();
                    let to_remove: Vec<String> = cluster.topics.difference(&keep).cloned().collect();
                    for t in &to_remove {
                        cluster.topics.remove(t);
                        if let Some(set) = self.topic_index.get_mut(t) {
                            set.remove(cid);
                        }
                    }
                    pruned_topics += to_remove.len();
                }

                if cluster.entities.len() > sweep_cfg.entity_max_after_prune {
                    let mut scored: Vec<(String, f64)> = cluster.entities.iter()
                        .map(|e| (e.clone(), self.entity_idf.score(e)))
                        .collect();
                    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let keep: HashSet<String> = scored.iter()
                        .take(sweep_cfg.entity_max_after_prune)
                        .map(|(e, _)| e.clone())
                        .collect();
                    let to_remove: Vec<String> = cluster.entities.difference(&keep).cloned().collect();
                    for e in &to_remove {
                        cluster.entities.remove(e);
                        if let Some(set) = self.entity_index.get_mut(e) {
                            set.remove(cid);
                        }
                    }
                    pruned_entities += to_remove.len();
                }
            }
        }

        // --- Pass 2: Shed oversized clusters ---
        for cid in &cluster_ids {
            if let Some(cluster) = self.clusters.get_mut(cid) {
                if cluster.event_ids.len() > sweep_cfg.shed_above_events {
                    let drain_count = cluster.event_ids.len() - sweep_cfg.shed_target_events;
                    cluster.event_ids.drain(..drain_count);
                    // Keep event_count in sync with event_ids for velocity calculations,
                    // but total_events_ingested preserves the true historical count.
                    cluster.event_count = cluster.event_ids.len();
                    shed_events += drain_count;
                }
            }
        }

        // --- Pass 3: Orphan cleanup ---
        let now = self.now();
        let orphan_age = chrono::Duration::seconds(sweep_cfg.orphan_min_age_secs as i64);
        let orphan_ids: Vec<Uuid> = self.clusters.values()
            .filter(|c| {
                c.parent_id.is_some()
                    && c.event_count < sweep_cfg.orphan_min_events
                    && (now - c.first_seen) > orphan_age
            })
            .map(|c| c.id)
            .collect();

        for oid in &orphan_ids {
            if let Some(removed) = self.clusters.remove(oid) {
                for e in &removed.entities {
                    if let Some(set) = self.entity_index.get_mut(e) {
                        set.remove(oid);
                    }
                }
                for t in &removed.topics {
                    if let Some(set) = self.topic_index.get_mut(t) {
                        set.remove(oid);
                    }
                }
                removed_orphans += 1;
            }
        }
        // --- Pass 3b: Enforce max_children_per_parent cap ---
        // Orphan the smallest children from parents that exceed the cap.
        let max_children = self.config.cluster_caps.max_children_per_parent;
        let mut children_per_parent: std::collections::HashMap<Uuid, Vec<(Uuid, usize)>> = std::collections::HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                children_per_parent.entry(pid).or_default().push((c.id, c.event_count));
            }
        }
        let mut cap_orphaned = 0usize;
        for (_pid, mut kids) in children_per_parent {
            if kids.len() <= max_children {
                continue;
            }
            // Keep the largest children, orphan the rest
            kids.sort_by(|a, b| b.1.cmp(&a.1));
            for &(kid_id, _) in kids.iter().skip(max_children) {
                if let Some(kid) = self.clusters.get_mut(&kid_id) {
                    kid.parent_id = None;
                    cap_orphaned += 1;
                }
            }
        }
        if cap_orphaned > 0 {
            info!(cap_orphaned, max_children, "Sweep: enforced max_children_per_parent cap");
        }

        // --- Pass 3c: Orphan children with no topical connection to parent ---
        // Checks title word overlap, shared entities, and shared semantic topics.
        // Source-type topics (gdacs, firms, copernicus, etc.) are excluded from
        // overlap calculation since they indicate data source, not situation topic.
        let mut topical_orphaned = 0usize;
        let source_topics: HashSet<&str> = [
            "gdacs", "firms", "copernicus", "usgs", "acled",
            "gdelt", "green", "orange", "red", "closed",
        ].into_iter().collect();
        {
            let child_ids: Vec<(Uuid, Uuid)> = self.clusters.values()
                .filter_map(|c| c.parent_id.map(|pid| (c.id, pid)))
                .collect();
            for (child_id, parent_id) in child_ids {
                let (child_title, child_entities, child_topics) = match self.clusters.get(&child_id) {
                    Some(c) => (c.title.clone(), c.entities.clone(), c.topics.clone()),
                    None => continue,
                };
                let (parent_title, parent_entities, parent_topics) = match self.clusters.get(&parent_id) {
                    Some(c) => (c.title.clone(), c.entities.clone(), c.topics.clone()),
                    None => {
                        // Parent gone — orphan the child
                        if let Some(c) = self.clusters.get_mut(&child_id) { c.parent_id = None; }
                        topical_orphaned += 1;
                        continue;
                    }
                };

                let shared_entities = child_entities.intersection(&parent_entities).count();
                // Only count semantic topics (exclude source-type tags)
                let semantic_child: HashSet<&String> = child_topics.iter()
                    .filter(|t| !source_topics.contains(t.as_str()))
                    .collect();
                let semantic_parent: HashSet<&String> = parent_topics.iter()
                    .filter(|t| !source_topics.contains(t.as_str()))
                    .collect();
                let _shared_semantic_topics = semantic_child.intersection(&semantic_parent).count();

                // Title word overlap (Jaccard similarity).
                // Exclude generic category words (e.g., "wildfires", "earthquake")
                // that create false overlap between geographically unrelated situations.
                let generic = &super::scoring::GENERIC_TITLE_WORDS;
                let words_a: HashSet<String> = parent_title.to_lowercase()
                    .split_whitespace().filter(|w| w.len() > 2)
                    .filter(|w| !generic.contains(w))
                    .map(|w| w.to_string()).collect();
                let words_b: HashSet<String> = child_title.to_lowercase()
                    .split_whitespace().filter(|w| w.len() > 2)
                    .filter(|w| !generic.contains(w))
                    .map(|w| w.to_string()).collect();
                let title_overlap = if words_a.is_empty() || words_b.is_empty() {
                    0.0
                } else {
                    let inter = words_a.intersection(&words_b).count();
                    let union = words_a.union(&words_b).count();
                    if union == 0 { 0.0 } else { inter as f64 / union as f64 }
                };

                // Orphan if: no meaningful title overlap AND no shared entities.
                // Generic words like "Wildfires" are excluded so
                // "Central Africa Wildfires" won't keep "Thailand Wildfires" as child.
                if shared_entities == 0 && title_overlap < 0.15 {
                    if let Some(c) = self.clusters.get_mut(&child_id) {
                        debug!(
                            child_id = %child_id,
                            parent_id = %parent_id,
                            child_title = %child_title,
                            parent_title = %parent_title,
                            "Orphaning child: no topical connection to parent"
                        );
                        c.parent_id = None;
                        topical_orphaned += 1;
                        // Add merge rejection to prevent re-merge
                        let key = if parent_id < child_id { (parent_id, child_id) } else { (child_id, parent_id) };
                        self.merge_rejections.insert(key, self.now());
                    }
                }
            }
        }
        if topical_orphaned > 0 {
            info!(topical_orphaned, "Sweep: orphaned children with no topical connection");
        }

        // --- Pass 4: Coherence check (when embeddings available) ---
        let mut coherence_splits = 0usize;
        if let Some(cache) = embedding_cache {
            let mut to_split: Vec<Uuid> = Vec::new();

            for cid in &cluster_ids {
                if let Some(cluster) = self.clusters.get(cid) {
                    if cluster.event_count < sweep_cfg.coherence_min_events {
                        continue;
                    }
                    let recent_keys: Vec<String> = cluster.event_ids.iter()
                        .rev()
                        .take(sweep_cfg.coherence_sample_size)
                        .map(|(_, ref_id)| ref_id.clone())
                        .collect();

                    let embeddings: Vec<&[f32]> = recent_keys.iter()
                        .filter_map(|k| cache.get(k))
                        .map(|v| v.as_slice())
                        .collect();

                    if embeddings.len() < 4 {
                        continue;
                    }

                    let mut total_sim = 0.0f32;
                    let mut count = 0usize;
                    for i in 0..embeddings.len() {
                        for j in (i + 1)..embeddings.len() {
                            total_sim += EmbeddingCache::cosine_similarity(
                                embeddings[i], embeddings[j],
                            );
                            count += 1;
                        }
                    }
                    let mean_sim = if count > 0 { total_sim / count as f32 } else { 1.0 };

                    if (mean_sim as f64) < sweep_cfg.coherence_min {
                        if sweep_cfg.coherence_auto_split {
                            to_split.push(*cid);
                        }
                        tracing::info!(
                            cluster_id = %cid,
                            mean_similarity = %format!("{:.3}", mean_sim),
                            event_count = cluster.event_count,
                            title = %cluster.title,
                            auto_split = sweep_cfg.coherence_auto_split,
                            "Low coherence detected — cluster may need splitting"
                        );
                    }
                }
            }

            let min_group = self.config.sweep.coherence_split_min_group;
            for cid in to_split {
                if let Some(new_id) = self.split_by_coherence(cid, cache, min_group) {
                    info!(%cid, %new_id, "Split incoherent cluster via k-means");
                    coherence_splits += 1;
                }
            }
        }

        // --- Pass 5: Topic-diversity split trigger ---
        // Clusters with too many distinct topics are likely conflating unrelated events.
        // Force a coherence split if embeddings are available.
        let mut topic_diversity_splits = 0usize;
        if let Some(cache) = embedding_cache {
            let topic_threshold = self.config.sweep.topic_diversity_split_threshold;
            let min_group_td = self.config.sweep.coherence_split_min_group;
            let coherence_min_events_td = self.config.sweep.coherence_min_events;
            let mut to_split_td: Vec<Uuid> = Vec::new();

            for cid in &cluster_ids {
                if let Some(cluster) = self.clusters.get(cid) {
                    if cluster.topics.len() >= topic_threshold
                        && cluster.event_count >= coherence_min_events_td
                    {
                        tracing::info!(
                            cluster_id = %cid,
                            topics = cluster.topics.len(),
                            title = %cluster.title,
                            "Topic diversity exceeds threshold — triggering split"
                        );
                        to_split_td.push(*cid);
                    }
                }
            }

            for cid in to_split_td {
                if let Some(new_id) = self.split_by_coherence(cid, cache, min_group_td) {
                    info!(%cid, %new_id, "Split high-topic-diversity cluster via k-means");
                    topic_diversity_splits += 1;
                }
            }
        }

        if pruned_topics > 0 || pruned_entities > 0 || shed_events > 0 || removed_orphans > 0 || coherence_splits > 0 || topic_diversity_splits > 0 {
            tracing::info!(
                pruned_topics,
                pruned_entities,
                shed_events,
                removed_orphans,
                coherence_splits,
                topic_diversity_splits,
                "Sweep completed"
            );
        }
    }
}
