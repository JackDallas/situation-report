pub mod db;
pub mod registry;
pub mod common;
pub mod rate_limit;
pub mod aircraft_db;

pub mod adsb;
pub mod shodan;
pub mod acled;
pub mod gdelt;
pub mod geoconfirmed;
pub mod opensky;
pub mod airplaneslive;
pub mod ais;
pub mod firms;
pub mod usgs;
pub mod gdacs;
pub mod cloudflare;
pub mod ioda;
pub mod bgp;
pub mod otx;
pub mod certstream;
pub mod ooni;
pub mod nuclear;
pub mod notam;
pub mod gdelt_geo;
pub mod gfw;
pub mod gpsjam;
pub mod telegram;
pub mod reliefweb;
pub mod rss_news;
pub mod ukmto;
pub mod copernicus;

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::broadcast;
use ts_rs::TS;

pub use sr_types;
use sr_types::{EventType, Severity, SourceType};

/// Context passed to data sources for polling/streaming.
pub struct SourceContext {
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub config: serde_json::Value,
}

/// Flat event struct used for both persistence and SSE broadcast.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "SituationEvent", export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct InsertableEvent {
    pub event_time: DateTime<Utc>,
    pub source_type: SourceType,
    pub source_id: Option<String>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub region_code: Option<String>,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub event_type: EventType,
    pub severity: Severity,
    pub confidence: Option<f32>,
    pub tags: Vec<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    #[ts(type = "Record<string, unknown>")]
    pub payload: serde_json::Value,
    pub heading: Option<f32>,
    pub speed: Option<f32>,
    pub altitude: Option<f32>,
}

impl Default for InsertableEvent {
    fn default() -> Self {
        Self {
            event_time: Utc::now(),
            source_type: SourceType::Gdelt,
            source_id: None,
            longitude: None,
            latitude: None,
            region_code: None,
            entity_id: None,
            entity_name: None,
            event_type: EventType::NewsArticle,
            severity: Severity::Info,
            confidence: None,
            tags: vec![],
            title: None,
            description: None,
            payload: serde_json::json!({}),
            heading: None,
            speed: None,
            altitude: None,
        }
    }
}

/// Trait for all data source implementations.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Unique identifier for this source.
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Default polling interval.
    fn default_interval(&self) -> Duration;

    /// Fetch new data, returning events to broadcast.
    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>>;

    /// Whether this source uses streaming instead of polling.
    fn is_streaming(&self) -> bool {
        false
    }

    /// Start a persistent stream (called once instead of poll).
    async fn start_stream(
        &self,
        _ctx: &SourceContext,
        _tx: broadcast::Sender<InsertableEvent>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
