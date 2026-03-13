use axum::extract::State;
use axum::Json;
use crate::state::AppState;

pub async fn get_pipeline_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.pipeline_config).unwrap_or_default())
}

pub async fn get_intel_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.intel_config).unwrap_or_default())
}

pub async fn get_app_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sources: Vec<serde_json::Value> = state.source_registry.sources()
        .iter()
        .map(|s| serde_json::json!({
            "id": s.id(),
            "name": s.name(),
            "interval_secs": s.default_interval().as_secs(),
            "streaming": s.is_streaming(),
        }))
        .collect();
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "sources": sources,
        "regions": [
            {"id": "middle-east", "name": "Middle East", "bbox": [25.0, 34.0, 42.0, 63.0]},
            {"id": "eastern-europe", "name": "Eastern Europe", "bbox": [44.0, 22.0, 53.0, 40.0]},
            {"id": "east-asia", "name": "East Asia", "bbox": [20.0, 100.0, 45.0, 145.0]},
            {"id": "africa", "name": "Africa", "bbox": [-35.0, -20.0, 37.0, 55.0]},
            {"id": "southeast-asia", "name": "Southeast Asia", "bbox": [-10.0, 95.0, 25.0, 140.0]},
        ],
    }))
}
