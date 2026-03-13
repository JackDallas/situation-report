use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::Json;
use sr_pipeline::Summary;
use tracing::info;

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
        "events_ingested": m.events_ingested.load(Ordering::Relaxed),
        "events_correlated": m.events_correlated.load(Ordering::Relaxed),
        "events_enriched": m.events_enriched.load(Ordering::Relaxed),
        "events_published": m.events_published.load(Ordering::Relaxed),
        "events_filtered": m.events_filtered.load(Ordering::Relaxed),
        "incidents_created": m.incidents_created.load(Ordering::Relaxed),
        "gpu_paused": m.is_gpu_paused(),
    }))
}

/// POST /api/pipeline/gpu/pause — pause GPU-intensive processing
///
/// Pauses embedding inference (BGE-M3) and Ollama LLM calls (enrichment,
/// titles, narratives, merge audit, analysis). Data ingestion, clustering,
/// correlation rules, and SSE publishing continue normally.
pub async fn pause_gpu(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    state.metrics.gpu_paused.store(true, Ordering::Relaxed);
    info!("GPU processing paused via API");
    axum::Json(serde_json::json!({ "gpu_paused": true }))
}

/// POST /api/pipeline/gpu/resume — resume GPU-intensive processing
pub async fn resume_gpu(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    state.metrics.gpu_paused.store(false, Ordering::Relaxed);
    info!("GPU processing resumed via API");
    axum::Json(serde_json::json!({ "gpu_paused": false }))
}
