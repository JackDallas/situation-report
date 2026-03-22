use std::time::Duration;
use std::sync::Mutex;

use chrono::Utc;
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

/// Queries for the GDELT GEO 2.0 endpoint.  The geo API resolves geographic
/// entities, so terms should be place names (not event descriptions like
/// "cyber attack" which return 404).
const GEO_QUERIES: &[&str] = &[
    "iran",
    "ukraine",
    "yemen",
    "israel",
    "syria",
    "persian gulf",
    "sudan",
];

pub struct GdeltGeoSource {
    query_index: Mutex<usize>,
}

impl Default for GdeltGeoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GdeltGeoSource {
    pub fn new() -> Self {
        Self { query_index: Mutex::new(0) }
    }

    fn next_query(&self) -> &'static str {
        let mut idx = self.query_index.lock().unwrap_or_else(|e| e.into_inner());
        let query = GEO_QUERIES[*idx % GEO_QUERIES.len()];
        *idx = (*idx + 1) % GEO_QUERIES.len();
        query
    }

    /// Map GDELT GEO query to a region code for clustering
    fn query_region(query: &str) -> Option<String> {
        match query {
            "iran" | "persian gulf" | "israel" | "syria" | "yemen" => {
                Some("ME".to_string())
            }
            "ukraine" => Some("EU".to_string()),
            "sudan" => Some("AF".to_string()),
            _ => None,
        }
    }
}

impl DataSource for GdeltGeoSource {
    fn id(&self) -> &str { "gdelt-geo" }
    fn name(&self) -> &str { "GDELT GEO 2.0" }
    fn default_interval(&self) -> Duration { Duration::from_secs(20 * 60) }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let query = self.next_query();
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("query", query)
            .finish();
        let encoded_value = &encoded["query=".len()..];

        let url = format!(
            "https://api.gdeltproject.org/api/v2/geo/geo?query={}&format=GeoJSON&maxpoints=250",
            encoded_value
        );

        debug!(query, "Polling GDELT GEO 2.0");

        let resp = match ctx.http.get(&url).timeout(Duration::from_secs(15)).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, query, "GDELT GEO request failed, retrying once");
                tokio::time::sleep(Duration::from_secs(2)).await;
                match ctx.http.get(&url).timeout(Duration::from_secs(15)).send().await {
                    Ok(r) => r,
                    Err(e2) => {
                        warn!(error = %e2, query, "GDELT GEO retry also failed");
                        return Err(anyhow::anyhow!("GDELT GEO request failed after retry: {e2}"));
                    }
                }
            }
        };

        // GDELT GEO returns 404 for queries it can't resolve geographically
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            debug!(query, "GDELT GEO returned 404 for query, skipping");
            return Ok(Vec::new());
        }

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "gdelt-geo")?;

        let body_text = resp.text().await?;
        let body: serde_json::Value = match serde_json::from_str(&body_text) {
            Ok(v) => v,
            Err(e) => {
                if body_text.trim().is_empty() || body_text.trim().starts_with('<') {
                    // Empty body or HTML error page — service is down/broken
                    return Err(anyhow::anyhow!("GDELT GEO returned non-JSON response ({} bytes): {e}", body_text.len()));
                }
                // Non-empty JSON-ish body that failed to parse — legitimate no-results
                warn!(error = %e, body_len = body_text.len(), query, "Failed to parse GDELT GEO response");
                return Ok(Vec::new());
            }
        };

        let mut events = Vec::new();

        if let Some(features) = body.get("features").and_then(|f| f.as_array()) {
            for feature in features {
                let coords = feature.pointer("/geometry/coordinates").and_then(|c| c.as_array());
                let (lon, lat) = match coords {
                    Some(c) if c.len() >= 2 => (c[0].as_f64(), c[1].as_f64()),
                    _ => continue,
                };

                let props = feature.get("properties").cloned().unwrap_or_default();
                let name = props.get("name").and_then(|v| v.as_str()).map(String::from);
                let url_val = props.get("url").and_then(|v| v.as_str()).map(String::from);
                let _html = props.get("html").and_then(|v| v.as_str()).unwrap_or("");
                let _count = props.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                let tone = props.get("tone").and_then(|v| v.as_f64()).unwrap_or(0.0);

                let severity = if tone < -5.0 { Severity::High } else if tone < -2.0 { Severity::Medium } else { Severity::Low };

                events.push(InsertableEvent {
                    event_time: Utc::now(),
                    source_type: SourceType::GdeltGeo,
                    source_id: url_val.clone(),
                    longitude: lon,
                    latitude: lat,
                    region_code: Self::query_region(query),
                    entity_id: None,
                    entity_name: name.clone(),
                    event_type: EventType::GeoNews,
                    severity,
                    confidence: None,
                    tags: vec![format!("source:gdelt"), format!("query:{}", query)],
                    title: name,
                    description: url_val,
                    payload: feature.clone(),
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }
        }

        debug!(count = events.len(), query, "GDELT GEO complete");
        Ok(events)
        })
    }
}
