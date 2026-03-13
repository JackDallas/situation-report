use axum::Json;
use axum::extract::State;
use sr_intel::{AnalysisReport, BudgetStatus};

use crate::state::AppState;

/// GET /api/intel/latest — latest intelligence analysis report
pub async fn get_latest_analysis(
    State(state): State<AppState>,
) -> Json<Option<AnalysisReport>> {
    let report = state
        .analysis
        .read()
        .ok()
        .and_then(|lock| lock.clone());
    Json(report)
}

/// GET /api/intel/budget — current AI spend status
pub async fn get_budget(
    State(state): State<AppState>,
) -> Json<BudgetStatus> {
    let status = state.budget.status().await;
    Json(status)
}
