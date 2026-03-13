//! Replay API routes — trigger and view algorithm replays directly from the database.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use sr_pipeline::replay::{ReplayComparison, ReplayConfig, ReplayHarness, ReplayMetrics};
use sr_sources::db::queries::query_replay_events;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ReplayParams {
    /// Start of time range (ISO 8601). Default: 72h ago.
    pub since: Option<DateTime<Utc>>,
    /// End of time range (ISO 8601). Default: now.
    pub until: Option<DateTime<Utc>>,
    /// Max events to replay. Default: 500,000.
    pub limit: Option<i64>,
    /// Snapshot interval in minutes. Default: 15.
    pub snapshot_minutes: Option<i64>,
}

/// POST /api/replay/run — run a replay over a time range from the database.
/// Returns full metrics including snapshots of cluster state over time.
pub async fn run_replay(
    State(state): State<AppState>,
    Query(params): Query<ReplayParams>,
) -> Result<Json<ReplayMetrics>, ApiError> {
    let since = params.since.unwrap_or_else(|| Utc::now() - Duration::hours(72));
    let until = params.until.unwrap_or_else(Utc::now);
    let limit = params.limit.unwrap_or(500_000);
    let snapshot_minutes = params.snapshot_minutes.unwrap_or(15);

    tracing::info!(%since, %until, limit, "Starting replay from DB");

    // Fetch events directly from DB
    let events = query_replay_events(&state.db, since, until, limit).await?;
    let event_count = events.len();
    tracing::info!(event_count, "Events loaded for replay");

    // Convert to replay events
    let replay_events: Vec<_> = events
        .into_iter()
        .map(sr_pipeline::replay::ReplayEvent::from)
        .collect();

    // Build dataset
    let dataset = sr_pipeline::replay::ReplayDataset::from_events(
        format!("api-replay-{}", Utc::now().format("%Y%m%dT%H%M%S")),
        replay_events,
        since,
        until,
        None,
    );

    // Run replay with the current pipeline config (no AI in API replays)
    let replay_config = ReplayConfig {
        snapshot_interval: Duration::minutes(snapshot_minutes),
        flush_pending: true,
        ai_enabled: false,
    };

    let pipeline_config = (*state.pipeline_config).clone();
    let metrics = tokio::spawn(async move {
        let mut harness = ReplayHarness::new(pipeline_config, replay_config);
        harness.run(&dataset).await
    })
    .await
    .map_err(|e| anyhow::anyhow!("Replay task failed: {e}"))?;

    tracing::info!(
        clusters = metrics.final_cluster_count,
        certainty = format!("{:.2}", metrics.avg_certainty),
        duration_ms = metrics.replay_duration_ms,
        "Replay complete"
    );

    Ok(Json(metrics))
}

#[derive(Debug, Deserialize)]
pub struct ReplayCompareParams {
    /// Start of time range for both runs.
    pub since: Option<DateTime<Utc>>,
    /// End of time range for both runs.
    pub until: Option<DateTime<Utc>>,
    /// Max events. Default: 500,000.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CompareResponse {
    pub baseline: ReplayMetricsSummary,
    pub candidate: ReplayMetricsSummary,
    pub comparison: ReplayComparison,
}

#[derive(Debug, Serialize)]
pub struct ReplayMetricsSummary {
    pub total_events: usize,
    pub final_cluster_count: usize,
    pub peak_cluster_count: usize,
    pub avg_certainty: f32,
    pub replay_duration_ms: u64,
    pub snapshot_count: usize,
}

impl From<&ReplayMetrics> for ReplayMetricsSummary {
    fn from(m: &ReplayMetrics) -> Self {
        Self {
            total_events: m.total_events,
            final_cluster_count: m.final_cluster_count,
            peak_cluster_count: m.peak_cluster_count,
            avg_certainty: m.avg_certainty,
            replay_duration_ms: m.replay_duration_ms,
            snapshot_count: m.snapshots.len(),
        }
    }
}

/// POST /api/replay/compare — run baseline (current config) vs candidate (posted config)
/// on the same event data, return comparison.
pub async fn compare_replay(
    State(state): State<AppState>,
    Query(params): Query<ReplayCompareParams>,
    Json(candidate_config): Json<sr_config::PipelineConfig>,
) -> Result<Json<CompareResponse>, ApiError> {
    let since = params.since.unwrap_or_else(|| Utc::now() - Duration::hours(72));
    let until = params.until.unwrap_or_else(Utc::now);
    let limit = params.limit.unwrap_or(500_000);

    tracing::info!(%since, %until, "Starting A/B replay comparison");

    // Fetch events once
    let events = query_replay_events(&state.db, since, until, limit).await?;
    let replay_events: Vec<_> = events
        .into_iter()
        .map(sr_pipeline::replay::ReplayEvent::from)
        .collect();

    let dataset = sr_pipeline::replay::ReplayDataset::from_events(
        format!("compare-{}", Utc::now().format("%Y%m%dT%H%M%S")),
        replay_events,
        since,
        until,
        None,
    );

    let baseline_config = (*state.pipeline_config).clone();

    // Run both replays sequentially (no AI in API comparisons)
    let (baseline_metrics, candidate_metrics) = tokio::spawn(async move {
        let replay_cfg = || ReplayConfig {
            snapshot_interval: Duration::minutes(15),
            flush_pending: true,
            ai_enabled: false,
        };

        let mut baseline_harness = ReplayHarness::new(baseline_config, replay_cfg());
        let baseline = baseline_harness.run(&dataset).await;

        let mut candidate_harness = ReplayHarness::new(candidate_config, replay_cfg());
        let candidate = candidate_harness.run(&dataset).await;

        (baseline, candidate)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Comparison task failed: {e}"))?;

    let comparison = ReplayComparison::compare(
        "current".to_string(),
        &baseline_metrics,
        "candidate".to_string(),
        &candidate_metrics,
    );

    Ok(Json(CompareResponse {
        baseline: ReplayMetricsSummary::from(&baseline_metrics),
        candidate: ReplayMetricsSummary::from(&candidate_metrics),
        comparison,
    }))
}
