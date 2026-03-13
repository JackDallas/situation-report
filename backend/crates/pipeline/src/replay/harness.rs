//! Replay harness: feeds a captured event stream through `PipelineCore`
//! with deterministic clock injection, capturing snapshots at configurable intervals.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sr_config::PipelineConfig;
use sr_intel::{BudgetManager, ClaudeClient, OllamaClient};
use sr_types::EventType;
use tracing::{debug, info};

use crate::core::PipelineCore;

use super::types::{ReplayDataset, ReplayMetrics, ReplaySnapshot};

/// Configuration for a replay run.
pub struct ReplayConfig {
    /// How often to capture a snapshot (in simulated time).
    /// Default: every 15 minutes.
    pub snapshot_interval: Duration,
    /// Whether to run flush_pending() after each batch of events at the same timestamp.
    /// Default: true (matches production behavior).
    pub flush_pending: bool,
    /// Enable AI processing (enrichment, titles, narratives).
    /// Default: false (fast clustering-only replay).
    pub ai_enabled: bool,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: Duration::minutes(15),
            flush_pending: true,
            ai_enabled: false,
        }
    }
}

/// The replay harness drives events through a `PipelineCore` with clock control.
pub struct ReplayHarness {
    core: PipelineCore,
    replay_config: ReplayConfig,
}

impl ReplayHarness {
    /// Create a new replay harness with the given pipeline configuration.
    /// No AI, no embeddings — fast clustering-only replay.
    pub fn new(pipeline_config: PipelineConfig, replay_config: ReplayConfig) -> Self {
        Self {
            core: PipelineCore::new(Arc::new(pipeline_config), false, None, None, None, None),
            replay_config,
        }
    }

    /// Create a new replay harness with AI clients for faithful replay.
    ///
    /// When `replay_config.ai_enabled` is true, the harness will:
    /// - Enrich enrichable events (news, telegram, geonews) before ingestion
    /// - Generate AI titles for clusters that need them
    /// - Generate narratives for active clusters
    pub fn with_ai(
        pipeline_config: PipelineConfig,
        replay_config: ReplayConfig,
        ollama: Option<Arc<OllamaClient>>,
        claude: Option<Arc<ClaudeClient>>,
        budget: Arc<BudgetManager>,
    ) -> Self {
        Self {
            core: PipelineCore::new(
                Arc::new(pipeline_config),
                false, // embeddings require the ONNX model; not loaded in replay
                ollama,
                claude,
                None, // Gemini not used in replay yet
                Some(budget),
            ),
            replay_config,
        }
    }

    /// Run a full replay of a dataset, returning metrics and snapshots.
    ///
    /// Events are fed in chronological order. The situation graph clock is
    /// advanced to each event's `event_time`, so all internal time-dependent
    /// logic (decay, gap tolerance, phase transitions) behaves identically
    /// to production — just at replay speed.
    ///
    /// When `ai_enabled` is true, enrichable events are enriched inline before
    /// ingestion, and AI titles/narratives are generated after each clustering tick.
    pub async fn run(&mut self, dataset: &ReplayDataset) -> ReplayMetrics {
        let wall_start = Instant::now();
        let mut snapshots = Vec::new();
        let mut events_ingested: usize = 0;
        let mut events_enriched: usize = 0;
        let mut peak_cluster_count: usize = 0;
        let mut last_snapshot_time: Option<DateTime<Utc>> = None;
        let mut last_tick_time: Option<DateTime<Utc>> = None;
        let mut skipped_deserialize: usize = 0;
        // Tick every 30s of simulated time (matches production tick rate)
        let tick_interval = Duration::seconds(30);
        let ai_enabled = self.replay_config.ai_enabled;

        info!(
            events = dataset.events.len(),
            range_start = %dataset.metadata.time_range_start,
            range_end = %dataset.metadata.time_range_end,
            ai = ai_enabled,
            "Starting replay"
        );

        for (i, replay_event) in dataset.events.iter().enumerate() {
            // Advance the clock to this event's time
            self.core.set_clock(Some(replay_event.event_time));

            // Convert and ingest (skip events that don't deserialize)
            let mut insertable = match replay_event.to_insertable() {
                Some(e) => e,
                None => {
                    skipped_deserialize += 1;
                    continue;
                }
            };

            // AI enrichment: enrich news/telegram/geonews inline before ingestion.
            // This mirrors production's defer→enrich→re-ingest pattern, but synchronous.
            if ai_enabled {
                let is_enrichable = matches!(
                    insertable.event_type,
                    EventType::NewsArticle | EventType::TelegramMessage | EventType::GeoNews | EventType::BlueskyPost
                ) && insertable.payload.get("enrichment").is_none();

                if is_enrichable && self.core.enrich_event(&mut insertable).await {
                    events_enriched += 1;
                }
            }

            self.core.ingest_event(&insertable);
            events_ingested += 1;

            // Run full clustering tick at production-like intervals (every 30s simulated)
            if self.replay_config.flush_pending {
                let should_tick = match last_tick_time {
                    None => true,
                    Some(last) => replay_event.event_time - last >= tick_interval,
                };
                if should_tick {
                    self.core.tick_clustering();

                    // AI: generate titles and narratives inline after clustering
                    if ai_enabled {
                        self.core.generate_titles().await;
                        self.core.generate_narratives().await;
                    }

                    last_tick_time = Some(replay_event.event_time);
                }
            }

            // Capture snapshot at configured intervals
            let should_snapshot = match last_snapshot_time {
                None => true,
                Some(last) => {
                    replay_event.event_time - last >= self.replay_config.snapshot_interval
                }
            };

            if should_snapshot {
                // Also tick before snapshot to get accurate state
                if self.replay_config.flush_pending {
                    self.core.tick_clustering();
                    last_tick_time = Some(replay_event.event_time);
                }
                let clusters = self.core.raw_clusters();
                peak_cluster_count = peak_cluster_count.max(clusters.len());
                snapshots.push(ReplaySnapshot {
                    time: replay_event.event_time,
                    events_ingested,
                    clusters,
                });
                last_snapshot_time = Some(replay_event.event_time);

                debug!(
                    event_idx = i,
                    time = %replay_event.event_time,
                    clusters = snapshots.last().unwrap().clusters.len(),
                    "Replay snapshot"
                );
            }

            // Progress logging every 10k events
            if events_ingested % 10_000 == 0 {
                info!(
                    events = events_ingested,
                    enriched = events_enriched,
                    skipped = skipped_deserialize,
                    internal = self.core.internal_cluster_count(),
                    pending = self.core.pending_buffer_len(),
                    elapsed_ms = wall_start.elapsed().as_millis(),
                    "Replay progress"
                );
            }
        }

        // Final tick + AI pass
        self.core.set_clock(Some(dataset.metadata.time_range_end));
        if self.replay_config.flush_pending {
            self.core.tick_clustering();
            if ai_enabled {
                self.core.generate_titles().await;
                self.core.generate_narratives().await;
            }
        }

        let final_raw = self.core.raw_clusters();
        let raw_count = final_raw.iter().filter(|c| c.parent_id.is_none()).count();
        peak_cluster_count = peak_cluster_count.max(final_raw.len());

        // Also compute production-quality filtered count
        let final_filtered = self.core.active_clusters();
        let final_count = final_filtered.iter().filter(|c| c.parent_id.is_none()).count();

        let avg_certainty = if final_raw.is_empty() {
            0.0
        } else {
            final_raw.iter().map(|c| c.certainty).sum::<f32>() / final_raw.len() as f32
        };
        let titled = final_raw
            .iter()
            .filter(|c| c.parent_id.is_none() && c.has_ai_title)
            .count();

        snapshots.push(ReplaySnapshot {
            time: dataset.metadata.time_range_end,
            events_ingested,
            clusters: final_raw,
        });

        let wall_elapsed = wall_start.elapsed();

        info!(
            events = events_ingested,
            enriched = events_enriched,
            raw_clusters = raw_count,
            filtered_clusters = final_count,
            ai_titled = titled,
            peak_clusters = peak_cluster_count,
            avg_certainty = format!("{:.2}", avg_certainty),
            wall_ms = wall_elapsed.as_millis(),
            "Replay complete"
        );

        ReplayMetrics {
            total_events: dataset.events.len(),
            events_accepted: events_ingested,
            raw_cluster_count: raw_count,
            final_cluster_count: final_count,
            peak_cluster_count,
            avg_certainty,
            titled_clusters: titled,
            replay_duration_ms: wall_elapsed.as_millis() as u64,
            git_hash: None,  // filled by caller
            label: None,     // filled by caller
            run_at: Some(Utc::now()),
            snapshots,
            config: PipelineConfig::clone(self.core.config()),
        }
    }

    /// Access the internal PipelineCore (e.g., for inspecting state after replay).
    pub fn core(&self) -> &PipelineCore {
        &self.core
    }

    /// Access the internal PipelineCore mutably.
    pub fn core_mut(&mut self) -> &mut PipelineCore {
        &mut self.core
    }

    /// Access the internal situation graph (e.g., for inspecting state after replay).
    pub fn graph(&self) -> &crate::situation_graph::SituationGraph {
        &self.core.graph
    }

    /// Access the internal situation graph mutably.
    pub fn graph_mut(&mut self) -> &mut crate::situation_graph::SituationGraph {
        &mut self.core.graph
    }
}

/// Compare two replay runs and produce a diff summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayComparison {
    /// Label for the baseline run (e.g., "v1.2.0" or "before").
    pub baseline_label: String,
    /// Label for the candidate run (e.g., "v1.3.0" or "after").
    pub candidate_label: String,
    /// Change in final cluster count.
    pub cluster_count_delta: i32,
    /// Change in average certainty.
    pub avg_certainty_delta: f32,
    /// Change in peak cluster count.
    pub peak_cluster_delta: i32,
    /// Titles present in baseline but not in candidate.
    pub lost_titles: Vec<String>,
    /// Titles present in candidate but not in baseline.
    pub new_titles: Vec<String>,
    /// Titles present in both (fuzzy match).
    pub common_titles: Vec<String>,
}

impl ReplayComparison {
    /// Build a comparison from two completed replay metrics.
    pub fn compare(
        baseline_label: String,
        baseline: &ReplayMetrics,
        candidate_label: String,
        candidate: &ReplayMetrics,
    ) -> Self {
        let baseline_final = baseline.snapshots.last();
        let candidate_final = candidate.snapshots.last();

        let baseline_titles: std::collections::HashSet<String> = baseline_final
            .map(|s| {
                s.clusters
                    .iter()
                    .filter(|c| c.parent_id.is_none())
                    .map(|c| c.title.clone())
                    .collect()
            })
            .unwrap_or_default();

        let candidate_titles: std::collections::HashSet<String> = candidate_final
            .map(|s| {
                s.clusters
                    .iter()
                    .filter(|c| c.parent_id.is_none())
                    .map(|c| c.title.clone())
                    .collect()
            })
            .unwrap_or_default();

        let lost: Vec<String> = baseline_titles
            .difference(&candidate_titles)
            .cloned()
            .collect();
        let new: Vec<String> = candidate_titles
            .difference(&baseline_titles)
            .cloned()
            .collect();
        let common: Vec<String> = baseline_titles
            .intersection(&candidate_titles)
            .cloned()
            .collect();

        Self {
            baseline_label,
            candidate_label,
            cluster_count_delta: candidate.final_cluster_count as i32
                - baseline.final_cluster_count as i32,
            avg_certainty_delta: candidate.avg_certainty - baseline.avg_certainty,
            peak_cluster_delta: candidate.peak_cluster_count as i32
                - baseline.peak_cluster_count as i32,
            lost_titles: lost,
            new_titles: new,
            common_titles: common,
        }
    }
}
