use axum::extract::State;
use axum::Json;
use sr_pipeline::Summary;

use crate::state::AppState;

/// GET /api/pipeline/summaries — current high-volume type summaries (for dashboard stats)
pub async fn get_summaries(
    State(state): State<AppState>,
) -> Json<Vec<Summary>> {
    let summaries = state
        .summaries
        .read()
        .map(|lock| lock.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Json(summaries)
}

/// GET /api/pipeline/metrics — atomic pipeline throughput counters
pub async fn get_pipeline_metrics(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    let m = &state.metrics;
    axum::Json(serde_json::json!({
        "events_ingested": m.events_ingested.load(std::sync::atomic::Ordering::Relaxed),
        "events_correlated": m.events_correlated.load(std::sync::atomic::Ordering::Relaxed),
        "events_enriched": m.events_enriched.load(std::sync::atomic::Ordering::Relaxed),
        "events_published": m.events_published.load(std::sync::atomic::Ordering::Relaxed),
        "events_filtered": m.events_filtered.load(std::sync::atomic::Ordering::Relaxed),
        "incidents_created": m.incidents_created.load(std::sync::atomic::Ordering::Relaxed),
    }))
}
