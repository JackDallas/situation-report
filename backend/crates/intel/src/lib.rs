pub mod analyze;
pub mod budget;
pub mod client;
pub mod enrich;
pub mod gemini;
pub mod narrative;
pub mod ollama;
pub mod prompts;
pub mod search;
pub mod titles;
pub mod types;

pub use sr_config;
pub use sr_types;

pub use analyze::{AnalysisInput, analyze_current_state, analyze_tiered, analysis_interval_secs, tempo_label};
pub use budget::BudgetManager;
pub use client::ClaudeClient;
pub use enrich::{article_from_event, enrich_article, enrich_article_gemini, enrich_article_tiered};
pub use gemini::{GeminiClient, GeminiModel};
pub use ollama::OllamaClient;
pub use search::{
    GapAnalysisInput, GapType, InformationGap, SearchHistory, SearchRateLimiter,
    SupplementaryData, analyze_gaps, build_gap_query, build_search_query,
    compute_search_priority, search_situation_context,
};
pub use titles::generate_situation_title;
pub use types::{AnalysisReport, BudgetStatus, EnrichedArticleV2, ExtractedRelationship, ExtractedStateChange};
pub use narrative::{NarrativeContext, EventBrief, SituationNarrative, generate_narrative, generate_narrative_tiered, should_regenerate};

use std::sync::{Arc, RwLock};

/// Shared latest analysis report — updated by the analysis scheduler, read by REST handlers.
pub type SharedAnalysis = Arc<RwLock<Option<AnalysisReport>>>;
