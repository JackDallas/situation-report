//! `PipelineCore` — shared business logic for production and replay drivers.
//!
//! Owns the `SituationGraph`, embedding cache, and AI clients. Both the
//! production `run_pipeline()` loop and the `ReplayHarness` call the same
//! methods here, ensuring identical clustering / scoring / enrichment
//! behavior regardless of the driver.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sr_config::PipelineConfig;
use sr_embeddings::EmbeddingCache;
use sr_intel::{
    BudgetManager, ClaudeClient, GeminiClient, OllamaClient,
    article_from_event, enrich_article_tiered, generate_narrative_tiered,
    generate_situation_title,
};
use sr_sources::InsertableEvent;
use sr_types::EventType;
use tracing::{debug, info};
use uuid::Uuid;

use crate::situation_graph::{PhaseTransition, SituationClusterDTO, SituationGraph};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-situation narrative tracking state.
pub struct NarrativeState {
    pub version: i32,
    pub last_generated: Option<DateTime<Utc>>,
    /// Cluster signal_event_count at last narrative generation.
    pub signal_count_at_gen: usize,
    pub last_narrative: Option<String>,
    /// Cumulative summary for long-running situation memory.
    pub previous_summary: Option<String>,
    /// Event count at last summary generation (regenerate every 50 events).
    pub event_count_at_summary: usize,
    /// Timestamp of last summary generation (regenerate every 6 hours).
    pub last_summary_generated: Option<DateTime<Utc>>,
}

/// Output from a single clustering tick — the caller (production or replay)
/// decides how to dispatch these results (channels, DB, metrics, etc.).
pub struct TickOutput {
    /// Merges applied this tick: (parent_id, child_id, skip_audit).
    pub merges: Vec<(Uuid, Uuid, bool)>,
    /// Phase transitions detected this tick.
    pub phase_transitions: Vec<(Uuid, PhaseTransition)>,
    /// Clusters whose severity escalated this tick.
    pub severity_escalations: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// PipelineCore
// ---------------------------------------------------------------------------

/// Core pipeline state shared between production and replay drivers.
///
/// Contains the situation graph, embedding cache, tick counter, and optional
/// AI clients. Both drivers call the same `ingest_event()` and
/// `tick_clustering()` methods, guaranteeing identical business logic.
pub struct PipelineCore {
    pub graph: SituationGraph,
    pub embedding_cache: EmbeddingCache,
    pub embeddings_enabled: bool,
    tick_count: u64,
    config: Arc<PipelineConfig>,

    // AI clients (optional — None in replay without --ai)
    pub ollama: Option<Arc<OllamaClient>>,
    pub claude: Option<Arc<ClaudeClient>>,
    pub gemini: Option<Arc<GeminiClient>>,
    pub budget: Option<Arc<BudgetManager>>,

    // AI tracking state
    pub title_pending: HashSet<Uuid>,
    pub narrative_state: HashMap<Uuid, NarrativeState>,
}

impl PipelineCore {
    /// Create a new `PipelineCore`.
    ///
    /// For replay without AI, pass `None` for ollama/claude/budget.
    pub fn new(
        config: Arc<PipelineConfig>,
        embeddings_enabled: bool,
        ollama: Option<Arc<OllamaClient>>,
        claude: Option<Arc<ClaudeClient>>,
        gemini: Option<Arc<GeminiClient>>,
        budget: Option<Arc<BudgetManager>>,
    ) -> Self {
        Self {
            graph: SituationGraph::new(config.clone()),
            embedding_cache: EmbeddingCache::new(10_000, 0.05),
            embeddings_enabled,
            tick_count: 0,
            config,
            ollama,
            claude,
            gemini,
            budget,
            title_pending: HashSet::new(),
            narrative_state: HashMap::new(),
        }
    }

    // ── Clock ────────────────────────────────────────────────────────────

    /// Set a simulated clock for replay. Pass `None` to use real time.
    pub fn set_clock(&mut self, time: Option<DateTime<Utc>>) {
        self.graph.set_clock(time);
    }

    // ── Ingest ───────────────────────────────────────────────────────────

    /// Ingest a single event into the situation graph.
    ///
    /// Automatically passes the embedding cache if embeddings are enabled.
    pub fn ingest_event(&mut self, event: &InsertableEvent) {
        let cache = if self.embeddings_enabled {
            Some(&mut self.embedding_cache)
        } else {
            None
        };
        self.graph.ingest(event, cache);
    }

    // ── Clustering tick ──────────────────────────────────────────────────

    /// Run all synchronous clustering operations — prune, merge, split,
    /// flush, evaluate phases. Returns a `TickOutput` describing what
    /// changed; the caller decides how to dispatch (channels, DB, etc.).
    ///
    /// This is the exact logic from `tick_situations()` lines 1748-1870,
    /// extracted so both production and replay share it.
    pub fn tick_clustering(&mut self) -> TickOutput {
        // Prune stale clusters
        if self.embeddings_enabled {
            self.graph.prune_stale_with_cache(
                Duration::from_secs(6 * 3600),
                Some(&mut self.embedding_cache),
            );
        } else {
            self.graph.prune_stale(Duration::from_secs(6 * 3600));
        }

        self.graph.expire_merge_rejections();

        self.tick_count += 1;
        let mut merges = Vec::new();

        if self.tick_count % 2 == 0 {
            let cache_ref = if self.embeddings_enabled {
                Some(&self.embedding_cache as &EmbeddingCache)
            } else {
                None
            };
            merges = self.graph.merge_overlapping(cache_ref);

            self.graph.split_divergent();

            if self.tick_count % 4 == 0 {
                let cache_ref = if self.embeddings_enabled {
                    Some(&self.embedding_cache as &EmbeddingCache)
                } else {
                    None
                };
                self.graph.sweep(cache_ref);
            }
        }

        // Flush noise buffer
        let cache = if self.embeddings_enabled {
            Some(&mut self.embedding_cache)
        } else {
            None
        };
        self.graph.flush_pending(cache);

        // Evaluate phase transitions + severity escalations
        let (phase_transitions, severity_escalations) = self.graph.evaluate_phases();

        TickOutput {
            merges,
            phase_transitions,
            severity_escalations,
        }
    }

    // ── AI: titles ───────────────────────────────────────────────────────

    /// Generate AI titles for clusters that need them (synchronous/await).
    ///
    /// In production this is typically fire-and-forget via tokio::spawn.
    /// In replay with `--ai`, it blocks inline for faithful reproduction.
    /// Returns `(cluster_id, new_title)` pairs.
    pub async fn generate_titles(&mut self) -> Vec<(Uuid, String)> {
        let has_ai = self.claude.is_some() || self.gemini.is_some() || self.ollama.is_some();
        if !has_ai {
            return vec![];
        }
        let budget = match self.budget {
            Some(ref b) => Arc::clone(b),
            None => return vec![],
        };

        // Collect title-generation inputs before borrowing graph mutably
        let needing = self.graph.clusters_needing_titles(&self.title_pending);
        let work: Vec<_> = needing
            .into_iter()
            .map(|cluster| {
                (
                    cluster.id,
                    cluster.entities.iter().cloned().collect::<Vec<_>>(),
                    cluster.topics.iter().cloned().collect::<Vec<_>>(),
                    cluster.region_codes.iter().cloned().collect::<Vec<_>>(),
                    cluster.event_titles.clone(),
                    cluster.event_count,
                    cluster.source_types.len(),
                    cluster.title.clone(),
                )
            })
            .collect();
        let mut results = Vec::new();

        for (cid, entities, topics, regions, event_titles, event_count, source_count, fallback_title) in work {
            self.title_pending.insert(cid);

            let title = generate_situation_title(
                self.claude.as_deref(),
                self.gemini.as_deref(),
                self.ollama.as_deref(),
                &budget,
                &entities,
                &topics,
                &regions,
                &event_titles,
                event_count,
                source_count,
                None,
                None,
                None,
                &[],
            )
            .await
            .unwrap_or(fallback_title);

            self.graph.update_cluster_title(cid, title.clone());
            self.title_pending.remove(&cid);
            results.push((cid, title));
        }

        results
    }

    // ── AI: enrichment ───────────────────────────────────────────────────

    /// Enrich an event if it's enrichable and lacks enrichment.
    ///
    /// Calls the same `enrich_article()` / `OllamaClient::enrich_article()`
    /// as production. Returns `true` if enrichment was applied.
    ///
    /// Note: this does NOT persist to DB or update entity graphs — those
    /// are production-driver concerns. It only populates `event.payload.enrichment`
    /// and upgrades coordinates via geocoding.
    pub async fn enrich_event(&self, event: &mut InsertableEvent) -> bool {
        let has_ai = self.claude.is_some() || self.gemini.is_some() || self.ollama.is_some();
        if !has_ai {
            return false;
        }
        let budget = match self.budget {
            Some(ref b) => Arc::clone(b),
            None => return false,
        };

        let is_enrichable = matches!(
            event.event_type,
            EventType::NewsArticle | EventType::TelegramMessage | EventType::GeoNews | EventType::BlueskyPost
        );
        if !is_enrichable {
            return false;
        }

        let article = match article_from_event(event) {
            Some(a) => a,
            None => return false,
        };

        // Tiered enrichment: Ollama -> Gemini Flash-Lite -> Claude
        let enrichment_result = enrich_article_tiered(
            self.claude.as_deref(),
            self.gemini.as_deref(),
            self.ollama.as_deref(),
            &budget,
            &article,
        )
        .await;

        match enrichment_result {
            Ok(enriched) => {
                if let Ok(enrichment_json) = serde_json::to_value(&enriched) {
                    if let Some(obj) = event.payload.as_object_mut() {
                        obj.insert("enrichment".to_string(), enrichment_json);
                    }
                }

                // Geocode upgrade
                let needs_geocode = event.latitude.is_none()
                    || event.longitude.is_none()
                    || matches!(
                        (event.latitude, event.longitude),
                        (Some(lat), Some(lon)) if sr_sources::common::is_region_centroid(lat, lon)
                    );
                if needs_geocode {
                    let upgraded_coords = enriched
                        .inferred_location
                        .as_ref()
                        .map(|loc| (loc.lat, loc.lon))
                        .or_else(|| {
                            enriched
                                .entities
                                .iter()
                                .filter(|e| e.entity_type == "location")
                                .find_map(|e| sr_sources::common::geocode_entity(&e.name))
                        });
                    if let Some((lat, lon)) = upgraded_coords {
                        event.latitude = Some(lat);
                        event.longitude = Some(lon);
                        if let Some(region) =
                            sr_sources::common::region_from_coords(lat, lon).map(String::from)
                        {
                            event.region_code = Some(region);
                        }
                    }
                }

                true
            }
            Err(e) => {
                debug!("Enrichment failed: {e}");
                false
            }
        }
    }

    // ── AI: narratives ───────────────────────────────────────────────────

    /// Generate narratives for clusters that need them (synchronous/await).
    ///
    /// In production this is fire-and-forget via tokio::spawn with a cap
    /// of MAX_NARRATIVES_PER_TICK. Here we process all eligible clusters
    /// sequentially. Returns `(situation_id, narrative_text)` pairs.
    pub async fn generate_narratives(&mut self) -> Vec<(Uuid, String)> {
        let has_ai = self.claude.is_some() || self.gemini.is_some() || self.ollama.is_some();
        if !has_ai {
            return vec![];
        }
        let budget = match self.budget {
            Some(ref b) => Arc::clone(b),
            None => return vec![],
        };

        let clusters = self.graph.active_clusters();
        let mut results = Vec::new();

        for cluster in &clusters {
            let ns = self
                .narrative_state
                .entry(cluster.id)
                .or_insert(NarrativeState {
                    version: 0,
                    last_generated: None,
                    signal_count_at_gen: 0,
                    last_narrative: None,
                    previous_summary: None,
                    event_count_at_summary: 0,
                    last_summary_generated: None,
                });
            let signal_count = self
                .graph
                .get_cluster(&cluster.id)
                .map(|c| c.signal_event_count)
                .unwrap_or(0);
            let events_since = signal_count.saturating_sub(ns.signal_count_at_gen);

            if !sr_intel::should_regenerate(
                ns.version,
                ns.last_generated,
                events_since,
                false,
                false,
            ) {
                continue;
            }

            let context = sr_intel::NarrativeContext {
                situation_title: cluster.title.clone(),
                situation_id: cluster.id,
                phase: cluster.phase.as_str().to_string(),
                severity: cluster.severity.to_string(),
                event_count: cluster.event_count,
                source_types: cluster
                    .source_types
                    .iter()
                    .map(|st| st.to_string())
                    .collect(),
                regions: cluster.region_codes.clone(),
                entities: cluster.entities.clone(),
                topics: cluster.topics.clone(),
                recent_events: Vec::new(),
                entity_context: None,
                previous_narrative: ns.last_narrative.clone(),
                current_version: ns.version,
                has_state_change: false,
                phase_history: vec![],
                event_rate_trend: "steady".to_string(),
                hours_since_last_event: 0.0,
                similar_historical: None,
                impact_summary: None,
                previous_summary: None,
            };

            match generate_narrative_tiered(
                self.claude.as_deref(),
                self.gemini.as_deref(),
                self.ollama.as_deref(),
                &budget,
                &context,
            )
            .await
            {
                Ok(Some(narrative)) => {
                    info!(
                        situation_id = %cluster.id,
                        version = narrative.version,
                        "PipelineCore: narrative generated"
                    );
                    ns.version = narrative.version;
                    ns.last_generated = Some(narrative.generated_at);
                    if let Some(c) = self.graph.get_cluster(&cluster.id) {
                        ns.signal_count_at_gen = c.signal_event_count;
                    }
                    ns.last_narrative = Some(narrative.narrative_text.clone());
                    results.push((cluster.id, narrative.narrative_text));
                }
                Ok(None) => {
                    debug!("Narrative generation skipped (budget)");
                }
                Err(e) => {
                    debug!("Narrative generation failed: {e}");
                }
            }
        }

        results
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// All clusters (unfiltered) for replay snapshots.
    pub fn raw_clusters(&self) -> Vec<SituationClusterDTO> {
        self.graph.raw_clusters()
    }

    /// Production-quality filtered clusters.
    pub fn active_clusters(&self) -> Vec<SituationClusterDTO> {
        self.graph.active_clusters()
    }

    /// Number of internal clusters (including noise buffer).
    pub fn internal_cluster_count(&self) -> usize {
        self.graph.internal_cluster_count()
    }

    /// Number of events in the noise/pending buffer.
    pub fn pending_buffer_len(&self) -> usize {
        self.graph.pending_buffer_len()
    }

    /// Current tick count.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Immutable access to the situation graph.
    pub fn graph(&self) -> &SituationGraph {
        &self.graph
    }

    /// Mutable access to the situation graph.
    pub fn graph_mut(&mut self) -> &mut SituationGraph {
        &mut self.graph
    }

    /// Pipeline config.
    pub fn config(&self) -> &Arc<PipelineConfig> {
        &self.config
    }
}
