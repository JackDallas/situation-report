use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

/// Search queries rotated on successive polls.
const SEARCH_QUERIES: &[&str] = &[
    "iran apt",
    "israel cyber",
    "ukraine apt",
    "sandworm",
    "charming kitten",
];

/// AlienVault OTX threat intelligence source.
pub struct OtxSource {
    /// Rotating index into SEARCH_QUERIES.
    query_index: Mutex<usize>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OtxPulsesResponse {
    #[serde(default)]
    results: Vec<OtxPulse>,
    #[serde(default)]
    count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OtxPulse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    modified: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    adversary: String,
    #[serde(default)]
    indicators: Vec<serde_json::Value>,
}

impl Default for OtxSource {
    fn default() -> Self {
        Self::new()
    }
}

impl OtxSource {
    pub fn new() -> Self {
        Self {
            query_index: Mutex::new(0),
        }
    }

    /// Advance the query index and return the current query.
    fn next_query(&self) -> &'static str {
        let mut idx = self.query_index.lock().unwrap_or_else(|e| e.into_inner());
        let query = SEARCH_QUERIES[*idx % SEARCH_QUERIES.len()];
        *idx = (*idx + 1) % SEARCH_QUERIES.len();
        query
    }

    /// Fetch subscribed pulses modified since the given ISO date.
    async fn fetch_subscribed(
        &self,
        ctx: &SourceContext,
        api_key: &str,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let since = (Utc::now() - chrono::Duration::hours(2))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();

        let url = format!(
            "https://otx.alienvault.com/api/v1/pulses/subscribed?limit=50&modified_since={}",
            since,
        );

        let resp = ctx
            .http
            .get(&url)
            .header("X-OTX-API-KEY", api_key)
            .send()
            .await?;

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "otx")?;

        let body: OtxPulsesResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Failed to parse OTX subscribed response");
                return Ok(Vec::new());
            }
        };

        let events = body
            .results
            .into_iter()
            .map(|pulse| Self::pulse_to_event(&pulse))
            .collect();

        Ok(events)
    }

    /// Search for pulses matching a query string.
    async fn fetch_search(
        &self,
        ctx: &SourceContext,
        api_key: &str,
        query: &str,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let encoded_query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q", query)
            .finish();
        let encoded_value = &encoded_query["q=".len()..];

        let url = format!(
            "https://otx.alienvault.com/api/v1/search/pulses?q={}&limit=20",
            encoded_value,
        );

        let resp = ctx
            .http
            .get(&url)
            .header("X-OTX-API-KEY", api_key)
            .send()
            .await?;

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "otx")?;

        let body: OtxPulsesResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, query, "Failed to parse OTX search response");
                return Ok(Vec::new());
            }
        };

        let events = body
            .results
            .into_iter()
            .map(|pulse| Self::pulse_to_event(&pulse))
            .collect();

        Ok(events)
    }

    /// Convert an OTX pulse into an InsertableEvent.
    fn pulse_to_event(pulse: &OtxPulse) -> InsertableEvent {
        let data = serde_json::json!({
            "pulse_id": pulse.id,
            "name": pulse.name,
            "description": pulse.description,
            "created": pulse.created,
            "modified": pulse.modified,
            "indicator_count": pulse.indicators.len(),
            "tags": pulse.tags,
            "adversary": pulse.adversary,
        });

        let severity = if !pulse.adversary.is_empty() { Severity::High } else { Severity::Medium };
        let entity_id = if pulse.adversary.is_empty() { None } else { Some(pulse.adversary.clone()) };

        InsertableEvent {
            event_time: Utc::now(),
            source_type: SourceType::Otx,
            source_id: if pulse.id.is_empty() { None } else { Some(pulse.id.clone()) },
            longitude: None,
            latitude: None,
            region_code: None,
            entity_id,
            entity_name: None,
            event_type: EventType::ThreatIntel,
            severity,
            confidence: None,
            tags: pulse.tags.clone(),
            title: if pulse.name.is_empty() { None } else { Some(pulse.name.clone()) },
            description: if pulse.description.is_empty() { None } else { Some(pulse.description.clone()) },
            payload: data,
            heading: None,
            speed: None,
            altitude: None,
        }
    }
}

#[async_trait]
impl DataSource for OtxSource {
    fn id(&self) -> &str {
        "otx"
    }

    fn name(&self) -> &str {
        "AlienVault OTX"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(60 * 60) // 1 hour
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let api_key = match std::env::var("OTX_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                warn!("OTX_API_KEY not set; skipping OTX poll");
                return Ok(Vec::new());
            }
        };

        let mut all_events: Vec<InsertableEvent> = Vec::new();

        // Fetch subscribed pulses.
        match self.fetch_subscribed(ctx, &api_key).await {
            Ok(events) => all_events.extend(events),
            Err(e) => {
                warn!(error = %e, "Failed to fetch OTX subscribed pulses");
            }
        }

        // Rotate through search queries.
        let query = self.next_query();
        debug!(query, "Searching OTX pulses");

        match self.fetch_search(ctx, &api_key, query).await {
            Ok(events) => all_events.extend(events),
            Err(e) => {
                warn!(query, error = %e, "Failed to search OTX pulses");
            }
        }

        debug!(count = all_events.len(), "OTX poll complete");
        Ok(all_events)
    }
}
