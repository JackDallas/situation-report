use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct TranslateResponse {
    pub message: String,
}

/// POST /api/translate — stub. Translation is now handled by Haiku enrichment.
pub async fn translate() -> Json<TranslateResponse> {
    Json(TranslateResponse {
        message: "Translation is now handled by AI enrichment. See event payload.enrichment for translated content.".to_string(),
    })
}
