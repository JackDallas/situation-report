//! Configuration for the Situation Report pipeline.
//!
//! All tunable parameters in one place. Every field has a sensible default
//! matching the current production behavior. Values can be overridden via
//! environment variables with a `PIPELINE_` prefix.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level pipeline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub scoring: ScoringConfig,
    pub merge: MergeConfig,
    pub cluster_caps: ClusterCapsConfig,
    pub phases: PhaseConfig,
    pub temporal_decay: TemporalDecayConfig,
    pub geo: GeoConfig,
    pub quality: QualityConfig,
    pub intervals: IntervalConfig,
    pub search: SearchConfig,
    pub backoff: BackoffConfig,
    pub idf: IdfConfig,
    pub burst: BurstConfig,
    pub sweep: SweepConfig,
    #[serde(default)]
    pub certainty: CertaintyConfig,
    #[serde(default)]
    pub severity: SeverityConfig,
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// Max contribution from entity IDF matches.
    pub entity_score_cap: i32,
    /// Max contribution from topic IDF matches.
    pub topic_score_cap: i32,
    /// Max burst anomaly bonus per topic.
    pub burst_bonus_max: f64,
    /// CUSUM change-point detection threshold.
    pub cusum_threshold: f64,
    /// Bonus for events within inner geo radius (radius * 0.5).
    pub geo_inner_bonus: i32,
    /// Bonus for events within outer geo radius.
    pub geo_outer_bonus: i32,
    /// Region match bonus.
    pub region_bonus: i32,
    /// Penalty for single-source clusters.
    pub single_source_penalty: i32,
    /// Vector similarity hard floor — reject below this (when soft gate disabled).
    pub vector_hard_gate: f64,
    /// Use soft penalty below gate instead of hard rejection.
    /// sim=0.35 → -0.5 penalty, sim=0.20 → -2.0, sim=0.0 → -4.0 (proportional, not a cliff).
    pub vector_soft_gate: bool,
    /// Multiplier for soft gate penalty: `score -= ((gate - sim) * multiplier).round()`.
    pub vector_soft_gate_multiplier: f64,
    /// Vector similarity tier thresholds and bonuses: (threshold, bonus).
    /// Applied in reverse order (highest first). Default: [(0.75, 3), (0.65, 2), (0.55, 1)].
    pub vector_tiers: Vec<(f64, i32)>,
    /// Title Jaccard similarity tiers: (threshold, bonus).
    /// Default: [(0.6, 3), (0.4, 2), (0.25, 1)].
    pub title_jaccard_tiers: Vec<(f64, i32)>,
    /// Size penalty base event count (penalty starts here).
    pub size_penalty_start: usize,
    /// Size penalty step (events per additional -1 penalty tier).
    pub size_penalty_step: usize,
    /// Size penalty max (most negative penalty before blocking).
    pub size_penalty_max: i32,
    /// Size at which merge is blocked entirely.
    /// Set to `None` to disable the hard block (use smooth penalty instead).
    pub size_block_at: Option<usize>,
    /// Smooth size penalty: use `-ln(1 + count/divisor)` instead of stepped tiers.
    /// When true, `size_penalty_start/step/max/block_at` are ignored.
    pub smooth_size_penalty: bool,
    /// Divisor for smooth size penalty: `-ln(1 + event_count / divisor)`.
    pub smooth_size_divisor: f64,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            entity_score_cap: 5,
            topic_score_cap: 4,
            burst_bonus_max: 2.0,
            cusum_threshold: 5.0,
            geo_inner_bonus: 2,
            geo_outer_bonus: 1,
            region_bonus: 1,
            single_source_penalty: -1,
            vector_hard_gate: 0.40,
            vector_soft_gate: true,
            vector_soft_gate_multiplier: 10.0,
            vector_tiers: vec![(0.75, 3), (0.65, 2), (0.55, 1)],
            title_jaccard_tiers: vec![(0.6, 3), (0.4, 2), (0.25, 1)],
            size_penalty_start: 20,
            size_penalty_step: 20,
            size_penalty_max: -3,
            size_block_at: None,
            smooth_size_penalty: true,
            smooth_size_divisor: 50.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConfig {
    /// Per-source same-source merge thresholds. Key is source_type string.
    pub same_source_thresholds: HashMap<String, i32>,
    /// Default same-source merge threshold.
    pub same_source_default: i32,
    /// Per-source cross-source merge thresholds.
    pub cross_source_thresholds: HashMap<String, i32>,
    /// Default cross-source merge threshold.
    pub cross_source_default: i32,

    // -- Merge overlapping conditions --
    /// Semantic similarity standalone merge threshold.
    pub semantic_threshold: f64,
    /// Semantic similarity + region merge threshold.
    pub semantic_region_threshold: f64,
    /// Topic Jaccard standalone merge threshold.
    pub topic_jaccard_threshold: f64,
    /// Min shared entities for standalone entity-based merge.
    pub entity_threshold: usize,
    /// Min shared topic IDF for topic-based merge conditions.
    pub min_topic_idf: f64,
    /// Min topic Jaccard for IDF-gated merge conditions.
    pub min_topic_jaccard: f64,
    /// Title Jaccard threshold for merge overlap detection.
    pub title_jaccard_merge: f64,
    /// Min shared topics for topic-based merge conditions.
    pub min_shared_topics_news: usize,
    /// Min shared topics for cross-source regional merge.
    pub min_shared_topics_region: usize,

    // -- Merge-overlapping fast-path thresholds --
    /// Title Jaccard threshold for the title-identity merge fast path.
    /// Pairs above this (with region overlap) merge without embedding check.
    pub title_identity_threshold: f64,
    /// Heuristic fallback: title Jaccard threshold when embeddings are unavailable.
    pub heuristic_title_threshold: f64,
    /// Semantic similarity threshold for the entity-empty guard.
    /// Both clusters have zero entities: require at least this cosine similarity to proceed.
    pub entity_empty_semantic_threshold: f64,
    /// Semantic similarity threshold for the low-content guard.
    /// Both clusters have <=2 signals: require at least this cosine similarity.
    pub low_content_semantic_threshold: f64,
    /// Regional absorb: max event count for the smaller cluster.
    pub regional_absorb_max_smaller: usize,
    /// Regional absorb: min event count for the larger cluster.
    pub regional_absorb_min_larger: usize,

    // -- Vector-primary merge --
    /// Use vector (embedding) cosine similarity as the primary merge signal.
    /// When false, uses the legacy conditional matrix. Default: true.
    #[serde(default = "default_true")]
    pub use_vector_primary_merge: bool,
    /// Vector-primary: merge threshold for cross-region pairs (higher = stricter).
    pub vector_threshold_cross_region: f64,
    /// Vector-primary: merge threshold for news-only pairs.
    pub vector_threshold_news_only: f64,
    /// Vector-primary: merge threshold for default pairs.
    pub vector_threshold_default: f64,
    /// Vector-primary: boost when shared_entities >= 2.
    pub vector_boost_entities_2: f64,
    /// Vector-primary: boost when shared_entities == 1 and shared_region.
    pub vector_boost_entity_region: f64,
    /// Vector-primary: boost when shared_region.
    pub vector_boost_region: f64,
    /// Vector-primary: boost when cluster_titles_similar >= 0.6.
    pub vector_boost_title_similar: f64,
}

fn default_true() -> bool {
    true
}

impl Default for MergeConfig {
    fn default() -> Self {
        let mut same = HashMap::new();
        same.insert("gdelt".into(), 5);
        same.insert("rss_news".into(), 5);
        same.insert("geoconfirmed".into(), 5);
        same.insert("gdelt_geo".into(), 5);
        same.insert("firms".into(), 4);

        let mut cross = HashMap::new();
        cross.insert("gdelt".into(), 4);
        cross.insert("rss_news".into(), 4);

        Self {
            same_source_thresholds: same,
            same_source_default: 4,
            cross_source_thresholds: cross,
            cross_source_default: 3,
            semantic_threshold: 0.75,
            semantic_region_threshold: 0.65,
            topic_jaccard_threshold: 0.5,
            entity_threshold: 2,
            min_topic_idf: 4.0,
            min_topic_jaccard: 0.15,
            title_jaccard_merge: 0.3,
            min_shared_topics_news: 2,
            min_shared_topics_region: 3,
            title_identity_threshold: 0.60,
            heuristic_title_threshold: 0.50,
            entity_empty_semantic_threshold: 0.75,
            low_content_semantic_threshold: 0.80,
            regional_absorb_max_smaller: 20,
            regional_absorb_min_larger: 50,
            use_vector_primary_merge: true,
            vector_threshold_cross_region: 0.80,
            vector_threshold_news_only: 0.70,
            vector_threshold_default: 0.65,
            vector_boost_entities_2: 0.10,
            vector_boost_entity_region: 0.05,
            vector_boost_region: 0.03,
            vector_boost_title_similar: 0.05,
        }
    }
}

// ---------------------------------------------------------------------------
// Cluster caps
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterCapsConfig {
    /// Max entities tracked per cluster.
    pub max_entities: usize,
    /// Max topics tracked per cluster.
    pub max_topics: usize,
    /// Max event_ids stored per cluster (oldest trimmed).
    pub max_event_ids: usize,
    /// Max event titles stored for AI context.
    pub max_event_titles: usize,
    /// Hard cap on leaf cluster event count (parents exempt).
    pub leaf_cluster_hard_cap: usize,
    /// Max children per parent cluster.
    pub max_children_per_parent: usize,
    /// Max total events across parent + children.
    pub max_events_per_parent: usize,
    /// Max entities extracted per event.
    pub max_entities_per_event: usize,
    /// Max enrichment topics taken per event.
    pub max_enrichment_topics: usize,
}

impl Default for ClusterCapsConfig {
    fn default() -> Self {
        Self {
            max_entities: 50,
            max_topics: 30,
            max_event_ids: 500,
            max_event_titles: 30,
            leaf_cluster_hard_cap: 500,
            max_children_per_parent: 15,
            max_events_per_parent: 1000,
            max_entities_per_event: 5,
            max_enrichment_topics: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseConfig {
    // Emerging → Developing
    pub emerging_min_events: usize,
    pub emerging_min_sources: usize,
    pub emerging_stale_hours: f64,

    // Developing → Active
    pub developing_velocity_threshold: usize,
    pub developing_signal_count: usize,

    // Active → Declining
    pub active_decline_rate_ratio: f64,
    pub active_decline_gap_factor: f64,
    pub active_decline_no_activity_hours: f64,
    pub active_min_dwell_hours: f64,
    pub active_recent_guard_hours: f64,
    pub critical_dwell_hours: f64,
    pub high_dwell_hours: f64,

    // Declining → Resolved
    pub declining_resolve_gap_factor: f64,

    // Resolved → Historical
    pub resolved_historical_gap_factor: f64,

    // Gap tolerance base by severity (hours)
    pub gap_tolerance_critical_hours: f64,
    pub gap_tolerance_high_hours: f64,
    pub gap_tolerance_medium_hours: f64,
    pub gap_tolerance_low_hours: f64,

    // Gap tolerance ceilings
    pub gap_tolerance_max_hours: f64,
    pub gap_tolerance_single_source_max_hours: f64,

    // News-only stale check
    pub news_stale_hours: i64,

    // Peak rate decay half-life (minutes)
    pub peak_decay_half_life_mins: f64,
}

impl Default for PhaseConfig {
    fn default() -> Self {
        Self {
            emerging_min_events: 3,
            emerging_min_sources: 2,
            emerging_stale_hours: 6.0,
            developing_velocity_threshold: 5,
            developing_signal_count: 3,
            active_decline_rate_ratio: 0.3,
            active_decline_gap_factor: 0.5,
            active_decline_no_activity_hours: 4.0,
            active_min_dwell_hours: 6.0,
            active_recent_guard_hours: 4.0,
            critical_dwell_hours: 24.0,
            high_dwell_hours: 12.0,
            declining_resolve_gap_factor: 1.5,
            resolved_historical_gap_factor: 3.0,
            gap_tolerance_critical_hours: 12.0,
            gap_tolerance_high_hours: 8.0,
            gap_tolerance_medium_hours: 4.0,
            gap_tolerance_low_hours: 2.0,
            gap_tolerance_max_hours: 24.0,
            gap_tolerance_single_source_max_hours: 6.0,
            news_stale_hours: 72,
            peak_decay_half_life_mins: 30.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Temporal decay (per event type)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalDecayConfig {
    /// (half_life_hours, offset_hours) per event type.
    pub per_type: HashMap<String, (f64, f64)>,
    pub default_half_life_hours: f64,
    pub default_offset_hours: f64,
}

impl Default for TemporalDecayConfig {
    fn default() -> Self {
        let mut per_type = HashMap::new();
        per_type.insert("conflict_event".into(), (4.0, 1.0));
        per_type.insert("thermal_anomaly".into(), (6.0, 1.5));
        per_type.insert("seismic_event".into(), (8.0, 2.0));
        per_type.insert("gps_interference".into(), (8.0, 2.0));
        per_type.insert("news_article".into(), (24.0, 6.0));
        per_type.insert("geo_news".into(), (24.0, 6.0));
        per_type.insert("internet_outage".into(), (24.0, 6.0));
        per_type.insert("bgp_anomaly".into(), (24.0, 6.0));
        per_type.insert("nuclear_event".into(), (48.0, 12.0));
        Self {
            per_type,
            default_half_life_hours: 12.0,
            default_offset_hours: 3.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Geo radii (per event type)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoConfig {
    /// Clustering radius in km per event type.
    pub per_type: HashMap<String, f64>,
    pub default_radius_km: f64,
}

impl Default for GeoConfig {
    fn default() -> Self {
        let mut per_type = HashMap::new();
        per_type.insert("conflict_event".into(), 50.0);
        per_type.insert("thermal_anomaly".into(), 25.0);
        per_type.insert("gps_interference".into(), 300.0);
        per_type.insert("seismic_event".into(), 100.0);
        per_type.insert("news_article".into(), 100.0);
        per_type.insert("geo_news".into(), 100.0);
        Self {
            per_type,
            default_radius_km: 150.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Quality gates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityConfig {
    /// Min events for a standalone cluster to appear in active_clusters().
    pub min_events_standalone: usize,
    /// Min events for a child cluster to appear in active_clusters().
    pub min_events_child: usize,
    /// Min events before requesting AI title.
    pub min_events_for_title: usize,
    /// Min events for child before requesting AI title.
    pub min_events_child_title: usize,
    /// New signal events required to trigger AI title re-generation.
    pub signal_events_for_retitle: usize,
    /// Min events for a cluster to be eligible for web search.
    pub min_events_for_search: usize,
    /// Seismic magnitude threshold for importance filter.
    pub seismic_importance_threshold: f64,
    /// Medium standalone penalty (extra min_events required).
    pub medium_standalone_penalty: u32,
    /// Incoherent topic threshold (topic count that triggers severity cap + certainty penalty).
    pub incoherent_topic_threshold: usize,
    /// Incoherent topic certainty penalty multiplier.
    pub incoherent_topic_penalty: f64,
    /// Natural disaster standalone cap.
    pub nd_standalone_cap: usize,
    /// Natural disaster parent cap.
    pub nd_parent_cap: usize,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            min_events_standalone: 15,
            min_events_child: 3,
            min_events_for_title: 3,
            min_events_child_title: 2,
            signal_events_for_retitle: 20,
            min_events_for_search: 3,
            seismic_importance_threshold: 4.0,
            medium_standalone_penalty: 8,
            incoherent_topic_threshold: 12,
            incoherent_topic_penalty: 0.6,
            nd_standalone_cap: 2,
            nd_parent_cap: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Intervals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntervalConfig {
    /// Situation publish/persist interval (seconds).
    pub situation_publish_secs: u64,
    /// Prune stale clusters interval (seconds).
    pub prune_interval_secs: u64,
    /// Sweep (coherence/metadata/oversized/orphan) interval (seconds).
    pub sweep_interval_secs: u64,
    /// Summary digest interval (seconds).
    pub summary_interval_secs: u64,
    /// Analysis check interval (seconds).
    pub analysis_check_secs: u64,
    /// Alert rules refresh interval (seconds).
    pub alert_refresh_secs: u64,
    /// Re-ingest sweep interval (seconds).
    pub reingest_sweep_secs: u64,
    /// Ollama health check interval (seconds).
    pub ollama_health_check_secs: u64,
    /// Max age for stale cluster pruning (hours).
    pub prune_max_age_hours: u64,
    /// Correlation window duration (hours).
    pub correlation_window_hours: u64,
    /// Backfill on startup (hours of history to load).
    pub backfill_hours: u64,
    /// Max events to load during backfill.
    pub backfill_max_events: i64,
    /// Max narratives generated per 30s tick.
    pub narrative_max_per_tick: usize,
    /// Max active incident age (hours) before pruning.
    pub incident_max_age_hours: i64,
    /// SSE broadcast channel buffer size.
    pub broadcast_channel_size: usize,
    /// Embedding worker inbound channel size.
    pub embed_channel_size: usize,
    /// Embedding result return channel size.
    pub embed_result_channel_size: usize,
    /// Search interval for critical/high clusters (minutes).
    pub search_interval_hot_mins: i64,
    /// Search interval for medium/low clusters (hours).
    pub search_interval_cool_hours: i64,
}

impl Default for IntervalConfig {
    fn default() -> Self {
        Self {
            situation_publish_secs: 30,
            prune_interval_secs: 60,
            sweep_interval_secs: 120,
            summary_interval_secs: 5,
            analysis_check_secs: 60,
            alert_refresh_secs: 120,
            reingest_sweep_secs: 10,
            ollama_health_check_secs: 300,
            prune_max_age_hours: 6,
            correlation_window_hours: 6,
            backfill_hours: 6,
            backfill_max_events: 5000,
            narrative_max_per_tick: 3,
            incident_max_age_hours: 1,
            broadcast_channel_size: 1024,
            embed_channel_size: 1024,
            embed_result_channel_size: 256,
            search_interval_hot_mins: 30,
            search_interval_cool_hours: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Search (Exa)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Max Exa requests per day.
    pub daily_cap: u32,
    /// Max Exa requests per hour.
    pub hourly_cap: u64,
    /// Min seconds between searches.
    pub cooldown_secs: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            daily_cap: 1_400,
            hourly_cap: 60,
            cooldown_secs: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Backoff (registry)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Base reconnect delay for streaming sources (seconds).
    pub stream_reconnect_base_secs: u64,
    /// Max streaming reconnect backoff (seconds).
    pub stream_max_backoff_secs: u64,
    /// Additive delay per consecutive 429 for poll sources (seconds).
    pub poll_429_additive_secs: u64,
    /// Max 429 backoff (seconds).
    pub poll_429_max_backoff_secs: u64,
    /// Base backoff for non-429 poll errors (seconds).
    pub poll_error_base_secs: u64,
    /// Max exponent for error backoff (base * 2^max_exp).
    pub poll_error_max_exponent: u32,
    /// Max non-429 backoff (seconds).
    pub poll_error_max_secs: u64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            stream_reconnect_base_secs: 10,
            stream_max_backoff_secs: 300,
            poll_429_additive_secs: 30,
            poll_429_max_backoff_secs: 600,
            poll_error_base_secs: 30,
            poll_error_max_exponent: 4,
            poll_error_max_secs: 300,
        }
    }
}

// ---------------------------------------------------------------------------
// IDF
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdfConfig {
    /// Exponential decay factor per observation (~0.9999 = half-life ~7000 events).
    pub decay_factor: f64,
    /// Min total for IDF baseline at startup.
    pub min_total: f64,
    /// Cleanup negligible entries every N observations.
    pub cleanup_interval: u64,
    /// Remove entries with freq below this threshold during cleanup.
    pub cleanup_threshold: f64,
    /// Min IDF clamp for entity scoring.
    pub min_entity_idf: f64,
    /// Max IDF clamp for entity scoring.
    pub max_entity_idf: f64,
    /// Min IDF clamp for topic scoring.
    pub min_topic_idf: f64,
    /// Max IDF clamp for topic scoring.
    pub max_topic_idf: f64,
}

impl Default for IdfConfig {
    fn default() -> Self {
        Self {
            decay_factor: 0.9999,
            min_total: 100.0,
            cleanup_interval: 1000,
            cleanup_threshold: 0.01,
            min_entity_idf: 1.0,
            max_entity_idf: 5.0,
            min_topic_idf: 1.0,
            max_topic_idf: 7.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Burst detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstConfig {
    /// Short EWMA half-life (seconds). Default: 300 (5 min).
    pub short_half_life_secs: f64,
    /// Long EWMA half-life (seconds). Default: 21600 (6 hr).
    pub long_half_life_secs: f64,
    /// Cleanup threshold for EWMA values.
    pub cleanup_threshold: f64,
    /// Cleanup every N observations.
    pub cleanup_interval: u64,
    /// Burst ratio thresholds: (ratio, anomaly_score). Applied in order.
    pub ratio_tiers: Vec<(f64, f64)>,
}

impl Default for BurstConfig {
    fn default() -> Self {
        Self {
            short_half_life_secs: 300.0,
            long_half_life_secs: 21600.0,
            cleanup_threshold: 0.001,
            cleanup_interval: 1000,
            ratio_tiers: vec![
                (3.0, 1.0),
                (2.0, 0.75),
                (1.5, 0.5),
                (1.0, 0.25),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Sweep (NEW — periodic cleanup)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepConfig {
    /// Min intra-cluster cosine similarity for coherence check.
    pub coherence_min: f64,
    /// Number of recent events to sample for coherence checks.
    pub coherence_sample_size: usize,
    /// Max entities to keep per cluster after metadata pruning.
    pub entity_max_after_prune: usize,
    /// Max topics to keep per cluster after metadata pruning.
    pub topic_max_after_prune: usize,
    /// Shed events from clusters above this count.
    pub shed_above_events: usize,
    /// Target event count after shedding.
    pub shed_target_events: usize,
    /// Min events for a child to survive orphan cleanup.
    pub orphan_min_events: usize,
    /// Min age (seconds) before orphan cleanup applies.
    pub orphan_min_age_secs: u64,
    /// Min events for coherence checking.
    pub coherence_min_events: usize,
    /// Automatically split clusters that fail the coherence check.
    pub coherence_auto_split: bool,
    /// Minimum group size after k-means split — smaller groups are rejected.
    pub coherence_split_min_group: usize,
    /// Max seconds a pending event stays in the noise buffer before being discarded.
    pub noise_buffer_secs: u64,
    /// Max number of events in the noise buffer. Oldest evicted when full.
    pub noise_buffer_max: usize,
    /// Min fraction of qualifying children that must be at a severity level for the
    /// parent to adopt it via propagation. Default: 0.34 (at least 1/3).
    pub severity_propagation_threshold: f32,
    /// Allow parent severity to decrease when children resolve/decline. Default: true.
    pub severity_propagation_allow_decrease: bool,
    /// Min distinct topic prefixes to trigger a topic-diversity split.
    pub topic_diversity_split_threshold: usize,

    // --- Split-divergent thresholds ---

    /// Min event count for a standalone cluster to be considered for split_divergent.
    pub split_divergent_min_events: usize,
    /// Min entity count for entity subgroup splitting in split_divergent.
    pub split_divergent_min_entities: usize,
    /// Max overlap ratio (largest_group / total_entities) for a split to be viable.
    pub split_divergent_max_overlap: f64,

    // --- Retroactive sweep: link old events to current situations ---

    /// Interval (seconds) between retroactive sweep ticks.
    pub retro_sweep_interval_secs: u64,
    /// How far back (hours) before a cluster's first_seen to search for linkable events.
    pub retro_sweep_lookback_hours: i32,
    /// Max events to link per cluster per sweep tick.
    pub retro_sweep_max_per_cluster: i64,
    /// Max clusters to sweep per tick (prioritized by severity + staleness).
    pub retro_sweep_max_clusters: usize,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            coherence_min: 0.55,
            coherence_sample_size: 20,
            entity_max_after_prune: 25,
            topic_max_after_prune: 15,
            shed_above_events: 200,
            shed_target_events: 150,
            orphan_min_events: 2,
            orphan_min_age_secs: 3600,
            coherence_min_events: 10,
            coherence_auto_split: true,
            coherence_split_min_group: 3,
            noise_buffer_secs: 300,
            noise_buffer_max: 200,
            severity_propagation_threshold: 0.34,
            severity_propagation_allow_decrease: false,
            topic_diversity_split_threshold: 25,
            split_divergent_min_events: 30,
            split_divergent_min_entities: 4,
            split_divergent_max_overlap: 0.7,
            retro_sweep_interval_secs: 300,
            retro_sweep_lookback_hours: 48,
            retro_sweep_max_per_cluster: 30,
            retro_sweep_max_clusters: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// Severity thresholds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityConfig {
    /// Minimum events for CRITICAL severity (active conflict with multi-source corroboration).
    pub critical_min_events: usize,
    /// Minimum source diversity for CRITICAL severity.
    pub critical_min_sources: usize,
    /// Minimum events for HIGH severity (developing/active conflict or multi-source cyber/disaster).
    pub high_min_events: usize,
    /// Minimum source diversity for MEDIUM severity (corroborated situation).
    pub medium_min_sources: usize,
    /// Minimum events for MEDIUM severity (3+ independent sources).
    pub medium_min_events: usize,
}

impl Default for SeverityConfig {
    fn default() -> Self {
        Self {
            critical_min_events: 40,
            critical_min_sources: 4,
            high_min_events: 10,
            medium_min_sources: 2,
            medium_min_events: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// Certainty (sigmoid-based scoring)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertaintyConfig {
    /// Base certainty for any cluster (floor).
    pub base: f32,
    /// Source diversity sigmoid: max contribution.
    pub source_max: f32,
    /// Source diversity sigmoid: steepness.
    pub source_steepness: f32,
    /// Source diversity sigmoid: midpoint (number of source types).
    pub source_midpoint: f32,
    /// Event count sigmoid: max contribution.
    pub event_max: f32,
    /// Event count sigmoid: steepness.
    pub event_steepness: f32,
    /// Event count sigmoid: midpoint (number of events).
    pub event_midpoint: f32,
    /// Entity presence: max contribution (exponential saturation).
    pub entity_max: f32,
    /// Entity presence: rate of saturation.
    pub entity_rate: f32,
    /// AI title bonus.
    pub ai_title_bonus: f32,
}

impl Default for CertaintyConfig {
    fn default() -> Self {
        Self {
            base: 0.10,
            source_max: 0.40,
            source_steepness: 1.5,
            source_midpoint: 2.0,
            event_max: 0.30,
            event_steepness: 0.3,
            event_midpoint: 10.0,
            entity_max: 0.15,
            entity_rate: 0.5,
            ai_title_bonus: 0.05,
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineConfig impl
// ---------------------------------------------------------------------------

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            scoring: ScoringConfig::default(),
            merge: MergeConfig::default(),
            cluster_caps: ClusterCapsConfig::default(),
            phases: PhaseConfig::default(),
            temporal_decay: TemporalDecayConfig::default(),
            geo: GeoConfig::default(),
            quality: QualityConfig::default(),
            intervals: IntervalConfig::default(),
            search: SearchConfig::default(),
            backoff: BackoffConfig::default(),
            idf: IdfConfig::default(),
            burst: BurstConfig::default(),
            sweep: SweepConfig::default(),
            certainty: CertaintyConfig::default(),
            severity: SeverityConfig::default(),
        }
    }
}

impl PipelineConfig {
    /// Load config from environment variables, falling back to defaults.
    ///
    /// If `PIPELINE_CONFIG_JSON` is set, it is parsed as a full or partial
    /// PipelineConfig JSON object that overrides defaults.
    /// Individual env vars with the `PIPELINE_` prefix override specific fields.
    pub fn from_env() -> Self {
        // Start with defaults
        let mut config = if let Ok(json) = std::env::var("PIPELINE_CONFIG_JSON") {
            match serde_json::from_str::<PipelineConfig>(&json) {
                Ok(c) => {
                    tracing::info!("Loaded pipeline config from PIPELINE_CONFIG_JSON");
                    c
                }
                Err(e) => {
                    tracing::warn!("Failed to parse PIPELINE_CONFIG_JSON: {e}, using defaults");
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        // Override individual fields from env vars
        macro_rules! env_override {
            ($field:expr, $var:expr, $ty:ty) => {
                if let Ok(val) = std::env::var($var) {
                    if let Ok(parsed) = val.parse::<$ty>() {
                        $field = parsed;
                    } else {
                        tracing::warn!(
                            "Failed to parse {} = {:?} as {}",
                            $var,
                            val,
                            stringify!($ty)
                        );
                    }
                }
            };
        }

        // Scoring
        env_override!(config.scoring.entity_score_cap, "PIPELINE_SCORING_ENTITY_SCORE_CAP", i32);
        env_override!(config.scoring.topic_score_cap, "PIPELINE_SCORING_TOPIC_SCORE_CAP", i32);
        env_override!(config.scoring.vector_hard_gate, "PIPELINE_SCORING_VECTOR_HARD_GATE", f64);

        // Quality
        env_override!(config.quality.min_events_standalone, "PIPELINE_QUALITY_MIN_EVENTS_STANDALONE", usize);
        env_override!(config.quality.min_events_child, "PIPELINE_QUALITY_MIN_EVENTS_CHILD", usize);
        env_override!(config.quality.signal_events_for_retitle, "PIPELINE_QUALITY_SIGNAL_EVENTS_RETITLE", usize);
        env_override!(config.quality.medium_standalone_penalty, "PIPELINE_QUALITY_MEDIUM_STANDALONE_PENALTY", u32);
        env_override!(config.quality.incoherent_topic_threshold, "PIPELINE_QUALITY_INCOHERENT_TOPIC_THRESHOLD", usize);
        env_override!(config.quality.incoherent_topic_penalty, "PIPELINE_QUALITY_INCOHERENT_TOPIC_PENALTY", f64);
        env_override!(config.quality.nd_standalone_cap, "PIPELINE_QUALITY_ND_STANDALONE_CAP", usize);
        env_override!(config.quality.nd_parent_cap, "PIPELINE_QUALITY_ND_PARENT_CAP", usize);

        // Merge
        env_override!(config.merge.semantic_threshold, "PIPELINE_MERGE_SEMANTIC_THRESHOLD", f64);
        env_override!(config.merge.semantic_region_threshold, "PIPELINE_MERGE_SEMANTIC_REGION_THRESHOLD", f64);
        env_override!(config.merge.entity_threshold, "PIPELINE_MERGE_ENTITY_THRESHOLD", usize);
        env_override!(config.merge.title_identity_threshold, "PIPELINE_MERGE_TITLE_IDENTITY_THRESHOLD", f64);
        env_override!(config.merge.heuristic_title_threshold, "PIPELINE_MERGE_HEURISTIC_TITLE_THRESHOLD", f64);
        env_override!(config.merge.entity_empty_semantic_threshold, "PIPELINE_MERGE_ENTITY_EMPTY_SEMANTIC", f64);
        env_override!(config.merge.low_content_semantic_threshold, "PIPELINE_MERGE_LOW_CONTENT_SEMANTIC", f64);
        env_override!(config.merge.regional_absorb_max_smaller, "PIPELINE_MERGE_REGIONAL_ABSORB_MAX_SMALLER", usize);
        env_override!(config.merge.regional_absorb_min_larger, "PIPELINE_MERGE_REGIONAL_ABSORB_MIN_LARGER", usize);
        env_override!(config.merge.use_vector_primary_merge, "PIPELINE_MERGE_USE_VECTOR_PRIMARY", bool);
        env_override!(config.merge.vector_threshold_cross_region, "PIPELINE_MERGE_VECTOR_CROSS_REGION", f64);
        env_override!(config.merge.vector_threshold_news_only, "PIPELINE_MERGE_VECTOR_NEWS_ONLY", f64);
        env_override!(config.merge.vector_threshold_default, "PIPELINE_MERGE_VECTOR_DEFAULT", f64);

        // Cluster caps
        env_override!(config.cluster_caps.max_children_per_parent, "PIPELINE_CAPS_MAX_CHILDREN", usize);
        env_override!(config.cluster_caps.max_events_per_parent, "PIPELINE_CAPS_MAX_EVENTS_PARENT", usize);
        env_override!(config.cluster_caps.leaf_cluster_hard_cap, "PIPELINE_CAPS_LEAF_HARD_CAP", usize);

        // Intervals
        env_override!(config.intervals.sweep_interval_secs, "PIPELINE_SWEEP_INTERVAL_SECS", u64);
        env_override!(config.intervals.narrative_max_per_tick, "PIPELINE_NARRATIVE_MAX_PER_TICK", usize);
        env_override!(config.intervals.broadcast_channel_size, "PIPELINE_BROADCAST_CHANNEL_SIZE", usize);

        // Search
        env_override!(config.search.daily_cap, "PIPELINE_SEARCH_DAILY_CAP", u32);
        env_override!(config.search.hourly_cap, "PIPELINE_SEARCH_HOURLY_CAP", u64);

        // Sweep
        env_override!(config.sweep.coherence_min, "PIPELINE_SWEEP_COHERENCE_MIN", f64);
        env_override!(config.sweep.shed_above_events, "PIPELINE_SWEEP_SHED_ABOVE", usize);
        env_override!(config.sweep.topic_diversity_split_threshold, "PIPELINE_SWEEP_TOPIC_DIVERSITY_THRESHOLD", usize);
        env_override!(config.sweep.split_divergent_min_events, "PIPELINE_SWEEP_SPLIT_MIN_EVENTS", usize);
        env_override!(config.sweep.split_divergent_min_entities, "PIPELINE_SWEEP_SPLIT_MIN_ENTITIES", usize);
        env_override!(config.sweep.split_divergent_max_overlap, "PIPELINE_SWEEP_SPLIT_MAX_OVERLAP", f64);

        // Severity
        env_override!(config.severity.critical_min_events, "PIPELINE_SEVERITY_CRITICAL_MIN_EVENTS", usize);
        env_override!(config.severity.critical_min_sources, "PIPELINE_SEVERITY_CRITICAL_MIN_SOURCES", usize);
        env_override!(config.severity.high_min_events, "PIPELINE_SEVERITY_HIGH_MIN_EVENTS", usize);
        env_override!(config.severity.medium_min_sources, "PIPELINE_SEVERITY_MEDIUM_MIN_SOURCES", usize);
        env_override!(config.severity.medium_min_events, "PIPELINE_SEVERITY_MEDIUM_MIN_EVENTS", usize);

        config
    }

    /// Get temporal decay parameters for an event type.
    pub fn decay_params(&self, event_type: &str) -> (f64, f64) {
        self.temporal_decay
            .per_type
            .get(event_type)
            .copied()
            .unwrap_or((
                self.temporal_decay.default_half_life_hours,
                self.temporal_decay.default_offset_hours,
            ))
    }

    /// Get geo radius for an event type in km.
    pub fn geo_radius_km(&self, event_type: &str) -> f64 {
        self.geo
            .per_type
            .get(event_type)
            .copied()
            .unwrap_or(self.geo.default_radius_km)
    }

    /// Get same-source merge threshold for a source type.
    pub fn same_source_threshold(&self, source_type: &str) -> i32 {
        self.merge
            .same_source_thresholds
            .get(source_type)
            .copied()
            .unwrap_or(self.merge.same_source_default)
    }

    /// Get cross-source merge threshold for a source type.
    pub fn cross_source_threshold(&self, source_type: &str) -> i32 {
        self.merge
            .cross_source_thresholds
            .get(source_type)
            .copied()
            .unwrap_or(self.merge.cross_source_default)
    }

    /// Compute size penalty for a cluster with the given event count.
    /// Returns `None` if the cluster should be blocked from merging.
    ///
    /// When `smooth_size_penalty` is true, uses `-ln(1 + count/divisor)`:
    /// - 50 events → -0.69
    /// - 100 events → -1.10
    /// - 200 events → -1.61
    /// - 500 events → -2.30
    /// Never blocks; sweeps handle oversized clusters.
    ///
    /// When `smooth_size_penalty` is false, uses the stepped tier system.
    pub fn size_penalty(&self, event_count: usize) -> Option<i32> {
        if let Some(block) = self.scoring.size_block_at {
            if event_count >= block {
                return None;
            }
        }
        if self.scoring.smooth_size_penalty {
            let penalty = -(1.0 + event_count as f64 / self.scoring.smooth_size_divisor).ln();
            return Some(penalty.round() as i32);
        }
        if event_count < self.scoring.size_penalty_start {
            return Some(0);
        }
        let tiers = (event_count - self.scoring.size_penalty_start) / self.scoring.size_penalty_step;
        let penalty = -(tiers as i32 + 1);
        Some(penalty.max(self.scoring.size_penalty_max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_match_current_behavior() {
        let config = PipelineConfig::default();

        // Scoring
        assert_eq!(config.scoring.entity_score_cap, 5);
        assert_eq!(config.scoring.topic_score_cap, 4);
        assert_eq!(config.scoring.vector_hard_gate, 0.40);
        assert_eq!(config.scoring.vector_tiers.len(), 3);

        // Merge
        assert_eq!(config.merge.semantic_threshold, 0.75);
        assert_eq!(config.merge.entity_threshold, 2);
        assert_eq!(*config.merge.same_source_thresholds.get("gdelt").unwrap(), 5);
        assert_eq!(*config.merge.same_source_thresholds.get("firms").unwrap(), 4);

        // Caps
        assert_eq!(config.cluster_caps.max_entities, 50);
        assert_eq!(config.cluster_caps.max_topics, 30);
        assert_eq!(config.cluster_caps.max_children_per_parent, 15);

        // Phases
        assert_eq!(config.phases.emerging_min_events, 3);
        assert_eq!(config.phases.active_decline_rate_ratio, 0.3);

        // Temporal decay
        assert_eq!(config.decay_params("conflict_event"), (4.0, 1.0));
        assert_eq!(config.decay_params("nuclear_event"), (48.0, 12.0));
        assert_eq!(config.decay_params("unknown_type"), (12.0, 3.0));

        // Geo
        assert_eq!(config.geo_radius_km("conflict_event"), 50.0);
        assert_eq!(config.geo_radius_km("gps_interference"), 300.0);
        assert_eq!(config.geo_radius_km("unknown_type"), 150.0);

        // Search
        assert_eq!(config.search.daily_cap, 1_400);
        assert_eq!(config.search.hourly_cap, 60);
    }

    #[test]
    fn test_size_penalty_smooth() {
        let config = PipelineConfig::default();
        assert!(config.scoring.smooth_size_penalty);

        // Smooth: -ln(1 + count/50)
        assert_eq!(config.size_penalty(0), Some(0));
        assert_eq!(config.size_penalty(10), Some(0)); // -0.18 rounds to 0
        assert_eq!(config.size_penalty(50), Some(-1)); // -0.69
        assert_eq!(config.size_penalty(100), Some(-1)); // -1.10
        assert_eq!(config.size_penalty(200), Some(-2)); // -1.61
        assert_eq!(config.size_penalty(500), Some(-2)); // -2.30
        // Never returns None (no hard block when smooth)
        assert!(config.size_penalty(1000).is_some());
    }

    #[test]
    fn test_size_penalty_stepped() {
        let mut config = PipelineConfig::default();
        config.scoring.smooth_size_penalty = false;
        config.scoring.size_block_at = Some(81);

        assert_eq!(config.size_penalty(0), Some(0));
        assert_eq!(config.size_penalty(19), Some(0));
        assert_eq!(config.size_penalty(20), Some(-1));
        assert_eq!(config.size_penalty(39), Some(-1));
        assert_eq!(config.size_penalty(40), Some(-2));
        assert_eq!(config.size_penalty(60), Some(-3));
        assert_eq!(config.size_penalty(79), Some(-3));
        assert_eq!(config.size_penalty(81), None); // blocked
    }

    #[test]
    fn test_size_penalty_no_block_stepped() {
        let mut config = PipelineConfig::default();
        config.scoring.smooth_size_penalty = false;
        config.scoring.size_block_at = None;

        // Without block, large clusters just get max penalty (stepped mode)
        assert_eq!(config.size_penalty(81), Some(-3));
        assert_eq!(config.size_penalty(200), Some(-3));
    }

    #[test]
    fn test_merge_thresholds() {
        let config = PipelineConfig::default();

        assert_eq!(config.same_source_threshold("gdelt"), 5);
        assert_eq!(config.same_source_threshold("firms"), 4);
        assert_eq!(config.same_source_threshold("unknown"), 4);
        assert_eq!(config.cross_source_threshold("any"), 3);
    }

    #[test]
    fn test_serialization() {
        let config = PipelineConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.scoring.entity_score_cap, 5);
        assert_eq!(deserialized.merge.semantic_threshold, 0.75);
    }
}

// ===========================================================================
// IntelConfig — intelligence layer configuration
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelConfig {
    pub budget: BudgetConfig,
    pub enrichment: EnrichmentConfig,
    pub analysis: AnalysisConfig,
    pub narrative: NarrativeConfig,
    pub ollama: OllamaConfig,
    pub title: TitleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub daily_cap_usd: f64,
    pub sonnet_headroom_ratio: f64,
    pub haiku_input_per_m: f64,
    pub haiku_output_per_m: f64,
    pub haiku_cache_per_m: f64,
    pub sonnet_input_per_m: f64,
    pub sonnet_output_per_m: f64,
    pub sonnet_cache_per_m: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily_cap_usd: 10.0,
            sonnet_headroom_ratio: 0.8,
            haiku_input_per_m: 1.0,
            haiku_output_per_m: 5.0,
            haiku_cache_per_m: 0.10,
            sonnet_input_per_m: 3.0,
            sonnet_output_per_m: 15.0,
            sonnet_cache_per_m: 0.30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentConfig {
    pub model: String,
    pub max_tokens: u32,
}

impl Default for EnrichmentConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-4-5-20251001".into(),
            max_tokens: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    pub model: String,
    pub max_tokens: u32,
    pub tempo_high_threshold: f64,
    pub tempo_elevated_threshold: f64,
    pub interval_high_secs: u64,
    pub interval_elevated_secs: u64,
    pub interval_normal_secs: u64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-6".into(),
            max_tokens: 8192,
            tempo_high_threshold: 20.0,
            tempo_elevated_threshold: 5.0,
            interval_high_secs: 900,
            interval_elevated_secs: 3600,
            interval_normal_secs: 7200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeConfig {
    pub max_tokens: u32,
    pub regen_event_threshold: usize,
    pub regen_timeout_mins: u64,
    pub regen_timeout_min_events: usize,
    pub sonnet_source_threshold: usize,
}

impl Default for NarrativeConfig {
    fn default() -> Self {
        Self {
            max_tokens: 1500,
            regen_event_threshold: 30,
            regen_timeout_mins: 120,
            regen_timeout_min_events: 10,
            sonnet_source_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub timeout_secs: u64,
    pub default_model: String,
    pub gpu_concurrency: usize,
    pub num_ctx_enrich: u32,
    pub num_ctx_narrative: u32,
    pub num_ctx_analysis: u32,
    pub temperature_enrich: f64,
    pub temperature_narrative: f64,
    pub temperature_analysis: f64,
    pub health_timeout_secs: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            default_model: "qwen2.5:7b".into(),
            gpu_concurrency: 1,
            num_ctx_enrich: 4096,
            num_ctx_narrative: 8192,
            num_ctx_analysis: 8192,
            temperature_enrich: 0.0,
            temperature_narrative: 0.1,
            temperature_analysis: 0.1,
            health_timeout_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleConfig {
    pub max_tokens_ollama: u32,
    pub max_tokens_claude: u32,
    pub fallback_model: String,
}

impl Default for TitleConfig {
    fn default() -> Self {
        Self {
            max_tokens_ollama: 2048,
            max_tokens_claude: 40,
            fallback_model: "claude-haiku-4-5-20251001".into(),
        }
    }
}

impl Default for IntelConfig {
    fn default() -> Self {
        Self {
            budget: BudgetConfig::default(),
            enrichment: EnrichmentConfig::default(),
            analysis: AnalysisConfig::default(),
            narrative: NarrativeConfig::default(),
            ollama: OllamaConfig::default(),
            title: TitleConfig::default(),
        }
    }
}

impl IntelConfig {
    /// Load from environment variables, falling back to defaults.
    /// Mirrors the PipelineConfig::from_env() pattern.
    pub fn from_env() -> Self {
        let mut config = if let Ok(json) = std::env::var("INTEL_CONFIG_JSON") {
            match serde_json::from_str::<IntelConfig>(&json) {
                Ok(c) => {
                    tracing::info!("Loaded intel config from INTEL_CONFIG_JSON");
                    c
                }
                Err(e) => {
                    tracing::warn!("Failed to parse INTEL_CONFIG_JSON: {e}, using defaults");
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        macro_rules! env_override {
            ($field:expr, $var:expr, $ty:ty) => {
                if let Ok(val) = std::env::var($var) {
                    if let Ok(parsed) = val.parse::<$ty>() {
                        $field = parsed;
                    }
                }
            };
        }

        // Budget
        env_override!(config.budget.daily_cap_usd, "INTEL_DAILY_BUDGET_USD", f64);

        // Models (backwards-compatible env var names)
        if let Ok(val) = std::env::var("INTEL_ENRICHMENT_MODEL") { config.enrichment.model = val; }
        if let Ok(val) = std::env::var("INTEL_ANALYSIS_MODEL") { config.analysis.model = val; }
        if let Ok(val) = std::env::var("OLLAMA_MODEL") { config.ollama.default_model = val; }

        // Ollama
        env_override!(config.ollama.timeout_secs, "OLLAMA_TIMEOUT_SECS", u64);
        env_override!(config.ollama.gpu_concurrency, "OLLAMA_GPU_CONCURRENCY", usize);

        config
    }
}

