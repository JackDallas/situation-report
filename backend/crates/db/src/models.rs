use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Event {
    pub event_time: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub source_type: String,
    pub source_id: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub region_code: Option<String>,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub event_type: Option<String>,
    pub severity: Option<String>,
    pub confidence: Option<f32>,
    pub tags: Option<Vec<String>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LatestPosition {
    pub entity_id: String,
    pub source_type: String,
    pub entity_name: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub heading: Option<f32>,
    pub speed: Option<f32>,
    pub altitude: Option<f32>,
    pub last_seen: DateTime<Utc>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SourceConfig {
    pub source_id: String,
    pub enabled: bool,
    pub poll_interval_secs: Option<i32>,
    pub api_key_encrypted: Option<String>,
    pub extra_config: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SourceHealth {
    pub source_id: String,
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: Option<i32>,
    pub total_events_24h: Option<i32>,
    pub status: String,
}
