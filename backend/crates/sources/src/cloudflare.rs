use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use crate::common::region_for_country;
use crate::{DataSource, InsertableEvent, SourceContext};

/// Countries monitored for internet outages and traffic anomalies.
const COUNTRIES: &[&str] = &[
    "IR", "IL", "UA", "RU", "YE", "SY", "SD", "MM", "BH", "QA", "KW", "AE",
];

/// Number of countries to check per poll cycle.
const COUNTRIES_PER_CYCLE: usize = 2;

/// Cloudflare Radar outage and traffic anomaly monitor.
pub struct CloudflareSource {
    /// Rotating index into the COUNTRIES list.
    country_index: Mutex<usize>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflareOutageResponse {
    #[serde(default)]
    result: Option<CloudflareOutageResult>,
    #[serde(default)]
    success: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CloudflareOutageResult {
    #[serde(default)]
    annotations: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflareAnomalyResponse {
    #[serde(default)]
    result: Option<CloudflareAnomalyResult>,
    #[serde(default)]
    success: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CloudflareAnomalyResult {
    #[serde(default, rename = "trafficAnomalies")]
    traffic_anomalies: Vec<serde_json::Value>,
}

impl Default for CloudflareSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudflareSource {
    pub fn new() -> Self {
        Self {
            country_index: Mutex::new(0),
        }
    }

    /// Advance the country index and return a slice of countries for this cycle.
    fn next_countries(&self) -> Vec<&'static str> {
        let mut idx = self.country_index.lock().unwrap_or_else(|e| e.into_inner());
        let mut batch = Vec::with_capacity(COUNTRIES_PER_CYCLE);
        for _ in 0..COUNTRIES_PER_CYCLE {
            batch.push(COUNTRIES[*idx % COUNTRIES.len()]);
            *idx = (*idx + 1) % COUNTRIES.len();
        }
        batch
    }

    /// Fetch outage annotations for a given country.
    async fn fetch_outages(
        &self,
        ctx: &SourceContext,
        token: &str,
        cc: &str,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/radar/annotations/outages?dateRange=7d&location={}",
            cc,
        );

        let resp = ctx
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "cloudflare")?;

        let body: CloudflareOutageResponse = resp.json().await?;

        let annotations = match body.result {
            Some(r) => r.annotations,
            None => return Ok(Vec::new()),
        };

        let region = region_for_country(cc);
        let mut events: Vec<InsertableEvent> = Vec::new();
        for annotation in annotations {
            let outage_type = annotation
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let start_date = annotation
                .get("startDate")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let end_date = annotation
                .get("endDate")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let description = annotation
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let title = format!("Internet outage: {} ({})", cc, outage_type);
            let source_id = format!("cf-outage:{}:{}:{}", cc, outage_type, start_date);

            let data = serde_json::json!({
                "country": cc,
                "outage_type": outage_type,
                "start_time": start_date,
                "end_time": end_date,
                "cause_classification": outage_type,
                "description": description,
                "source_api": "cloudflare_outages",
                "raw": annotation,
                "is_outage": true,
            });

            events.push(InsertableEvent {
                event_time: Utc::now(),
                source_type: SourceType::Cloudflare,
                source_id: Some(source_id),
                longitude: None,
                latitude: None,
                region_code: region.map(String::from),
                entity_id: None,
                entity_name: None,
                event_type: EventType::InternetOutage,
                severity: Severity::High,
                confidence: None,
                tags: vec![],
                title: Some(title),
                description: None,
                payload: data,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        Ok(events)
    }

    /// Fetch traffic anomalies for a given country.
    async fn fetch_anomalies(
        &self,
        ctx: &SourceContext,
        token: &str,
        cc: &str,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/radar/traffic_anomalies?location={}&dateRange=7d",
            cc,
        );

        let resp = ctx
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "cloudflare")?;

        let body: CloudflareAnomalyResponse = resp.json().await?;

        let anomalies = match body.result {
            Some(r) => r.traffic_anomalies,
            None => return Ok(Vec::new()),
        };

        let region = region_for_country(cc);
        let mut events: Vec<InsertableEvent> = Vec::new();
        for anomaly in anomalies {
            let start_date = anomaly
                .get("startDate")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let end_date = anomaly
                .get("endDate")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let anomaly_type = anomaly
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("traffic_anomaly");

            let title = format!("Internet outage: {} (traffic_anomaly)", cc);
            let source_id = format!("cf-anomaly:{}:{}:{}", cc, anomaly_type, start_date);

            let data = serde_json::json!({
                "country": cc,
                "outage_type": "traffic_anomaly",
                "start_time": start_date,
                "end_time": end_date,
                "cause_classification": anomaly_type,
                "source_api": "cloudflare_traffic_anomalies",
                "raw": anomaly,
                "is_outage": false,
            });

            events.push(InsertableEvent {
                event_time: Utc::now(),
                source_type: SourceType::Cloudflare,
                source_id: Some(source_id),
                longitude: None,
                latitude: None,
                region_code: region.map(String::from),
                entity_id: None,
                entity_name: None,
                event_type: EventType::InternetOutage,
                severity: Severity::Medium,
                confidence: None,
                tags: vec![],
                title: Some(title),
                description: None,
                payload: data,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        Ok(events)
    }
}

#[async_trait]
impl DataSource for CloudflareSource {
    fn id(&self) -> &str {
        "cloudflare"
    }

    fn name(&self) -> &str {
        "Cloudflare Radar"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(15 * 60) // 15 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let token = match std::env::var("CLOUDFLARE_API_TOKEN") {
            Ok(t) if !t.is_empty() => t,
            _ => {
                warn!("CLOUDFLARE_API_TOKEN not set; skipping Cloudflare poll");
                return Ok(Vec::new());
            }
        };

        let countries = self.next_countries();
        let mut all_events: Vec<InsertableEvent> = Vec::new();

        for cc in &countries {
            debug!(country = cc, "Polling Cloudflare Radar");

            match self.fetch_outages(ctx, &token, cc).await {
                Ok(events) => all_events.extend(events),
                Err(e) => {
                    warn!(country = cc, error = %e, "Failed to fetch Cloudflare outages");
                }
            }

            match self.fetch_anomalies(ctx, &token, cc).await {
                Ok(events) => all_events.extend(events),
                Err(e) => {
                    warn!(country = cc, error = %e, "Failed to fetch Cloudflare anomalies");
                }
            }

            // Small delay between countries to be polite to the API.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        debug!(count = all_events.len(), "Cloudflare poll complete");
        Ok(all_events)
    }
}

/// Cloudflare BGP leak detection source.
pub struct CloudflareBgpSource;

impl Default for CloudflareBgpSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudflareBgpSource {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl DataSource for CloudflareBgpSource {
    fn id(&self) -> &str { "cloudflare-bgp" }
    fn name(&self) -> &str { "Cloudflare BGP Leak Monitor" }
    fn default_interval(&self) -> Duration { Duration::from_secs(30 * 60) }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let token = match std::env::var("CLOUDFLARE_API_TOKEN") {
            Ok(t) if !t.is_empty() => t,
            _ => {
                tracing::warn!("CLOUDFLARE_API_TOKEN not set; skipping BGP leak poll");
                return Ok(Vec::new());
            }
        };

        let url = "https://api.cloudflare.com/client/v4/radar/bgp/leaks/events?per_page=25&sort_by=time&sort_order=desc";
        let resp = ctx.http.get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await?;

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "cloudflare-bgp")?;

        let body: serde_json::Value = resp.json().await?;
        let mut events = Vec::new();

        // The API returns { result: { data: [...] } } or { result: { asn_info: [...] } }
        let items = body.pointer("/result/data").and_then(|v| v.as_array());

        if let Some(leaks) = items {
            for leak in leaks {
                let origin_asn = leak.get("origin_asn").and_then(|v| v.as_u64());
                let leak_asn = leak.get("leak_asn").and_then(|v| v.as_u64());
                let prefix_count = leak.get("prefix_count").and_then(|v| v.as_u64()).unwrap_or(0);

                let detected = leak.get("detected_ts").and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);

                let entity_id = origin_asn.map(|asn| format!("AS{}", asn));
                let dedup_id = format!("{}-{}", origin_asn.unwrap_or(0), detected.timestamp());

                events.push(InsertableEvent {
                    event_time: detected,
                    source_type: SourceType::CloudflareBgp,
                    source_id: Some(dedup_id),
                    longitude: None,
                    latitude: None,
                    region_code: None,
                    entity_id,
                    entity_name: leak_asn.map(|a| format!("AS{}", a)),
                    event_type: EventType::BgpLeak,
                    severity: Severity::High,
                    confidence: None,
                    tags: vec!["bgp".to_string(), "leak".to_string()],
                    title: Some(format!("BGP leak: AS{} via AS{} ({} prefixes)",
                        origin_asn.unwrap_or(0), leak_asn.unwrap_or(0), prefix_count)),
                    description: None,
                    payload: leak.clone(),
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }
        }

        tracing::debug!(count = events.len(), "Cloudflare BGP leak poll complete");
        Ok(events)
    }
}
