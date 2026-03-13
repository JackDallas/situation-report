pub mod scoring;
pub mod lifecycle;
pub mod dto;
pub mod merge;

// Re-export public types from submodules so callers don't need to know the internal structure.
pub use dto::{SituationClusterDTO, ClusterGapAnalysis};
pub use lifecycle::{PhaseTransition, SituationPhase};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_config::PipelineConfig;
use sr_types::{EventType, Severity, SourceType};
use uuid::Uuid;

use sr_embeddings::cache::embed_key;
use sr_embeddings::EmbeddingCache;
use sr_intel::search::SearchHistory;
use sr_sources::InsertableEvent;
use tracing::{debug, info, warn};

use scoring::{
    BurstDetector, StreamingIdf, distance_km, effective_source_diversity,
    extract_entities, extract_topics, is_high_signal_event,
    normalize_region, region_code_to_name, title_case,
};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// An event buffered because it didn't match any existing cluster.
/// Held temporarily to see if a matching cluster appears or if other
/// pending events share enough signal to form a cluster together.
struct PendingEvent {
    event: InsertableEvent,
    received_at: DateTime<Utc>,
    entities: HashSet<String>,
    topics: HashSet<String>,
}

pub struct SituationGraph {
    entity_index: HashMap<String, HashSet<Uuid>>,
    topic_index: HashMap<String, HashSet<Uuid>>,
    clusters: HashMap<Uuid, SituationCluster>,
    /// Streaming IDF for entity terms — rare entities score higher.
    entity_idf: StreamingIdf,
    /// Streaming IDF for topic terms — rare topics score higher.
    topic_idf: StreamingIdf,
    /// Burst detector for topics — detects sudden rate spikes.
    burst_detector: BurstDetector,
    /// All tunable configuration parameters.
    pub config: Arc<PipelineConfig>,
    /// Pairs rejected by LLM merge audit — prevents re-merging the same clusters.
    /// Key is (smaller_id, larger_id) normalized, value is rejection timestamp.
    /// Entries expire after 6 hours to allow reconsideration as situations evolve.
    merge_rejections: HashMap<(Uuid, Uuid), DateTime<Utc>>,
    /// Noise buffer: events that didn't match any cluster on arrival.
    /// Held for up to `noise_buffer_secs` to see if a matching cluster appears
    /// or if other pending events share enough signal to form a cluster together.
    pending_buffer: Vec<PendingEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationCluster {
    pub id: Uuid,
    pub title: String,
    pub entities: HashSet<String>,
    pub topics: HashSet<String>,
    /// (event_time, source_id-or-event_type) tuples for every ingested event.
    pub event_ids: Vec<(DateTime<Utc>, String)>,
    pub region_codes: HashSet<String>,
    pub severity: Severity,
    pub first_seen: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub centroid: Option<(f64, f64)>,
    /// Convenience counter kept in sync with `event_ids.len()`.
    pub event_count: usize,
    /// Count of high-signal events (conflict, news, geo) — used to gate regen triggers.
    /// Routine telemetry (notam, thermal, bgp) doesn't increment this.
    pub signal_event_count: usize,
    pub source_types: HashSet<SourceType>,
    pub parent_id: Option<Uuid>,
    /// Recent event titles (for AI title generation context). Capped at 10.
    pub event_titles: Vec<String>,
    /// Whether this cluster has received an AI-generated title.
    pub has_ai_title: bool,
    /// Signal event count when the title was last generated (for periodic re-evaluation).
    pub title_signal_count_at_gen: usize,
    /// When the AI title was last generated (for rate-limiting regen).
    #[serde(default = "Utc::now")]
    pub last_title_gen: DateTime<Utc>,
    /// Supplementary web search data for this cluster.
    pub supplementary: Option<sr_intel::search::SupplementaryData>,
    /// When this cluster was last searched for supplementary data.
    pub last_searched: Option<DateTime<Utc>>,
    /// Per-gap-type search history for intelligent search selection.
    pub search_history: SearchHistory,
    /// Lifecycle phase of this situation.
    pub phase: SituationPhase,
    /// When the phase last changed.
    pub phase_changed_at: DateTime<Utc>,
    /// Peak 5-minute event rate observed.
    pub peak_event_rate: f64,
    /// When the peak event rate was observed.
    pub peak_rate_at: DateTime<Utc>,
    /// History of phase transitions.
    pub phase_transitions: Vec<PhaseTransition>,
    /// Certainty score (0.0–1.0) based on source diversity, event count, entities, and enrichment.
    #[serde(default)]
    pub certainty: f32,
    /// Composite anomaly score (0.0–1.0) based on burst detection across topics.
    #[serde(default)]
    pub anomaly_score: f64,
}

// ---------------------------------------------------------------------------
// SituationGraph implementation
// ---------------------------------------------------------------------------

impl SituationGraph {
    pub fn new(config: Arc<PipelineConfig>) -> Self {
        Self {
            entity_index: HashMap::new(),
            topic_index: HashMap::new(),
            clusters: HashMap::new(),
            entity_idf: StreamingIdf::new(&config.idf),
            topic_idf: StreamingIdf::new(&config.idf),
            burst_detector: BurstDetector::new(&config.burst),
            config,
            merge_rejections: HashMap::new(),
            pending_buffer: Vec::new(),
        }
    }

    /// Score a candidate cluster for an event. Returns None if the cluster
    /// is blocked (e.g. over size limit). Extracted for testability.
    fn score_candidate(
        &self,
        cluster: &SituationCluster,
        event: &InsertableEvent,
        entities: &HashSet<String>,
        topics: &HashSet<String>,
        embedding_cache: Option<&EmbeddingCache>,
    ) -> Option<i32> {
        // Tighter cap for single-source leaf clusters: they're likely sensor noise,
        // not real intelligence situations. Cap at 50 events to prevent FIRMS/GeoConfirmed
        // blobs from growing to 300+ events.
        if cluster.parent_id.is_none()
            && cluster.source_types.len() == 1
            && cluster.event_count >= 50
        {
            let is_parent = self.clusters.values().any(|c| c.parent_id == Some(cluster.id));
            if !is_parent {
                return None;
            }
        }

        // Hard cap: leaf clusters cannot exceed configured limit (prevents mega-clusters
        // from GDELT bulk ingestion). Parent-level situations are exempt.
        if cluster.parent_id.is_none() && cluster.event_count >= self.config.cluster_caps.leaf_cluster_hard_cap {
            // Check if this cluster has children (i.e. is a parent aggregator)
            let is_parent = self.clusters.values().any(|c| c.parent_id == Some(cluster.id));
            if !is_parent {
                warn!(
                    cluster_id = %cluster.id,
                    title = %cluster.title,
                    event_count = cluster.event_count,
                    "Event rejected: leaf cluster at hard cap of {} events",
                    self.config.cluster_caps.leaf_cluster_hard_cap,
                );
                return None;
            }
        }

        // Smooth size penalty — may block merge entirely
        let penalty = self.config.size_penalty(cluster.event_count)?;
        let mut score: i32 = penalty;

        // Entity matching with IDF weighting (rare entities score higher)
        let scoring = &self.config.scoring;
        let mut entity_score: i32 = 0;
        for e in entities {
            if cluster.entities.contains(e) {
                let idf = self.entity_idf.score(e).clamp(1.0, 5.0);
                entity_score += idf.round() as i32;
            }
        }
        score += entity_score.min(scoring.entity_score_cap);

        // Topic matching with IDF weighting — cap total contribution to prevent
        // large clusters from becoming "topic magnets" that absorb everything
        let mut topic_score: i32 = 0;
        for t in topics {
            if cluster.topics.contains(t) {
                let idf = self.topic_idf.score(t).clamp(1.0, 7.0);
                topic_score += idf.round() as i32;

                // Graduated burst bonus based on anomaly score
                let anomaly = self.burst_detector.anomaly_score(t);
                topic_score += (anomaly * scoring.burst_bonus_max).round() as i32;
            }
        }
        score += topic_score.min(scoring.topic_score_cap);

        // Region match (with normalization for ME/middle-east etc.)
        if let Some(ref rc) = event.region_code {
            let norm = normalize_region(rc);
            if cluster.region_codes.contains(rc)
                || cluster.region_codes.iter().any(|cr| normalize_region(cr) == norm)
            {
                score += scoring.region_bonus;
            }
        }

        // Geographic proximity with event-type-specific radii (graduated)
        if let (Some(lat), Some(lon), Some((clat, clon))) =
            (event.latitude, event.longitude, cluster.centroid)
        {
            let dist = distance_km(lat, lon, clat, clon);
            let radius = self.config.geo_radius_km(event.event_type.as_str());
            if dist <= radius * 0.5 {
                score += scoring.geo_inner_bonus;
            } else if dist <= radius {
                score += scoring.geo_outer_bonus;
            }
        }

        // Penalize single-source clusters (flight sources count as one source)
        let eff_diversity = effective_source_diversity(&cluster.source_types);
        if eff_diversity == 1 {
            if cluster.source_types.contains(&event.source_type)
                || (event.source_type.is_flight_source()
                    && cluster.source_types.iter().any(|s| s.is_flight_source()))
            {
                score += scoring.single_source_penalty;
            }
        }

        // Vector similarity scoring (graceful: skipped if embeddings not ready)
        if let Some(cache) = embedding_cache {
            let event_key = embed_key(event);
            if let (Some(event_vec), Some(centroid)) = (
                cache.get(&event_key),
                cache.get_cluster_centroid(&cluster.id),
            ) {
                let sim = EmbeddingCache::cosine_similarity(event_vec, centroid);
                let sim_f64 = sim as f64;
                if sim_f64 < scoring.vector_hard_gate {
                    if scoring.vector_soft_gate {
                        // Soft penalty: proportional to distance below gate
                        // sim=0.35 (gate=0.40) → -0.5, sim=0.20 → -2.0, sim=0.0 → -4.0
                        let deficit = scoring.vector_hard_gate - sim_f64;
                        score -= (deficit * scoring.vector_soft_gate_multiplier).round() as i32;
                    } else {
                        // Hard gate: reject semantically unrelated events
                        return Some(penalty);
                    }
                }
                // Apply tiered vector bonuses (highest threshold first)
                for &(threshold, bonus) in &scoring.vector_tiers {
                    if sim >= threshold as f32 {
                        score += bonus;
                        break;
                    }
                }
            }
        }

        // Title similarity scoring (Jaccard word overlap)
        if let Some(ref title) = event.title {
            let event_words: HashSet<String> = title
                .to_lowercase()
                .split_whitespace()
                .filter(|w| w.len() > 2)
                .map(|w| w.to_string())
                .collect();
            if !event_words.is_empty() {
                let best_jaccard = cluster
                    .event_titles
                    .iter()
                    .map(|ct| {
                        let cluster_words: HashSet<String> = ct
                            .to_lowercase()
                            .split_whitespace()
                            .filter(|w| w.len() > 2)
                            .map(|w| w.to_string())
                            .collect();
                        if cluster_words.is_empty() {
                            return 0.0;
                        }
                        let intersection = event_words.intersection(&cluster_words).count();
                        let union = event_words.union(&cluster_words).count();
                        if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
                    })
                    .fold(0.0_f64, f64::max);
                // Apply tiered Jaccard bonuses (highest threshold first)
                for &(threshold, bonus) in &scoring.title_jaccard_tiers {
                    if best_jaccard >= threshold {
                        score += bonus;
                        break;
                    }
                }
            }
        }

        // Temporal decay: reduce score for stale clusters
        let (half_life, offset) = self.config.decay_params(event.event_type.as_str());
        let dt_hours = (Utc::now() - cluster.last_updated).num_minutes().max(0) as f64 / 60.0;
        let effective_dt = (dt_hours - offset).max(0.0);
        let decay = (-0.693 / half_life * effective_dt).exp();
        score = (score as f64 * decay).round() as i32;

        Some(score)
    }

    /// Main entry point – ingest a single event and cluster it.
    /// When `embedding_cache` is provided, vector similarity is used as an
    /// additional scoring signal for cluster matching.
    pub fn ingest(&mut self, event: &InsertableEvent, mut embedding_cache: Option<&mut EmbeddingCache>) {
        // Skip low-relevance enriched articles (sports, entertainment, lifestyle).
        // They're still persisted to DB but won't form/join situation clusters.
        if event.event_type == EventType::NewsArticle {
            if let Some(enrichment) = event.payload.get("enrichment") {
                let relevance = enrichment
                    .get("relevance_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5);
                if relevance < 0.30 {
                    return;
                }
            }
        }

        let entities = extract_entities(event, self.config.cluster_caps.max_entities_per_event);
        let topics = extract_topics(event, self.config.cluster_caps.max_enrichment_topics);

        // Can't cluster without at least one signal.
        if entities.is_empty() && topics.is_empty() {
            return;
        }

        // Update streaming IDF and burst detector with this event's terms
        self.entity_idf.observe(&entities);
        self.topic_idf.observe(&topics);
        self.burst_detector.observe(&topics);

        // Collect candidate cluster IDs from the inverted indices.
        let mut candidates: HashSet<Uuid> = HashSet::new();
        for e in &entities {
            if let Some(ids) = self.entity_index.get(e) {
                candidates.extend(ids);
            }
        }
        for t in &topics {
            if let Some(ids) = self.topic_index.get(t) {
                candidates.extend(ids);
            }
        }

        // Score each candidate using the extracted scoring method.
        let mut best: Option<(Uuid, i32)> = None;
        for &cid in &candidates {
            let cluster = match self.clusters.get(&cid) {
                Some(c) => c,
                None => continue,
            };

            let cache_ref = embedding_cache.as_deref();
            if let Some(score) = self.score_candidate(cluster, event, &entities, &topics, cache_ref) {
                if score > best.as_ref().map_or(0, |b| b.1) {
                    best = Some((cid, score));
                }
            }
        }

        // Adaptive threshold: cross-source merges more easily.
        // Flight sources (airplaneslive, adsb-fi, adsb-lol, opensky) all report the
        // same aircraft — treat them as the SAME source for diversity scoring.
        if let Some((cid, score)) = best {
            let is_cross_source = self.clusters.get(&cid).is_some_and(|c| {
                if event.source_type.is_flight_source() {
                    // Only cross-source if cluster has non-flight sources
                    c.source_types.iter().any(|s| !s.is_flight_source())
                } else {
                    !c.source_types.contains(&event.source_type)
                }
            });
            let src = event.source_type.as_str();
            let threshold = if is_cross_source {
                self.config.cross_source_threshold(src)
            } else {
                self.config.same_source_threshold(src)
            };
            if score >= threshold {
                // Update cluster centroid with this event's embedding
                if let Some(ref mut cache) = embedding_cache {
                    let event_key = embed_key(event);
                    if let Some(event_vec) = cache.get(&event_key).cloned() {
                        cache.update_centroid(cid, &event_vec);
                    }
                }
                self.merge_into_cluster(cid, event, entities, topics);
                return;
            }
        }

        // Flight position events should NEVER create new situation clusters —
        // they can only merge into existing ones. Individual callsigns are not
        // "situations" on their own; flights only become relevant when correlated
        // with conflict, news, or other intelligence signals.
        if event.event_type == EventType::FlightPosition {
            return;
        }

        // High-signal events (Critical/High severity or important category) bypass
        // the noise buffer and create clusters immediately. Low-signal events are
        // buffered to see if they can join an existing cluster or group with other
        // pending events before creating a singleton.
        let is_high_signal = event.severity >= Severity::High
            || event.event_type.is_important_category();

        if is_high_signal {
            let new_id = self.create_cluster(event, entities, topics);
            if let Some(ref mut cache) = embedding_cache {
                let event_key = embed_key(event);
                if let Some(event_vec) = cache.get(&event_key).cloned() {
                    cache.init_centroid(new_id, &event_vec);
                }
            }
        } else {
            // Buffer low-signal events — they'll be processed by flush_pending()
            let max = self.config.sweep.noise_buffer_max;
            if self.pending_buffer.len() >= max {
                // Evict the oldest entry to make room
                self.pending_buffer.remove(0);
            }
            self.pending_buffer.push(PendingEvent {
                event: event.clone(),
                received_at: Utc::now(),
                entities,
                topics,
            });
        }
    }

    /// Flush the noise buffer: re-try matching pending events against current
    /// clusters (which may have grown since the event was buffered), group
    /// unmatched pending events that share enough signal, and discard expired
    /// entries that remain unmatched.
    pub fn flush_pending(&mut self, mut embedding_cache: Option<&mut EmbeddingCache>) {
        let now = Utc::now();
        let max_age = chrono::Duration::seconds(self.config.sweep.noise_buffer_secs as i64);

        // Take the buffer out so we can iterate while mutating self.
        let pending = std::mem::take(&mut self.pending_buffer);
        let mut still_pending: Vec<PendingEvent> = Vec::new();

        // --- Pass 1: Try to match each pending event against existing clusters ---
        for pe in pending {
            // Discard expired entries
            if now - pe.received_at > max_age {
                debug!(
                    event_type = %pe.event.event_type,
                    age_secs = (now - pe.received_at).num_seconds(),
                    "Noise buffer: discarding expired pending event"
                );
                continue;
            }

            // Re-try scoring against current clusters
            let mut candidates: HashSet<Uuid> = HashSet::new();
            for e in &pe.entities {
                if let Some(ids) = self.entity_index.get(e) {
                    candidates.extend(ids);
                }
            }
            for t in &pe.topics {
                if let Some(ids) = self.topic_index.get(t) {
                    candidates.extend(ids);
                }
            }

            let mut best: Option<(Uuid, i32)> = None;
            for &cid in &candidates {
                let cluster = match self.clusters.get(&cid) {
                    Some(c) => c,
                    None => continue,
                };
                let cache_ref = embedding_cache.as_deref();
                if let Some(score) = self.score_candidate(cluster, &pe.event, &pe.entities, &pe.topics, cache_ref) {
                    if score > best.as_ref().map_or(0, |b| b.1) {
                        best = Some((cid, score));
                    }
                }
            }

            if let Some((cid, score)) = best {
                let is_cross_source = self.clusters.get(&cid).is_some_and(|c| {
                    if pe.event.source_type.is_flight_source() {
                        c.source_types.iter().any(|s| !s.is_flight_source())
                    } else {
                        !c.source_types.contains(&pe.event.source_type)
                    }
                });
                let src = pe.event.source_type.as_str();
                let threshold = if is_cross_source {
                    self.config.cross_source_threshold(src)
                } else {
                    self.config.same_source_threshold(src)
                };
                if score >= threshold {
                    if let Some(ref mut cache) = embedding_cache {
                        let event_key = embed_key(&pe.event);
                        if let Some(event_vec) = cache.get(&event_key).cloned() {
                            cache.update_centroid(cid, &event_vec);
                        }
                    }
                    self.merge_into_cluster(cid, &pe.event, pe.entities, pe.topics);
                    continue;
                }
            }

            still_pending.push(pe);
        }

        // --- Pass 2: Group unmatched pending events that share signal ---
        // Score all pairs and use a simple greedy grouping (union-find style).
        let n = still_pending.len();
        let mut group: Vec<usize> = (0..n).collect(); // each event is its own group

        // Find root of a group (path compression).
        fn find(group: &mut [usize], i: usize) -> usize {
            let mut root = i;
            while group[root] != root {
                root = group[root];
            }
            // Path compression
            let mut cur = i;
            while group[cur] != root {
                let next = group[cur];
                group[cur] = root;
                cur = next;
            }
            root
        }

        for i in 0..n {
            for j in (i + 1)..n {
                // Quick overlap check: shared entities or topics
                let shared_entities = still_pending[i].entities.intersection(&still_pending[j].entities).count();
                let shared_topics = still_pending[i].topics.intersection(&still_pending[j].topics).count();
                if shared_entities + shared_topics >= 2 {
                    let ri = find(&mut group, i);
                    let rj = find(&mut group, j);
                    if ri != rj {
                        group[rj] = ri;
                    }
                }
            }
        }

        // Collect groups of size >= 2
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let root = find(&mut group, i);
            groups.entry(root).or_default().push(i);
        }

        // Track which indices got consumed into a new cluster
        let mut consumed: HashSet<usize> = HashSet::new();

        for (_root, members) in &groups {
            if members.len() < 2 {
                continue;
            }
            // Create a cluster from the first event, merge the rest in
            let first_idx = members[0];
            let first_pe = &still_pending[first_idx];
            let new_id = self.create_cluster(
                &first_pe.event,
                first_pe.entities.clone(),
                first_pe.topics.clone(),
            );
            if let Some(ref mut cache) = embedding_cache {
                let event_key = embed_key(&first_pe.event);
                if let Some(event_vec) = cache.get(&event_key).cloned() {
                    cache.init_centroid(new_id, &event_vec);
                }
            }
            consumed.insert(first_idx);

            for &idx in members.iter().skip(1) {
                let pe = &still_pending[idx];
                if let Some(ref mut cache) = embedding_cache {
                    let event_key = embed_key(&pe.event);
                    if let Some(event_vec) = cache.get(&event_key).cloned() {
                        cache.update_centroid(new_id, &event_vec);
                    }
                }
                self.merge_into_cluster(new_id, &pe.event, pe.entities.clone(), pe.topics.clone());
                consumed.insert(idx);
            }

            debug!(
                cluster_id = %new_id,
                group_size = members.len(),
                "Noise buffer: created cluster from pending event group"
            );
        }

        // Keep unconsumed (still unmatched, not expired) events in the buffer
        let mut remaining = Vec::new();
        for (i, pe) in still_pending.into_iter().enumerate() {
            if !consumed.contains(&i) {
                remaining.push(pe);
            }
        }
        self.pending_buffer = remaining;

        if !self.pending_buffer.is_empty() {
            debug!(
                pending_count = self.pending_buffer.len(),
                "Noise buffer: events still pending after flush"
            );
        }
    }

    /// Merge an event into an existing cluster, updating indices and metadata.
    fn merge_into_cluster(
        &mut self,
        cluster_id: Uuid,
        event: &InsertableEvent,
        entities: HashSet<String>,
        topics: HashSet<String>,
    ) {
        // Count children before taking mutable borrow on the cluster
        let child_count = self.clusters.values().filter(|c| c.parent_id == Some(cluster_id)).count();

        let cluster = match self.clusters.get_mut(&cluster_id) {
            Some(c) => c,
            None => return,
        };

        // Event reference
        let event_ref = event
            .source_id
            .clone()
            .unwrap_or_else(|| event.event_type.to_string());
        cluster.event_ids.push((event.event_time, event_ref));
        // Trim to max_event_ids for memory efficiency (velocity windows only need 30min)
        let max_eids = self.config.cluster_caps.max_event_ids;
        if cluster.event_ids.len() > max_eids {
            let drain_count = cluster.event_ids.len() - max_eids;
            cluster.event_ids.drain(..drain_count);
        }
        cluster.event_count += 1;
        if is_high_signal_event(event.event_type) {
            cluster.signal_event_count += 1;
        }

        // Source type tracking
        cluster.source_types.insert(event.source_type);

        // Update centroid (running weighted average)
        if let (Some(lat), Some(lon)) = (event.latitude, event.longitude) {
            let n = (cluster.event_count - 1) as f64; // prior count
            cluster.centroid = Some(match cluster.centroid {
                Some((old_lat, old_lon)) => (
                    (old_lat * n + lat) / (n + 1.0),
                    (old_lon * n + lon) / (n + 1.0),
                ),
                None => (lat, lon),
            });
        }

        // Region (normalize to canonical hyphenated form)
        if let Some(ref rc) = event.region_code {
            let norm = normalize_region(rc);
            cluster.region_codes.insert(norm.to_string());
        }

        // Severity – only upgrade if new event is higher, but allow
        // recompute_cluster_severity() to lower it later based on recent events
        if event.severity > cluster.severity {
            cluster.severity = event.severity;
        }

        // Boost severity based on conflict fatalities (concrete evidence)
        if event.event_type == EventType::ConflictEvent {
            if let Some(fatalities) = event.payload.get("fatalities").and_then(|v| v.as_f64()) {
                if fatalities > 0.0 && cluster.severity < Severity::High {
                    cluster.severity = Severity::High;
                }
            }
        }

        // Timestamps
        if event.event_time < cluster.first_seen {
            cluster.first_seen = event.event_time;
        }
        cluster.last_updated = Utc::now();

        // Merge entities with IDF-based eviction: if at cap, evict the lowest-IDF
        // (most common) entity to make room for a rarer, more specific one.
        let max_entities = self.config.cluster_caps.max_entities;
        for e in &entities {
            if cluster.entities.contains(e) {
                continue; // already present
            }
            if cluster.entities.len() < max_entities {
                cluster.entities.insert(e.clone());
                self.entity_index.entry(e.clone()).or_default().insert(cluster_id);
            } else {
                let new_idf = self.entity_idf.score(e);
                // Find the lowest-IDF entity currently in the set
                if let Some(worst) = cluster.entities.iter()
                    .min_by(|a, b| self.entity_idf.score(a).partial_cmp(&self.entity_idf.score(b)).unwrap_or(std::cmp::Ordering::Equal))
                    .cloned()
                {
                    if new_idf > self.entity_idf.score(&worst) {
                        // Evict the common entity, insert the rarer one
                        cluster.entities.remove(&worst);
                        if let Some(set) = self.entity_index.get_mut(&worst) {
                            set.remove(&cluster_id);
                            if set.is_empty() { self.entity_index.remove(&worst); }
                        }
                        cluster.entities.insert(e.clone());
                        self.entity_index.entry(e.clone()).or_default().insert(cluster_id);
                    }
                }
            }
        }
        // Merge topics with IDF-based eviction (same pattern)
        let max_topics = self.config.cluster_caps.max_topics;
        for t in &topics {
            if cluster.topics.contains(t) {
                continue;
            }
            if cluster.topics.len() < max_topics {
                cluster.topics.insert(t.clone());
                self.topic_index.entry(t.clone()).or_default().insert(cluster_id);
            } else {
                let new_idf = self.topic_idf.score(t);
                if let Some(worst) = cluster.topics.iter()
                    .min_by(|a, b| self.topic_idf.score(a).partial_cmp(&self.topic_idf.score(b)).unwrap_or(std::cmp::Ordering::Equal))
                    .cloned()
                {
                    if new_idf > self.topic_idf.score(&worst) {
                        cluster.topics.remove(&worst);
                        if let Some(set) = self.topic_index.get_mut(&worst) {
                            set.remove(&cluster_id);
                            if set.is_empty() { self.topic_index.remove(&worst); }
                        }
                        cluster.topics.insert(t.clone());
                        self.topic_index.entry(t.clone()).or_default().insert(cluster_id);
                    }
                }
            }
        }

        // Update composite anomaly score based on current burst state
        cluster.anomaly_score = self.burst_detector.composite_anomaly_score(&cluster.topics);

        // Collect event title for AI context
        if let Some(ref t) = event.title {
            let trimmed = t.trim();
            if !trimmed.is_empty() && cluster.event_titles.len() < self.config.cluster_caps.max_event_titles {
                cluster.event_titles.push(trimmed.to_string());
            }
        }

        // Only regenerate formulaic title if we don't have an AI title yet
        if !cluster.has_ai_title {
            let new_title = Self::generate_title(
                &cluster.entities,
                &cluster.topics,
                &cluster.region_codes,
            );
            // Title stability: don't drift parent situation titles
            if !Self::should_accept_title(&cluster.title, &new_title, child_count, cluster.event_count, cluster.phase, cluster.severity) {
                info!(
                    cluster_id = %cluster_id,
                    old_title = %cluster.title,
                    rejected_title = %new_title,
                    child_count,
                    event_count = cluster.event_count,
                    "Title update rejected: stability check for parent situation"
                );
            } else {
                cluster.title = new_title;
            }
        }
    }

    /// Create a brand-new cluster from a single event.
    fn create_cluster(
        &mut self,
        event: &InsertableEvent,
        entities: HashSet<String>,
        topics: HashSet<String>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now();

        let mut region_codes = HashSet::new();
        if let Some(ref rc) = event.region_code {
            region_codes.insert(normalize_region(rc).to_string());
        }

        let centroid = match (event.latitude, event.longitude) {
            (Some(lat), Some(lon)) => Some((lat, lon)),
            _ => None,
        };

        let event_ref = event
            .source_id
            .clone()
            .unwrap_or_else(|| event.event_type.to_string());

        let mut source_types = HashSet::new();
        source_types.insert(event.source_type);

        let title = Self::generate_title(&entities, &topics, &region_codes);

        let mut event_titles = Vec::new();
        if let Some(ref t) = event.title {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                event_titles.push(trimmed.to_string());
            }
        }

        let mut severity = event.severity;
        // Boost severity for high-relevance enrichment, but ONLY for event types
        // that represent actual security threats (conflict, nuclear, etc.)
        // Political scandals, trade disputes, etc. should not auto-escalate.
        if let Some(enrichment) = event.payload.get("enrichment") {
            if let Some(score) = enrichment.get("relevance_score").and_then(|v| v.as_f64()) {
                let is_security_relevant = matches!(
                    event.event_type,
                    EventType::ConflictEvent
                        | EventType::ThermalAnomaly
                        | EventType::NuclearEvent
                        | EventType::GpsInterference
                        | EventType::SeismicEvent
                        | EventType::NotamEvent
                        | EventType::GeoEvent
                );
                if score >= 0.85 && is_security_relevant {
                    severity = severity.max(Severity::High);
                }
            }
        }
        if event.event_type == EventType::ConflictEvent {
            if let Some(fatalities) = event.payload.get("fatalities").and_then(|v| v.as_f64()) {
                if fatalities > 0.0 {
                    severity = severity.max(Severity::High);
                }
            }
        }

        let anomaly_score = self.burst_detector.composite_anomaly_score(&topics);

        let cluster = SituationCluster {
            id,
            title,
            entities: entities.clone(),
            topics: topics.clone(),
            event_ids: vec![(event.event_time, event_ref)],
            region_codes,
            severity,
            first_seen: event.event_time,
            last_updated: now,
            centroid,
            event_count: 1,
            signal_event_count: if is_high_signal_event(event.event_type) { 1 } else { 0 },
            source_types,
            parent_id: None,
            event_titles,
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: Vec::new(),
            certainty: 0.0,
            anomaly_score,
        };

        // Update indices
        for e in &entities {
            self.entity_index.entry(e.clone()).or_default().insert(id);
        }
        for t in &topics {
            self.topic_index.entry(t.clone()).or_default().insert(id);
        }

        self.clusters.insert(id, cluster);
        id
    }

    /// Build a human-readable title from the top entities, topics, and regions.
    fn generate_title(
        entities: &HashSet<String>,
        topics: &HashSet<String>,
        regions: &HashSet<String>,
    ) -> String {
        let region_label = regions
            .iter()
            .next()
            .map(|rc| region_code_to_name(rc))
            .unwrap_or_else(|| "Unknown Region".to_string());

        // Pick the best topic for context (prefer descriptive ones)
        let topic_label = {
            let mut sorted: Vec<&String> = topics.iter().collect();
            sorted.sort();
            sorted.first().map(|t| title_case(t))
        };

        if !entities.is_empty() {
            let mut sorted: Vec<&String> = entities.iter().collect();
            sorted.sort();
            let entity_part = if sorted.len() >= 2 {
                format!("{}, {}", title_case(sorted[0]), title_case(sorted[1]))
            } else {
                title_case(sorted[0])
            };
            // If we have a topic, use it as context
            if let Some(ref topic) = topic_label {
                return format!("{topic}: {entity_part} ({region_label})");
            }
            return format!("{entity_part} — {region_label}");
        }

        if let Some(topic) = topic_label {
            return format!("{topic} in {region_label}");
        }

        format!("Activity in {region_label}")
    }

    /// Detect garbage titles produced by LLM refusals — these should never be locked.
    fn is_garbage_title(title: &str) -> bool {
        let lower = title.to_lowercase();
        // LLM refusal patterns
        if lower.contains("no relevant")
            || lower.contains("no location")
            || lower.contains("no information")
            || lower.contains("not identified")
            || lower.contains("no core situation")
            || lower.contains("no context provided")
            || lower.contains("unspecified")
        {
            return true;
        }
        // Compound "and" titles joining unrelated concepts (magnet cluster sign)
        // Short "and" titles (<=5 words like "Israel and Lebanon Border Conflict") are OK
        if lower.contains(" and ") && title.split_whitespace().count() >= 6 {
            return true;
        }
        // Vague filler patterns
        let vague = [
            "economic security concerns",
            "regional security concerns",
            "security tensions",
            "security concerns",
            "unspecified challenges",
            "face unspecified",
            "i cannot generate",
            "cannot generate a meaningful",
            "no logical connection",
        ];
        vague.iter().any(|p| lower.contains(p))
    }

    /// Title stability check — phase and severity-aware. Returns true if the new
    /// title should be accepted, false if it should be rejected to prevent drift.
    ///
    /// Locking rules:
    /// - Garbage old title → always accept replacement
    /// - Active + High/Critical → always lock (mature high-priority situations)
    /// - Declining/Resolved/Historical → always lock (situation winding down)
    /// - Large parents (>10 children or >50 events) → always lock
    /// - Medium parents (>=5 children) → Jaccard overlap check
    /// - Emerging/Developing → existing size-based logic
    fn should_accept_title(
        old_title: &str,
        new_title: &str,
        child_count: usize,
        event_count: usize,
        phase: SituationPhase,
        severity: Severity,
    ) -> bool {
        // Garbage titles should always be replaced regardless of locks
        if Self::is_garbage_title(old_title) {
            return true;
        }

        // Phase-based locks: mature or winding-down situations should not churn titles
        match phase {
            SituationPhase::Active if severity.rank() >= Severity::High.rank() => return false,
            SituationPhase::Declining | SituationPhase::Resolved | SituationPhase::Historical => return false,
            _ => {}
        }

        // Large parents: lock title entirely
        if child_count > 10 || event_count > 50 {
            return false;
        }
        // Medium parents: check word overlap
        if child_count >= 5 {
            let old_words: HashSet<String> = old_title
                .to_lowercase()
                .split_whitespace()
                .map(|w| w.to_string())
                .collect();
            if old_words.is_empty() {
                return true;
            }
            let new_words: HashSet<String> = new_title
                .to_lowercase()
                .split_whitespace()
                .map(|w| w.to_string())
                .collect();
            let retained = old_words.iter().filter(|w| new_words.contains(*w)).count();
            let ratio = retained as f64 / old_words.len() as f64;
            if ratio < 0.3 {
                return false;
            }
        }
        true
    }

    /// Get a cluster by ID (read-only).
    pub fn get_cluster(&self, id: &Uuid) -> Option<&SituationCluster> {
        self.clusters.get(id)
    }

    /// Return clusters that pass the quality gate but don't yet have AI-generated titles.
    /// Excludes clusters whose IDs are in `pending` (already being processed).
    pub fn clusters_needing_titles(&self, pending: &HashSet<Uuid>) -> Vec<&SituationCluster> {
        let quality = &self.config.quality;
        let now = Utc::now();
        self.clusters
            .values()
            .filter(|c| {
                if pending.contains(&c.id) {
                    return false;
                }
                // New clusters that haven't received an AI title yet
                let min_events = if c.parent_id.is_some() {
                    quality.min_events_child_title
                } else {
                    quality.min_events_for_title
                };
                if !c.has_ai_title && c.event_count >= min_events {
                    return true;
                }
                // Force re-generation for garbage titles (LLM refusals)
                if c.has_ai_title && Self::is_garbage_title(&c.title) {
                    return true;
                }
                // Re-evaluate title when enough new signal events AND enough time has passed
                if c.has_ai_title {
                    let new_signals = c.signal_event_count.saturating_sub(c.title_signal_count_at_gen);
                    let age_minutes = (now - c.last_title_gen).num_minutes();
                    if new_signals >= quality.signal_events_for_retitle && age_minutes >= 30 {
                        return true;
                    }
                }
                false
            })
            .collect()
    }

    /// Update a cluster's title with an AI-generated one.
    /// Applies title stability checks for parent situations to prevent drift.
    pub fn update_cluster_title(&mut self, cluster_id: Uuid, title: String) {
        // Reject garbage titles from the AI — don't store them, let it retry next cycle
        if Self::is_garbage_title(&title) {
            debug!(
                cluster_id = %cluster_id,
                rejected_title = %title,
                "AI title rejected: new title is garbage, keeping old"
            );
            return;
        }

        let child_count = self.clusters.values().filter(|c| c.parent_id == Some(cluster_id)).count();
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            // For re-generation (already has AI title), apply stability check
            if cluster.has_ai_title {
                if !Self::should_accept_title(&cluster.title, &title, child_count, cluster.event_count, cluster.phase, cluster.severity) {
                    info!(
                        cluster_id = %cluster_id,
                        old_title = %cluster.title,
                        rejected_title = %title,
                        child_count,
                        event_count = cluster.event_count,
                        "AI title update rejected: stability check for parent situation"
                    );
                    // Still update the signal count to prevent re-triggering
                    cluster.title_signal_count_at_gen = cluster.signal_event_count;
                    return;
                }
            }
            cluster.title = title;
            cluster.has_ai_title = true;
            cluster.title_signal_count_at_gen = cluster.signal_event_count;
            cluster.last_title_gen = Utc::now();
        }
    }

    /// Attach or accumulate supplementary web search data for a cluster.
    /// If the cluster already has supplementary data, new articles are merged
    /// (deduped by URL, capped at 10 articles total).
    pub fn update_cluster_supplementary(&mut self, cluster_id: Uuid, data: sr_intel::search::SupplementaryData) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.last_searched = Some(Utc::now());
            match cluster.supplementary.as_mut() {
                Some(existing) => existing.merge(data),
                None => cluster.supplementary = Some(data),
            }
        }
    }

    /// Restore persisted search history for a cluster from the database.
    pub fn restore_search_history(&mut self, cluster_id: Uuid, gap: sr_intel::search::GapType, last: DateTime<Utc>, total: u32, empty: u32) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.search_history.set_from_db(gap, last, total, empty);
        }
    }

    /// Restore pre-built clusters from the database, rebuilding all internal indices.
    /// Used on startup when persisted cluster state is available, avoiding the need
    /// to replay events through `ingest()`.
    pub fn restore_clusters(&mut self, clusters: Vec<SituationCluster>) {
        // First pass: collect all cluster IDs so we can detect orphans
        let all_ids: HashSet<Uuid> = clusters.iter().map(|c| c.id).collect();

        for mut cluster in clusters {
            let id = cluster.id;

            // Fix orphans: if parent_id references a non-existent cluster, promote to top-level.
            // This allows the cluster to participate in merge_overlapping() again.
            if let Some(pid) = cluster.parent_id {
                if !all_ids.contains(&pid) {
                    debug!(
                        cluster_id = %id,
                        missing_parent = %pid,
                        title = %cluster.title,
                        "Promoting orphan cluster to top-level (parent not found)"
                    );
                    cluster.parent_id = None;
                }
            }

            // Rebuild entity index
            for entity in &cluster.entities {
                self.entity_index.entry(entity.clone()).or_default().insert(id);
            }
            // Rebuild topic index
            for topic in &cluster.topics {
                self.topic_index.entry(topic.clone()).or_default().insert(id);
            }
            self.clusters.insert(id, cluster);
        }
        // Post-restore: enforce max_children_per_parent cap.
        // Orphan excess children (smallest first) so parents don't balloon.
        let max_children = self.config.cluster_caps.max_children_per_parent;
        let mut children_per_parent: HashMap<Uuid, Vec<(Uuid, usize)>> = HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                children_per_parent.entry(pid).or_default().push((c.id, c.event_count));
            }
        }
        let mut orphaned = 0usize;
        for (pid, mut kids) in children_per_parent {
            if kids.len() <= max_children {
                continue;
            }
            // Sort by event_count descending — keep the biggest children
            kids.sort_by(|a, b| b.1.cmp(&a.1));
            for &(kid_id, _) in kids.iter().skip(max_children) {
                if let Some(kid) = self.clusters.get_mut(&kid_id) {
                    kid.parent_id = None;
                    orphaned += 1;
                }
            }
            info!(
                parent = %pid,
                over = kids.len() - max_children,
                "Orphaned excess children to enforce max_children cap"
            );
        }
        if orphaned > 0 {
            info!(orphaned, "Post-restore child cap enforcement complete");
        }

        // Detach significant children that are parents themselves.
        // A cluster with 3+ children of its own is an established situation that
        // was incorrectly absorbed by a magnet cluster — promote to top-level.
        let significant_children: Vec<Uuid> = self.clusters.values()
            .filter(|c| c.parent_id.is_some())
            .filter(|c| {
                let own_children = self.clusters.values()
                    .filter(|k| k.parent_id == Some(c.id))
                    .count();
                own_children >= 3
            })
            .map(|c| c.id)
            .collect();

        let detached = significant_children.len();
        for cid in &significant_children {
            if let Some(cluster) = self.clusters.get_mut(cid) {
                info!(
                    cluster_id = %cid,
                    title = %cluster.title,
                    "Detaching significant child (has own sub-situations) to top-level"
                );
                cluster.parent_id = None;
            }
        }
        if detached > 0 {
            info!(detached, "Post-restore grandparent detachment complete");
        }

        // Propagate severity from substantial children to parents on restore,
        // using proportional threshold (same logic as periodic lifecycle sweep).
        let min_events = self.config.quality.min_events_standalone;
        let threshold = self.config.sweep.severity_propagation_threshold;
        let allow_decrease = self.config.sweep.severity_propagation_allow_decrease;
        let parent_ids: Vec<Uuid> = self.clusters.values()
            .filter(|c| c.parent_id.is_none())
            .map(|c| c.id)
            .collect();
        let mut severity_changed = 0usize;
        for pid in parent_ids {
            let child_severities: Vec<Severity> = self.clusters.values()
                .filter(|c| c.parent_id == Some(pid))
                .filter(|c| c.event_count >= min_events)
                .map(|c| c.severity)
                .collect();
            if child_severities.is_empty() {
                continue;
            }
            let total = child_severities.len() as f32;
            let candidate = [Severity::Critical, Severity::High, Severity::Medium, Severity::Low, Severity::Info]
                .iter()
                .find(|&&sev| {
                    let at_or_above = child_severities.iter().filter(|s| s.rank() >= sev.rank()).count() as f32;
                    at_or_above / total >= threshold
                })
                .copied()
                .unwrap_or(Severity::Info);
            if let Some(parent) = self.clusters.get_mut(&pid) {
                if candidate != parent.severity && (candidate > parent.severity || allow_decrease) {
                    info!(
                        cluster_id = %pid,
                        title = %parent.title,
                        from = %parent.severity,
                        to = %candidate,
                        "Parent severity adjusted via proportional threshold on restore"
                    );
                    parent.severity = candidate;
                    severity_changed += 1;
                }
            }
        }
        if severity_changed > 0 {
            info!(severity_changed, "Post-restore parent severity propagation complete");
        }

        info!(count = self.clusters.len(), "Restored clusters from DB");
    }

    /// Record that a specific gap type was searched for a cluster.
    pub fn record_gap_searched(&mut self, cluster_id: Uuid, gap_type: sr_intel::search::GapType) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.search_history.last_searched_by_type.insert(gap_type, Utc::now());
            cluster.search_history.total_searches += 1;
        }
    }

    /// Record that a search for a cluster returned no results.
    pub fn record_empty_search(&mut self, cluster_id: Uuid) {
        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.search_history.empty_searches += 1;
        }
    }
}

impl Default for SituationGraph {
    fn default() -> Self {
        Self::new(Arc::new(PipelineConfig::default()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    use lifecycle::{compute_gap_tolerance, evaluate_phase_transition, compute_certainty_with_config, PhaseMetrics};
    use scoring::distance_km;

    /// Helper to parse a SourceType from a JSON override string, defaulting to Acled.
    fn parse_source_type(s: Option<&str>) -> SourceType {
        match s {
            Some("acled") => SourceType::Acled,
            Some("gdelt") => SourceType::Gdelt,
            Some("gdelt-geo") => SourceType::GdeltGeo,
            Some("shodan") => SourceType::Shodan,
            Some("firms") => SourceType::Firms,
            Some("usgs") => SourceType::Usgs,
            Some("certstream") => SourceType::Certstream,
            Some("airplaneslive") => SourceType::AirplanesLive,
            Some("adsb-lol") => SourceType::AdsbLol,
            Some("adsb-fi") => SourceType::AdsbFi,
            Some("opensky") => SourceType::Opensky,
            Some("bgp") => SourceType::Bgp,
            Some("cloudflare") => SourceType::Cloudflare,
            Some("ioda") => SourceType::Ioda,
            Some("ooni") => SourceType::Ooni,
            Some("otx") => SourceType::Otx,
            Some("telegram") => SourceType::Telegram,
            Some("nuclear") => SourceType::Nuclear,
            Some("gpsjam") => SourceType::Gpsjam,
            Some("gfw") => SourceType::Gfw,
            Some("notam") => SourceType::Notam,
            Some("ais") => SourceType::Ais,
            Some("geoconfirmed") => SourceType::Geoconfirmed,
            Some("rss-news") => SourceType::RssNews,
            _ => SourceType::Acled, // default for tests
        }
    }

    /// Helper to parse an EventType from a JSON override string, defaulting to ConflictEvent.
    fn parse_event_type(s: Option<&str>) -> EventType {
        match s {
            Some("conflict_event") => EventType::ConflictEvent,
            Some("thermal_anomaly") => EventType::ThermalAnomaly,
            Some("seismic_event") => EventType::SeismicEvent,
            Some("news_article") => EventType::NewsArticle,
            Some("geo_news") => EventType::GeoNews,
            Some("bgp_anomaly") => EventType::BgpAnomaly,
            Some("internet_outage") => EventType::InternetOutage,
            Some("gps_interference") => EventType::GpsInterference,
            Some("nuclear_event") => EventType::NuclearEvent,
            Some("flight_position") => EventType::FlightPosition,
            Some("vessel_position") => EventType::VesselPosition,
            Some("cert_issued") => EventType::CertIssued,
            Some("shodan_banner") => EventType::ShodanBanner,
            Some("threat_intel") => EventType::ThreatIntel,
            Some("censorship_event") => EventType::CensorshipEvent,
            Some("fishing_event") => EventType::FishingEvent,
            Some("telegram_message") => EventType::TelegramMessage,
            Some("notam_event") => EventType::NotamEvent,
            Some("geo_event") => EventType::GeoEvent,
            Some("source_health") => EventType::SourceHealth,
            _ => EventType::ConflictEvent, // default for tests
        }
    }

    fn make_event(overrides: serde_json::Value) -> InsertableEvent {
        InsertableEvent {
            event_time: Utc::now(),
            source_type: parse_source_type(overrides["source_type"].as_str()),
            source_id: overrides["source_id"].as_str().map(|s| s.to_string()),
            longitude: overrides["longitude"].as_f64(),
            latitude: overrides["latitude"].as_f64(),
            region_code: overrides["region_code"].as_str().map(|s| s.to_string()),
            entity_id: None,
            entity_name: overrides["entity_name"].as_str().map(|s| s.to_string()),
            event_type: parse_event_type(overrides["event_type"].as_str()),
            severity: Severity::from_str_lossy(
                overrides["severity"].as_str().unwrap_or("medium"),
            ),
            confidence: None,
            tags: overrides["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            title: overrides["title"].as_str().map(|s| s.to_string()),
            description: None,
            payload: overrides
                .get("payload")
                .cloned()
                .unwrap_or_else(|| json!({})),
            heading: None,
            speed: None,
            altitude: None,
        }
    }

    #[test]
    fn test_skip_without_signals() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({}));
        g.ingest(&event, None);
        assert!(g.clusters.is_empty());
    }

    #[test]
    fn test_create_cluster_with_entity() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "entity_name": "USS Gerald Ford",
            "region_code": "US",
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let cluster = g.clusters.values().next().unwrap();
        // Entities are now normalized to lowercase
        assert!(cluster.entities.contains("uss gerald ford"));
        assert!(cluster.title.contains("Uss Gerald Ford"));
    }

    #[test]
    fn test_create_cluster_with_topic_tag() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "tags": ["topic:cyber-attack"],
            "region_code": "UA",
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let cluster = g.clusters.values().next().unwrap();
        assert!(cluster.topics.contains("cyber-attack"));
    }

    #[test]
    fn test_merge_by_shared_entities() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Iran",
            "tags": ["actor:IRGC", "topic:missile"],
            "region_code": "IR",
        }));
        g.ingest(&e1, None);
        assert_eq!(g.clusters.len(), 1);

        let e2 = make_event(json!({
            "entity_name": "Iran",
            "tags": ["topic:missile"],
            "source_type": "acled",
            "region_code": "IR",
        }));
        g.ingest(&e2, None);
        assert_eq!(g.clusters.len(), 1);

        let cluster = g.clusters.values().next().unwrap();
        assert_eq!(cluster.event_count, 2);
        assert!(cluster.source_types.contains(&SourceType::Acled));
    }

    #[test]
    fn test_no_merge_below_threshold() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Alpha",
            "region_code": "US",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "tags": ["topic:supply-chain"],
            "entity_name": "Beta",
            "region_code": "UK",
        }));
        g.ingest(&e2, None);
        assert_eq!(g.clusters.len(), 2);
    }

    #[test]
    fn test_severity_max() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Target",
            "tags": ["actor:APT28", "topic:phishing"],
            "severity": "low",
            "region_code": "DE",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "entity_name": "Target",
            "tags": ["topic:phishing"],
            "severity": "critical",
            "region_code": "DE",
        }));
        g.ingest(&e2, None);

        assert_eq!(g.clusters.len(), 1);
        let cluster = g.clusters.values().next().unwrap();
        assert_eq!(cluster.severity, Severity::Critical);
    }

    #[test]
    fn test_centroid_update() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Site",
            "tags": ["topic:construction"],
            "latitude": 40.0,
            "longitude": 30.0,
            "region_code": "TR",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "entity_name": "Site",
            "tags": ["topic:construction"],
            "latitude": 40.1,
            "longitude": 30.1,
            "region_code": "TR",
        }));
        g.ingest(&e2, None);

        assert_eq!(g.clusters.len(), 1);
        let cluster = g.clusters.values().next().unwrap();
        let (clat, clon) = cluster.centroid.unwrap();
        assert!((clat - 40.05).abs() < 0.01);
        assert!((clon - 30.05).abs() < 0.01);
    }

    #[test]
    fn test_enrichment_entities_and_topics() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "payload": {
                "enrichment": {
                    "entities": [
                        {"name": "Hezbollah", "type": "org"},
                        {"name": "Lebanon", "type": "location"}
                    ],
                    "topics": ["missile", "border-conflict"]
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let cluster = g.clusters.values().next().unwrap();
        assert!(cluster.entities.contains("hezbollah"));
        assert!(cluster.entities.contains("lebanon"));
        assert!(cluster.topics.contains("missile"));
        assert!(cluster.topics.contains("border-conflict"));
    }

    #[test]
    fn test_prune_stale() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "entity_name": "OldEntity",
            "region_code": "XX",
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);

        let cid = *g.clusters.keys().next().unwrap();
        g.clusters.get_mut(&cid).unwrap().last_updated =
            Utc::now() - chrono::Duration::hours(25);

        g.prune_stale(std::time::Duration::from_secs(24 * 3600));
        assert!(g.clusters.is_empty());
        assert!(g.entity_index.is_empty());
    }

    #[test]
    fn test_haversine() {
        let d = distance_km(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((d - 344.0).abs() < 5.0);
    }

    #[test]
    fn test_active_clusters_sorted() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "First", "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "geoconfirmed" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "firms" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "firms" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "geoconfirmed" })), None);
        g.ingest(&make_event(json!({ "entity_name": "First", "tags": ["topic:x"], "source_type": "acled" })), None);

        g.ingest(&make_event(json!({ "entity_name": "Second", "source_type": "usgs" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "notam" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "usgs" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "usgs" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "usgs" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "notam" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "usgs" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Second", "tags": ["topic:y"], "source_type": "usgs" })), None);

        let clusters = g.active_clusters();
        assert_eq!(clusters.len(), 2);
        assert!(clusters[0].last_updated >= clusters[1].last_updated);
    }

    #[test]
    fn test_clusters_needing_search_first_time() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "Target", "source_type": "geoconfirmed", "severity": "high" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Target", "tags": ["topic:test"], "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Target", "tags": ["topic:test"], "source_type": "firms" })), None);

        let cid = *g.clusters.keys().next().unwrap();

        let needing = g.clusters_needing_search(&HashSet::new());
        assert!(needing.is_empty());

        g.update_cluster_title(cid, "Test Situation Title".to_string());

        let needing = g.clusters_needing_search(&HashSet::new());
        assert_eq!(needing.len(), 1);
    }

    #[test]
    fn test_clusters_needing_search_respects_severity_intervals() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "Alpha", "source_type": "geoconfirmed", "severity": "high" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Alpha", "tags": ["topic:t1"], "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Alpha", "tags": ["topic:t1"], "source_type": "firms" })), None);
        let cid = *g.clusters.keys().next().unwrap();
        g.update_cluster_title(cid, "High Severity Situation".to_string());

        g.clusters.get_mut(&cid).unwrap().last_searched =
            Some(Utc::now() - chrono::Duration::minutes(35));

        let needing = g.clusters_needing_search(&HashSet::new());
        assert_eq!(needing.len(), 1);

        g.clusters.get_mut(&cid).unwrap().last_searched =
            Some(Utc::now() - chrono::Duration::minutes(20));
        let needing = g.clusters_needing_search(&HashSet::new());
        assert!(needing.is_empty());
    }

    #[test]
    fn test_clusters_needing_search_medium_severity_interval() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "Beta", "source_type": "geoconfirmed", "severity": "medium" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Beta", "tags": ["topic:t2"], "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Beta", "tags": ["topic:t2"], "source_type": "firms" })), None);
        let cid = *g.clusters.keys().next().unwrap();
        g.update_cluster_title(cid, "Medium Severity Situation".to_string());

        g.clusters.get_mut(&cid).unwrap().last_searched =
            Some(Utc::now() - chrono::Duration::minutes(90));
        let needing = g.clusters_needing_search(&HashSet::new());
        assert!(needing.is_empty());

        g.clusters.get_mut(&cid).unwrap().last_searched =
            Some(Utc::now() - chrono::Duration::hours(3));
        let needing = g.clusters_needing_search(&HashSet::new());
        assert_eq!(needing.len(), 1);
    }

    #[test]
    fn test_clusters_needing_search_excludes_pending() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "Gamma", "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Gamma", "tags": ["topic:t3"], "source_type": "gdelt" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Gamma", "tags": ["topic:t3"], "source_type": "firms" })), None);
        let cid = *g.clusters.keys().next().unwrap();
        g.update_cluster_title(cid, "Pending Test".to_string());

        let mut pending = HashSet::new();
        pending.insert(cid);

        let needing = g.clusters_needing_search(&pending);
        assert!(needing.is_empty());
    }

    #[test]
    fn test_temporal_decay_via_config() {
        let config = PipelineConfig::default();
        let (half_life, offset) = config.decay_params("conflict_event");
        assert_eq!(half_life, 4.0);
        assert_eq!(offset, 1.0);

        let decay = (-0.693 / half_life * (0.0_f64 - offset).max(0.0)).exp();
        assert!((decay - 1.0).abs() < 0.01, "Fresh cluster decay should be ~1.0, got {decay}");

        let effective_dt = (8.0 - offset).max(0.0);
        let decay = (-0.693 / half_life * effective_dt).exp();
        assert!(decay < 0.5 && decay > 0.1, "8h-old decay should be ~0.30, got {decay}");
    }

    #[test]
    fn test_geo_radius_via_config() {
        let config = PipelineConfig::default();
        assert_eq!(config.geo_radius_km("conflict_event"), 50.0);
        assert_eq!(config.geo_radius_km("thermal_anomaly"), 25.0);
        assert_eq!(config.geo_radius_km("gps_interference"), 300.0);
        assert_eq!(config.geo_radius_km("shodan_banner"), 150.0);
    }

    #[test]
    fn test_size_penalty_via_config() {
        let config = PipelineConfig::default();
        assert_eq!(config.size_penalty(5), Some(0));
        assert!(config.size_penalty(50).unwrap() < 0);
        assert!(config.size_penalty(500).is_some());
    }

    #[test]
    fn test_geo_graduated_scoring() {
        let mut g = SituationGraph::default();
        let e1 = make_event(json!({
            "entity_name": "Force Alpha",
            "tags": ["topic:airstrike"],
            "latitude": 40.0,
            "longitude": 30.0,
            "region_code": "TR",
            "event_type": "conflict_event",
        }));
        g.ingest(&e1, None);
        assert_eq!(g.clusters.len(), 1);

        let e2 = make_event(json!({
            "entity_name": "Force Alpha",
            "tags": ["topic:airstrike"],
            "latitude": 40.01,
            "longitude": 30.01,
            "region_code": "TR",
            "event_type": "conflict_event",
            "source_type": "acled",
        }));
        g.ingest(&e2, None);
        assert_eq!(g.clusters.len(), 1);
    }

    #[test]
    fn test_supplementary_accumulation() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({ "entity_name": "Delta", "source_type": "acled" })), None);
        g.ingest(&make_event(json!({ "entity_name": "Delta", "tags": ["topic:t4"], "source_type": "gdelt" })), None);
        let cid = *g.clusters.keys().next().unwrap();

        let data1 = sr_intel::search::SupplementaryData {
            articles: vec![sr_intel::search::SearchArticle {
                title: "Article 1".into(),
                url: "https://example.com/1".into(),
                snippet: "First article".into(),
                published_date: None,
                highlights: Vec::new(),
            }],
            context: "First article".into(),
        };
        g.update_cluster_supplementary(cid, data1);
        assert!(g.clusters[&cid].last_searched.is_some());
        assert_eq!(g.clusters[&cid].supplementary.as_ref().unwrap().articles.len(), 1);

        let data2 = sr_intel::search::SupplementaryData {
            articles: vec![
                sr_intel::search::SearchArticle {
                    title: "Article 1 dup".into(),
                    url: "https://example.com/1".into(),
                    snippet: "Duplicate".into(),
                    published_date: None,
                    highlights: Vec::new(),
                },
                sr_intel::search::SearchArticle {
                    title: "Article 2".into(),
                    url: "https://example.com/2".into(),
                    snippet: "Second article".into(),
                    published_date: None,
                    highlights: Vec::new(),
                },
            ],
            context: "whatever".into(),
        };
        g.update_cluster_supplementary(cid, data2);
        let supp = g.clusters[&cid].supplementary.as_ref().unwrap();
        assert_eq!(supp.articles.len(), 2);
        assert_eq!(supp.articles[0].title, "Article 1");
        assert_eq!(supp.articles[1].title, "Article 2");
    }

    #[test]
    fn test_compute_certainty_sigmoid() {
        let c = sr_config::CertaintyConfig::default();
        let now = Utc::now();

        let make_cluster = |source_count: usize, event_count: usize, entity_count: usize, has_ai_title: bool| -> SituationCluster {
            let mut source_types = HashSet::new();
            let all = [SourceType::Gdelt, SourceType::Geoconfirmed, SourceType::Acled, SourceType::Firms];
            for st in all.iter().take(source_count) { source_types.insert(*st); }
            let entities: HashSet<String> = (0..entity_count).map(|i| format!("entity_{i}")).collect();
            SituationCluster {
                id: Uuid::new_v4(),
                title: "test".into(),
                entities,
                topics: HashSet::new(),
                event_ids: (0..event_count).map(|i| (now, format!("e{i}"))).collect(),
                region_codes: HashSet::from(["middle-east".into()]),
                severity: Severity::Medium,
                first_seen: now,
                last_updated: now,
                centroid: None,
                event_count,
                signal_event_count: 0,
                source_types,
                parent_id: None,
                event_titles: vec![],
                has_ai_title,
                title_signal_count_at_gen: 0,
                last_title_gen: now,
                supplementary: None,
                last_searched: None,
                search_history: SearchHistory::default(),
                phase: SituationPhase::Emerging,
                phase_changed_at: now,
                peak_event_rate: 0.0,
                peak_rate_at: now,
                phase_transitions: vec![],
                certainty: 0.0,
                anomaly_score: 0.0,
            }
        };

        let low = compute_certainty_with_config(&make_cluster(1, 2, 0, false), &c);
        assert!(low > 0.0, "Should have positive base, got {low}");
        assert!(low < 0.5, "Minimal cluster should be <0.5, got {low}");

        let high = compute_certainty_with_config(&make_cluster(4, 25, 3, true), &c);
        assert!(high > 0.7, "Rich cluster expected >0.7, got {high}");
        assert!(high <= 1.0, "Should be capped at 1.0, got {high}");

        let c1 = compute_certainty_with_config(&make_cluster(1, 5, 0, false), &c);
        let c2 = compute_certainty_with_config(&make_cluster(2, 5, 0, false), &c);
        let c3 = compute_certainty_with_config(&make_cluster(2, 20, 0, false), &c);
        let c4 = compute_certainty_with_config(&make_cluster(2, 20, 5, false), &c);
        let c5 = compute_certainty_with_config(&make_cluster(2, 20, 5, true), &c);
        assert!(c2 > c1, "More sources should increase: {c1} -> {c2}");
        assert!(c3 > c2, "More events should increase: {c2} -> {c3}");
        assert!(c4 > c3, "Entities should increase: {c3} -> {c4}");
        assert!(c5 > c4, "AI title should increase: {c4} -> {c5}");
    }

    /// Helper to create a minimal SituationCluster for phase transition tests.
    fn make_cluster_for_phase(severity: Severity, peak_event_rate: f64, source_types: HashSet<SourceType>) -> SituationCluster {
        let now = Utc::now();
        SituationCluster {
            id: Uuid::new_v4(),
            title: "Test Cluster".into(),
            entities: HashSet::new(),
            topics: HashSet::new(),
            event_ids: vec![(now, "e1".into())],
            region_codes: HashSet::from(["middle-east".into()]),
            severity,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 1,
            signal_event_count: 0,
            source_types,
            parent_id: None,
            event_titles: vec![],
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        }
    }

    #[test]
    fn test_gap_tolerance_values() {
        let low_cluster = make_cluster_for_phase(
            Severity::Low,
            0.0,
            HashSet::from([SourceType::Gdelt]),
        );
        let low_tol = compute_gap_tolerance(&low_cluster, &sr_config::PhaseConfig::default());
        assert!(
            (low_tol - 2.0).abs() < 0.01,
            "Low/single-source should give ~2.0h tolerance, got {low_tol}"
        );

        let critical_cluster = make_cluster_for_phase(
            Severity::Critical,
            15.0,
            HashSet::from([SourceType::Gdelt, SourceType::Geoconfirmed, SourceType::Acled, SourceType::Firms]),
        );
        let critical_tol = compute_gap_tolerance(&critical_cluster, &sr_config::PhaseConfig::default());
        assert!(
            critical_tol > 20.0,
            "Critical/multi-source should give >20h tolerance, got {critical_tol}"
        );
        assert!(
            (critical_tol - 39.0).abs() < 0.1,
            "Expected ~39.0h for critical/4-source/15-rate, got {critical_tol}"
        );

        let medium_cluster = make_cluster_for_phase(
            Severity::Medium,
            5.0,
            HashSet::from([SourceType::Gdelt, SourceType::Firms]),
        );
        let medium_tol = compute_gap_tolerance(&medium_cluster, &sr_config::PhaseConfig::default());
        assert!(
            (medium_tol - 6.0).abs() < 0.1,
            "Medium/2-source/5-rate should give ~6.0h, got {medium_tol}"
        );
    }

    #[test]
    fn test_session_window_hot_situation_stays_active() {
        let cluster = make_cluster_for_phase(
            Severity::Critical,
            15.0,
            HashSet::from([SourceType::Gdelt, SourceType::Geoconfirmed, SourceType::Acled, SourceType::Firms]),
        );
        let gap_tolerance = compute_gap_tolerance(&cluster, &sr_config::PhaseConfig::default());
        assert!(
            (gap_tolerance - 39.0).abs() < 0.1,
            "Expected gap_tolerance ~39.0, got {gap_tolerance}"
        );

        let metrics_5h = PhaseMetrics {
            event_velocity_5m: 0,
            event_velocity_30m: 0,
            peak_rate: 15.0,
            current_rate: 5.0,
            source_diversity: 4,
            max_severity_rank: Severity::Critical.rank(),
            hours_since_last_event: 5.0,
            event_count: 50,
        };
        let phases = sr_config::PhaseConfig::default();
        let result = evaluate_phase_transition(SituationPhase::Active, &metrics_5h, gap_tolerance, &phases);
        assert!(
            result.is_none(),
            "Hot critical situation should stay Active at 5h gap (threshold={:.1}h), but got {:?}",
            gap_tolerance * 0.5,
            result
        );

        let metrics_beyond = PhaseMetrics {
            event_velocity_5m: 0,
            event_velocity_30m: 0,
            peak_rate: 15.0,
            current_rate: 5.0,
            source_diversity: 4,
            max_severity_rank: Severity::Critical.rank(),
            hours_since_last_event: gap_tolerance * 0.5 + 1.0,
            event_count: 50,
        };
        let result = evaluate_phase_transition(SituationPhase::Active, &metrics_beyond, gap_tolerance, &phases);
        assert!(
            matches!(result, Some((SituationPhase::Declining, _))),
            "Hot critical situation should decline at {:.1}h gap, got {:?}",
            gap_tolerance * 0.5 + 1.0,
            result
        );

        let cold_cluster = make_cluster_for_phase(
            Severity::Low,
            0.0,
            HashSet::from([SourceType::Gdelt]),
        );
        let cold_tol = compute_gap_tolerance(&cold_cluster, &sr_config::PhaseConfig::default());
        let metrics_cold = PhaseMetrics {
            event_velocity_5m: 0,
            event_velocity_30m: 0,
            peak_rate: 1.0,
            current_rate: 0.5,
            source_diversity: 1,
            max_severity_rank: Severity::Low.rank(),
            hours_since_last_event: 1.5,
            event_count: 5,
        };
        let result = evaluate_phase_transition(SituationPhase::Active, &metrics_cold, cold_tol, &phases);
        assert!(
            matches!(result, Some((SituationPhase::Declining, _))),
            "Cold low-severity situation should decline at 1.5h (threshold={:.1}h), got {:?}",
            cold_tol * 0.5,
            result
        );
    }

    // =========================================================================
    // Relevance filtering tests
    // =========================================================================

    #[test]
    fn test_relevance_filter_skips_low_relevance_news() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "event_type": "news_article",
            "source_type": "rss-news",
            "entity_name": "Baseball Team",
            "tags": ["topic:baseball"],
            "payload": {
                "enrichment": {
                    "relevance_score": 0.1,
                    "entities": [{"name": "MLB"}],
                    "topics": ["baseball"]
                }
            }
        }));
        g.ingest(&event, None);
        assert!(g.clusters.is_empty(), "Low-relevance news should not create clusters");
    }

    #[test]
    fn test_relevance_filter_accepts_high_relevance_news() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "event_type": "news_article",
            "source_type": "rss-news",
            "entity_name": "Iran",
            "tags": ["topic:iran-conflict"],
            "payload": {
                "enrichment": {
                    "relevance_score": 0.8,
                    "entities": [{"name": "Iran"}],
                    "topics": ["iran-conflict"]
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1, "High-relevance news should create a cluster");
    }

    #[test]
    fn test_relevance_filter_defaults_when_no_enrichment() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "event_type": "news_article",
            "source_type": "rss-news",
            "entity_name": "Iran",
            "tags": ["topic:iran-conflict"],
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1, "Unenriched news should default to acceptable relevance");
    }

    #[test]
    fn test_relevance_filter_only_applies_to_news() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "event_type": "geo_event",
            "source_type": "geoconfirmed",
            "entity_name": "Ukraine",
            "tags": ["topic:combat"],
            "payload": {
                "enrichment": {
                    "relevance_score": 0.05
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1, "Non-news events should bypass relevance filter");
    }

    #[test]
    fn test_relevance_filter_boundary_value() {
        let mut g = SituationGraph::default();

        let event_low = make_event(json!({
            "event_type": "news_article",
            "source_type": "rss-news",
            "entity_name": "Topic",
            "tags": ["topic:borderline"],
            "payload": {
                "enrichment": {
                    "relevance_score": 0.29,
                    "entities": [{"name": "Topic"}],
                    "topics": ["borderline"]
                }
            }
        }));
        g.ingest(&event_low, None);
        assert_eq!(g.clusters.len(), 0, "Score at 0.29 should be rejected (threshold 0.30)");

        let event = make_event(json!({
            "event_type": "news_article",
            "source_type": "rss-news",
            "entity_name": "Topic",
            "tags": ["topic:borderline"],
            "payload": {
                "enrichment": {
                    "relevance_score": 0.30,
                    "entities": [{"name": "Topic"}],
                    "topics": ["borderline"]
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1, "Score at exactly 0.30 should be accepted");
    }

    // =========================================================================
    // Garbage title detection + title stability tests
    // =========================================================================

    #[test]
    fn test_is_garbage_title_detects_refusals() {
        assert!(SituationGraph::is_garbage_title("No relevant information found"));
        assert!(SituationGraph::is_garbage_title("No location identified in data"));
        assert!(SituationGraph::is_garbage_title("NO RELEVANT Information Available"));
        assert!(SituationGraph::is_garbage_title("No core situation detected"));
        assert!(SituationGraph::is_garbage_title("No context provided for the analysis"));
    }

    #[test]
    fn test_is_garbage_title_accepts_good_titles() {
        assert!(!SituationGraph::is_garbage_title("Iran-Israel Conflict Escalation"));
        assert!(!SituationGraph::is_garbage_title("Yemen Military Activity"));
        assert!(!SituationGraph::is_garbage_title("Ukraine-Russia Maritime Conflict"));
        assert!(!SituationGraph::is_garbage_title("Myanmar Military Deployments"));
    }

    #[test]
    fn test_should_accept_title_always_replaces_garbage() {
        assert!(SituationGraph::should_accept_title(
            "No relevant information",
            "Iran War Escalation",
            20, 100, SituationPhase::Active, Severity::Critical,
        ));
    }

    #[test]
    fn test_should_accept_title_locks_active_high_severity() {
        assert!(!SituationGraph::should_accept_title(
            "Existing Good Title",
            "Completely Different Title",
            0, 10, SituationPhase::Active, Severity::High,
        ));
    }

    #[test]
    fn test_should_accept_title_locks_declining() {
        assert!(!SituationGraph::should_accept_title(
            "Existing Title",
            "New Title",
            0, 5, SituationPhase::Declining, Severity::Medium,
        ));
    }

    #[test]
    fn test_should_accept_title_locks_large_parent() {
        assert!(!SituationGraph::should_accept_title(
            "Parent Title",
            "New Title",
            15, 30, SituationPhase::Developing, Severity::Medium,
        ));
    }

    #[test]
    fn test_should_accept_title_medium_parent_overlap_check() {
        assert!(SituationGraph::should_accept_title(
            "Iran Israel Conflict Escalation",
            "Iran Israel War Escalation Update",
            5, 20, SituationPhase::Developing, Severity::Medium,
        ));

        assert!(!SituationGraph::should_accept_title(
            "Iran Israel Conflict Escalation",
            "Baseball World Cup Finals",
            5, 20, SituationPhase::Developing, Severity::Medium,
        ));
    }

    #[test]
    fn test_should_accept_title_emerging_small_accepts() {
        assert!(SituationGraph::should_accept_title(
            "Old Title",
            "Totally Different Title",
            0, 5, SituationPhase::Emerging, Severity::Medium,
        ));
    }

    // =========================================================================
    // Telemetry pruning tests
    // =========================================================================

    #[test]
    fn test_pure_telemetry_cluster_prunes_faster() {
        let mut g = SituationGraph::default();

        let now = Utc::now();
        let cid = Uuid::new_v4();
        g.clusters.insert(cid, SituationCluster {
            id: cid,
            title: "Flight Tracking Cluster".into(),
            entities: HashSet::from(["military-callsign".into()]),
            topics: HashSet::new(),
            event_ids: (0..25).map(|i| (now, format!("flight_{i}"))).collect(),
            region_codes: HashSet::from(["middle-east".into()]),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now - chrono::Duration::hours(8),
            centroid: None,
            event_count: 25,
            signal_event_count: 0,
            source_types: HashSet::from([SourceType::AirplanesLive]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        assert_eq!(g.clusters.len(), 1);
        assert_eq!(g.clusters[&cid].signal_event_count, 0);
        assert!(g.clusters[&cid].event_count >= 20);

        g.prune_stale(std::time::Duration::from_secs(24 * 3600));
        assert!(g.clusters.is_empty(), "Pure telemetry cluster should be pruned at accelerated rate (8h > 6h cutoff)");
    }

    #[test]
    fn test_signal_cluster_uses_normal_prune_rate() {
        let mut g = SituationGraph::default();

        let e = make_event(json!({
            "entity_name": "Iran",
            "tags": ["topic:iran-conflict"],
            "event_type": "news_article",
            "source_type": "rss-news",
        }));
        g.ingest(&e, None);
        assert_eq!(g.clusters.len(), 1);
        let cid = *g.clusters.keys().next().unwrap();
        assert!(g.clusters[&cid].signal_event_count > 0);

        g.clusters.get_mut(&cid).unwrap().last_updated =
            Utc::now() - chrono::Duration::hours(8);

        g.prune_stale(std::time::Duration::from_secs(24 * 3600));
        assert_eq!(g.clusters.len(), 1, "Signal cluster should NOT be pruned at 8h with 24h max_age");
    }

    // =========================================================================
    // Multi-source merge tests
    // =========================================================================

    #[test]
    fn test_cross_source_merge_lower_threshold() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Iran",
            "tags": ["topic:airstrike"],
            "region_code": "IR",
            "source_type": "geoconfirmed",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "entity_name": "Iran",
            "tags": ["topic:airstrike"],
            "region_code": "IR",
            "source_type": "rss-news",
        }));
        g.ingest(&e2, None);
        assert_eq!(g.clusters.len(), 1, "Cross-source events with shared entity+topic+region should merge");
        let c = g.clusters.values().next().unwrap();
        assert!(c.source_types.contains(&SourceType::Geoconfirmed));
        assert!(c.source_types.contains(&SourceType::RssNews));
    }

    #[test]
    fn test_same_source_requires_higher_threshold() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "Alpha",
            "region_code": "IR",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "entity_name": "Beta",
            "region_code": "IR",
        }));
        g.ingest(&e2, None);

        assert_eq!(g.clusters.len(), 2, "Same-source events need stronger signals to merge");
    }

    // =========================================================================
    // Enrichment extraction tests
    // =========================================================================

    #[test]
    fn test_enrichment_relationships_extracted() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "payload": {
                "enrichment": {
                    "entities": [
                        {"name": "Iran", "type": "location"},
                        {"name": "Israel", "type": "location"}
                    ],
                    "topics": ["iran-israel-conflict"],
                    "relationships": [
                        {"source": "Iran", "target": "Israel", "type": "rivalry", "confidence": 0.9}
                    ]
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let c = g.clusters.values().next().unwrap();
        assert!(c.entities.contains("iran"));
        assert!(c.entities.contains("israel"));
        assert!(c.topics.contains("iran-israel-conflict"));
    }

    #[test]
    fn test_news_org_entities_filtered() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "payload": {
                "enrichment": {
                    "entities": [
                        {"name": "Reuters", "type": "organization"},
                        {"name": "Deutsche Welle", "type": "organization"},
                        {"name": "Iran", "type": "location"}
                    ],
                    "topics": ["iran-conflict"]
                }
            }
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let c = g.clusters.values().next().unwrap();
        assert!(c.entities.contains("iran"));
    }

    // =========================================================================
    // Parent/child hierarchy tests
    // =========================================================================

    #[test]
    fn test_parent_child_assignment() {
        let mut g = SituationGraph::default();

        for i in 0..10 {
            let e = make_event(json!({
                "entity_name": "Iran",
                "tags": ["topic:iran-conflict", "topic:missiles"],
                "region_code": "IR",
                "source_type": if i % 2 == 0 { "geoconfirmed" } else { "rss-news" },
            }));
            g.ingest(&e, None);
        }
        assert_eq!(g.clusters.len(), 1, "All events should merge into one cluster");

        let e_small = make_event(json!({
            "entity_name": "Israel",
            "tags": ["topic:defense"],
            "region_code": "IL",
            "source_type": "acled",
        }));
        g.ingest(&e_small, None);

        assert!(g.clusters.len() >= 1 && g.clusters.len() <= 2);
    }

    // =========================================================================
    // Edge case tests
    // =========================================================================

    #[test]
    fn test_empty_entity_name_creates_no_cluster() {
        let mut g = SituationGraph::default();
        let event = make_event(json!({
            "region_code": "IR",
            "severity": "high",
        }));
        g.ingest(&event, None);
        assert!(g.clusters.is_empty(), "Event without entities/topics should not create a cluster");
    }

    #[test]
    fn test_entity_normalization_case_insensitive() {
        let mut g = SituationGraph::default();

        let e1 = make_event(json!({
            "entity_name": "IRAN",
            "tags": ["topic:conflict"],
            "source_type": "acled",
        }));
        g.ingest(&e1, None);

        let e2 = make_event(json!({
            "entity_name": "iran",
            "tags": ["topic:conflict"],
            "source_type": "geoconfirmed",
        }));
        g.ingest(&e2, None);
        assert_eq!(g.clusters.len(), 1, "Entity normalization should make IRAN == iran");
    }

    #[test]
    fn test_topic_prefix_stripping() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "tags": ["topic:cyber-attack", "source:gdelt", "query:missiles", "actor:IRGC"],
        }));
        g.ingest(&event, None);
        assert_eq!(g.clusters.len(), 1);
        let c = g.clusters.values().next().unwrap();
        assert!(c.topics.contains("cyber-attack"));
        assert!(!c.topics.contains("gdelt"));
        assert!(!c.topics.contains("missiles"));
        assert!(c.entities.contains("islamic revolutionary guard corps"),
            "Expected 'islamic revolutionary guard corps' in entities: {:?}", c.entities);
    }

    #[test]
    fn test_cluster_event_count_accurate() {
        let mut g = SituationGraph::default();

        for _ in 0..5 {
            g.ingest(&make_event(json!({
                "entity_name": "Target",
                "tags": ["topic:strike"],
                "source_type": "acled",
            })), None);
        }

        assert_eq!(g.clusters.len(), 1);
        let c = g.clusters.values().next().unwrap();
        assert_eq!(c.event_count, 5, "Event count should match number of ingested events");
    }

    #[test]
    fn test_multiple_distinct_clusters() {
        let mut g = SituationGraph::default();

        g.ingest(&make_event(json!({
            "entity_name": "Iran",
            "tags": ["topic:missiles"],
            "region_code": "IR",
        })), None);

        g.ingest(&make_event(json!({
            "entity_name": "Australia",
            "tags": ["topic:weather"],
            "region_code": "AU",
        })), None);

        g.ingest(&make_event(json!({
            "entity_name": "NASA",
            "tags": ["topic:space"],
            "region_code": "US",
        })), None);

        assert_eq!(g.clusters.len(), 3, "Unrelated events should form distinct clusters");
    }

    #[test]
    fn test_cluster_title_includes_entity() {
        let mut g = SituationGraph::default();

        let event = make_event(json!({
            "entity_name": "Ukraine",
            "tags": ["topic:frontlines"],
            "region_code": "UA",
        }));
        g.ingest(&event, None);

        let c = g.clusters.values().next().unwrap();
        assert!(
            c.title.to_lowercase().contains("ukraine"),
            "Auto-generated title should contain the primary entity, got: {}",
            c.title
        );
    }

    #[test]
    fn test_zero_entity_clusters_do_not_merge_on_region_topics() {
        let mut g = SituationGraph::default();
        let now = Utc::now();

        let cid_a = Uuid::new_v4();
        g.clusters.insert(cid_a, SituationCluster {
            id: cid_a,
            title: "Generic Asia News".into(),
            entities: HashSet::new(),
            topics: HashSet::from(["economy".into(), "trade".into(), "security".into()]),
            event_ids: vec![(now, "a1".into())],
            region_codes: HashSet::from(["AS".into()]),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 10,
            signal_event_count: 5,
            source_types: HashSet::from([SourceType::RssNews]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 5,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        let cid_b = Uuid::new_v4();
        g.clusters.insert(cid_b, SituationCluster {
            id: cid_b,
            title: "Asia Economic Concerns".into(),
            entities: HashSet::new(),
            topics: HashSet::from(["economy".into(), "trade".into(), "security".into(), "diplomacy".into()]),
            event_ids: vec![(now, "b1".into())],
            region_codes: HashSet::from(["AS".into()]),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 8,
            signal_event_count: 4,
            source_types: HashSet::from([SourceType::RssNews]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 4,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        for t in ["economy", "trade", "security"] {
            g.topic_index.entry(t.into()).or_default().insert(cid_a);
            g.topic_index.entry(t.into()).or_default().insert(cid_b);
        }
        g.topic_index.entry("diplomacy".into()).or_default().insert(cid_b);

        let merges = g.merge_overlapping(None);
        assert!(
            merges.is_empty(),
            "Zero-entity clusters should NOT merge on region+topics alone: {:?}",
            merges
        );
    }

    #[test]
    fn test_low_content_guard_blocks_sparse_merge() {
        let mut g = SituationGraph::default();
        let now = Utc::now();

        let cid_a = Uuid::new_v4();
        g.clusters.insert(cid_a, SituationCluster {
            id: cid_a,
            title: "Sparse Cluster A".into(),
            entities: HashSet::from(["entity-a".into()]),
            topics: HashSet::from(["topic-x".into()]),
            event_ids: vec![(now, "a1".into())],
            region_codes: HashSet::from(["ME".into()]),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 5,
            signal_event_count: 3,
            source_types: HashSet::from([SourceType::RssNews]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 3,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        let cid_b = Uuid::new_v4();
        g.clusters.insert(cid_b, SituationCluster {
            id: cid_b,
            title: "Sparse Cluster B".into(),
            entities: HashSet::from(["entity-b".into()]),
            topics: HashSet::from(["topic-x".into()]),
            event_ids: vec![(now, "b1".into())],
            region_codes: HashSet::from(["ME".into()]),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 5,
            signal_event_count: 3,
            source_types: HashSet::from([SourceType::Gdelt]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 3,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        g.topic_index.entry("topic-x".into()).or_default().insert(cid_a);
        g.topic_index.entry("topic-x".into()).or_default().insert(cid_b);
        g.entity_index.entry("entity-a".into()).or_default().insert(cid_a);
        g.entity_index.entry("entity-b".into()).or_default().insert(cid_b);

        let merges = g.merge_overlapping(None);
        assert!(
            merges.is_empty(),
            "Low-content clusters (<=2 signals each) should NOT merge without high embedding sim: {:?}",
            merges,
        );
    }

    #[test]
    fn test_embedding_merge_requires_anchor() {
        let mut g = SituationGraph::default();
        let now = Utc::now();

        let cid_a = Uuid::new_v4();
        g.clusters.insert(cid_a, SituationCluster {
            id: cid_a,
            title: "Cluster A".into(),
            entities: HashSet::from(["entity-a".into()]),
            topics: HashSet::from(["topic-a".into(), "topic-b".into(), "topic-c".into()]),
            event_ids: vec![(now, "a1".into())],
            region_codes: HashSet::from(["EU".into()]),
            severity: Severity::Medium,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 10,
            signal_event_count: 5,
            source_types: HashSet::from([SourceType::RssNews, SourceType::Gdelt]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 5,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Developing,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        let cid_b = Uuid::new_v4();
        g.clusters.insert(cid_b, SituationCluster {
            id: cid_b,
            title: "Cluster B".into(),
            entities: HashSet::from(["entity-x".into()]),
            topics: HashSet::from(["topic-x".into(), "topic-y".into(), "topic-z".into()]),
            event_ids: vec![(now, "b1".into())],
            region_codes: HashSet::from(["EU".into()]),
            severity: Severity::Medium,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 10,
            signal_event_count: 5,
            source_types: HashSet::from([SourceType::RssNews, SourceType::Gdelt]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 5,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Developing,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });

        g.entity_index.entry("entity-a".into()).or_default().insert(cid_a);
        g.entity_index.entry("entity-x".into()).or_default().insert(cid_b);
        for t in ["topic-a", "topic-b", "topic-c"] {
            g.topic_index.entry(t.into()).or_default().insert(cid_a);
        }
        for t in ["topic-x", "topic-y", "topic-z"] {
            g.topic_index.entry(t.into()).or_default().insert(cid_b);
        }

        let merges = g.merge_overlapping(None);
        assert!(
            merges.is_empty(),
            "Clusters with no shared entities/topics should NOT merge even in same region: {:?}",
            merges,
        );
    }

    #[test]
    fn test_single_source_leaf_cap() {
        let mut g = SituationGraph::default();

        let cid = Uuid::new_v4();
        let now = Utc::now();
        g.clusters.insert(cid, SituationCluster {
            id: cid,
            title: "Iran Fire Cluster".into(),
            entities: HashSet::from(["iran".into()]),
            topics: HashSet::from(["thermal-anomaly".into()]),
            event_ids: (0..50).map(|i| (now, format!("f{i}"))).collect(),
            region_codes: HashSet::from(["IR".into()]),
            severity: Severity::Medium,
            first_seen: now,
            last_updated: now,
            centroid: Some((51.0, 35.0)),
            event_count: 50,
            signal_event_count: 50,
            source_types: HashSet::from([SourceType::Firms]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: true,
            title_signal_count_at_gen: 50,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Active,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        });
        g.entity_index.entry("iran".into()).or_default().insert(cid);
        g.topic_index.entry("thermal-anomaly".into()).or_default().insert(cid);

        let event = make_event(json!({
            "event_type": "thermal_anomaly",
            "source_type": "firms",
            "entity_name": "Iran",
            "tags": ["topic:thermal-anomaly"],
            "region_code": "IR",
            "latitude": 35.0,
            "longitude": 51.0,
        }));
        let entities = HashSet::from(["iran".into()]);
        let topics = HashSet::from(["thermal-anomaly".into()]);

        let score = g.score_candidate(
            g.clusters.get(&cid).unwrap(),
            &event,
            &entities,
            &topics,
            None,
        );
        assert!(
            score.is_none(),
            "Single-source leaf cluster at 50 events should reject: got {:?}",
            score,
        );
    }

    // =========================================================================
    // K-means and coherence split tests
    // =========================================================================

    #[test]
    fn test_kmeans_2_two_clear_clusters() {
        use merge::kmeans_2;

        // Two well-separated groups in 3D
        let group_a: Vec<Vec<f32>> = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.9, 0.1, 0.0],
            vec![0.95, 0.05, 0.0],
        ];
        let group_b: Vec<Vec<f32>> = vec![
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.9, 0.1],
            vec![0.0, 0.95, 0.05],
        ];
        let mut all = group_a.clone();
        all.extend(group_b.clone());

        let result = kmeans_2(&all, 10);
        assert!(result.is_some(), "Should successfully split two clear clusters");

        let (ga, gb) = result.unwrap();
        assert_eq!(ga.len() + gb.len(), 6, "All points should be assigned");
        assert!(ga.len() >= 3 || gb.len() >= 3, "At least one group should have 3 elements");

        // Verify points from the same original group are assigned together
        let first_group_indices: Vec<usize> = vec![0, 1, 2];
        let second_group_indices: Vec<usize> = vec![3, 4, 5];

        let first_all_in_a = first_group_indices.iter().all(|i| ga.contains(i));
        let first_all_in_b = first_group_indices.iter().all(|i| gb.contains(i));
        assert!(
            first_all_in_a || first_all_in_b,
            "First group should be entirely in one partition"
        );

        let second_all_in_a = second_group_indices.iter().all(|i| ga.contains(i));
        let second_all_in_b = second_group_indices.iter().all(|i| gb.contains(i));
        assert!(
            second_all_in_a || second_all_in_b,
            "Second group should be entirely in one partition"
        );
    }

    #[test]
    fn test_kmeans_2_too_few_embeddings() {
        use merge::kmeans_2;

        let single = vec![vec![1.0, 0.0, 0.0]];
        assert!(kmeans_2(&single, 10).is_none(), "Should return None for single embedding");

        let empty: Vec<Vec<f32>> = vec![];
        assert!(kmeans_2(&empty, 10).is_none(), "Should return None for empty input");
    }

    #[test]
    fn test_kmeans_2_identical_embeddings() {
        use merge::kmeans_2;

        // All identical — k-means should produce a degenerate split (one empty group)
        let all_same: Vec<Vec<f32>> = vec![
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
        ];
        let result = kmeans_2(&all_same, 10);
        // Could be None (degenerate) or Some with an imbalanced split — both are acceptable
        if let Some((ga, gb)) = result {
            assert_eq!(ga.len() + gb.len(), 4);
        }
    }

    #[test]
    fn test_split_by_coherence_basic() {
        let mut g = SituationGraph::default();
        let mut cache = EmbeddingCache::new(10_000, 0.05);

        let now = Utc::now();
        let cluster_id = Uuid::new_v4();

        // Build event_ids referencing embedding keys
        // Group A: conflict-like events (embedding near [1,0,0])
        // Group B: cyber-like events (embedding near [0,1,0])
        let mut event_ids: Vec<(DateTime<Utc>, String)> = Vec::new();

        for i in 0..5 {
            let key = format!("conflict_{i}");
            cache.insert(key.clone(), vec![1.0, 0.05 * i as f32, 0.0]);
            event_ids.push((now, key));
        }
        for i in 0..5 {
            let key = format!("cyber_{i}");
            cache.insert(key.clone(), vec![0.0, 1.0, 0.05 * i as f32]);
            event_ids.push((now, key));
        }

        let cluster = SituationCluster {
            id: cluster_id,
            title: "Mixed cluster".into(),
            entities: HashSet::from(["entity_a".into(), "entity_b".into()]),
            topics: HashSet::from(["conflict".into(), "cyber".into()]),
            event_ids,
            region_codes: HashSet::from(["middle-east".into()]),
            severity: Severity::Medium,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 10,
            signal_event_count: 5,
            source_types: HashSet::from([SourceType::Gdelt, SourceType::Acled]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Developing,
            phase_changed_at: now,
            peak_event_rate: 2.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.5,
            anomaly_score: 0.0,
        };

        g.clusters.insert(cluster_id, cluster);

        let result = g.split_by_coherence(cluster_id, &cache, 3);
        assert!(result.is_some(), "Should split cluster with two clear embedding groups");

        let new_id = result.unwrap();

        // Verify the new cluster exists
        assert!(g.clusters.contains_key(&new_id), "New cluster should exist");
        let new_cluster = g.clusters.get(&new_id).unwrap();

        // New cluster should be a child of the original
        assert_eq!(new_cluster.parent_id, Some(cluster_id));

        // Both clusters should have events
        let original = g.clusters.get(&cluster_id).unwrap();
        assert!(original.event_count > 0, "Original should retain events");
        assert!(new_cluster.event_count > 0, "New cluster should have events");
        assert!(new_cluster.event_count >= 3, "New cluster should have at least min_group_size events");

        // Total events should be preserved
        assert_eq!(
            original.event_count + new_cluster.event_count,
            10,
            "Total events should be preserved"
        );
    }

    #[test]
    fn test_split_by_coherence_too_few_embeddings() {
        let mut g = SituationGraph::default();
        let cache = EmbeddingCache::new(10_000, 0.05);

        let now = Utc::now();
        let cluster_id = Uuid::new_v4();

        // Only 3 events — below the minimum of 6
        let event_ids: Vec<(DateTime<Utc>, String)> = (0..3)
            .map(|i| (now, format!("event_{i}")))
            .collect();

        let cluster = SituationCluster {
            id: cluster_id,
            title: "Small cluster".into(),
            entities: HashSet::from(["entity_a".into()]),
            topics: HashSet::new(),
            event_ids,
            region_codes: HashSet::new(),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 3,
            signal_event_count: 0,
            source_types: HashSet::from([SourceType::Gdelt]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        };

        g.clusters.insert(cluster_id, cluster);

        let result = g.split_by_coherence(cluster_id, &cache, 3);
        assert!(result.is_none(), "Should not split when too few embeddings are available");
    }

    #[test]
    fn test_split_by_coherence_min_group_size_enforced() {
        let mut g = SituationGraph::default();
        let mut cache = EmbeddingCache::new(10_000, 0.05);

        let now = Utc::now();
        let cluster_id = Uuid::new_v4();

        // 8 events: 7 in one direction, 1 in another — split would produce a group of 1
        let mut event_ids: Vec<(DateTime<Utc>, String)> = Vec::new();
        for i in 0..7 {
            let key = format!("majority_{i}");
            cache.insert(key.clone(), vec![1.0, 0.01 * i as f32, 0.0]);
            event_ids.push((now, key));
        }
        let outlier_key = "outlier_0".to_string();
        cache.insert(outlier_key.clone(), vec![0.0, 0.0, 1.0]);
        event_ids.push((now, outlier_key));

        let cluster = SituationCluster {
            id: cluster_id,
            title: "Lopsided cluster".into(),
            entities: HashSet::from(["entity_a".into()]),
            topics: HashSet::new(),
            event_ids,
            region_codes: HashSet::new(),
            severity: Severity::Low,
            first_seen: now,
            last_updated: now,
            centroid: None,
            event_count: 8,
            signal_event_count: 0,
            source_types: HashSet::from([SourceType::Gdelt]),
            parent_id: None,
            event_titles: vec![],
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: now,
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: SituationPhase::Emerging,
            phase_changed_at: now,
            peak_event_rate: 0.0,
            peak_rate_at: now,
            phase_transitions: vec![],
            certainty: 0.0,
            anomaly_score: 0.0,
        };

        g.clusters.insert(cluster_id, cluster);

        // min_group_size=3 — the outlier group of 1 should not pass
        let result = g.split_by_coherence(cluster_id, &cache, 3);
        assert!(result.is_none(), "Should not split when smaller group is below min_group_size");
    }
}
