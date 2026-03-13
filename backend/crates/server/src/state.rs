use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use sqlx::PgPool;
use sr_intel::{BudgetManager, SharedAnalysis};
use sr_pipeline::{PipelineConfig, PipelineMetrics, PublishEvent, SharedSummaries, SituationClusterDTO};
use sr_sources::registry::SourceRegistry;
use sr_sources::shodan::CameraResult;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::routes::satellites::SatelliteTle;

pub type SharedSituations = Arc<std::sync::RwLock<Vec<SituationClusterDTO>>>;
pub type SharedCameras = Arc<std::sync::RwLock<HashMap<Uuid, Vec<CameraResult>>>>;
pub type SharedSatelliteTles = Arc<std::sync::RwLock<Vec<SatelliteTle>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub publish_tx: broadcast::Sender<PublishEvent>,
    pub summaries: SharedSummaries,
    pub source_registry: Arc<SourceRegistry>,
    pub sse_event_counter: Arc<AtomicU64>,
    pub analysis: SharedAnalysis,
    pub budget: Arc<BudgetManager>,
    pub situations: SharedSituations,
    pub cameras: SharedCameras,
    pub metrics: Arc<PipelineMetrics>,
    pub pipeline_config: Arc<PipelineConfig>,
    pub intel_config: Arc<sr_config::IntelConfig>,
    pub api_key: Option<String>,
    pub satellite_tles: SharedSatelliteTles,
}
