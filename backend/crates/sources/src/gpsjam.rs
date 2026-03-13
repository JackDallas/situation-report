use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tracing::debug;

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

pub struct GpsJamSource;

impl Default for GpsJamSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GpsJamSource {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl DataSource for GpsJamSource {
    fn id(&self) -> &str { "gpsjam" }
    fn name(&self) -> &str { "GPSJam Interference Monitor" }
    fn default_interval(&self) -> Duration { Duration::from_secs(6 * 60 * 60) } // 6 hours

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let date = (Utc::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();

        // GPSJam uses a tile-based data endpoint. Try known patterns.
        // The main site loads data at various zoom levels.
        let urls = [
            format!("https://gpsjam.org/api/data/{}", date),
            format!("https://gpsjam.org/data/{}.json", date),
            format!("https://gpsjam.org/api/v1/interference?date={}", date),
        ];

        for url in &urls {
            let resp = match ctx.http.get(url).send().await {
                Ok(r) if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                    // Propagate 429 rate limits to the registry for proper backoff
                    return Err(crate::rate_limit::check_rate_limit(r, "gpsjam").unwrap_err());
                }
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };

            let body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(_) => continue,
            };

            let events = self.parse_data(&body, &date)?;
            if !events.is_empty() {
                return Ok(events);
            }
        }

        // If no API endpoint works, log and return empty
        debug!("GPSJam: no accessible data endpoint found");
        Ok(Vec::new())
    }
}

impl GpsJamSource {
    fn parse_data(&self, body: &serde_json::Value, date: &str) -> anyhow::Result<Vec<InsertableEvent>> {
        let mut events = Vec::new();

        let cells = body.as_array()
            .or_else(|| body.get("data").and_then(|d| d.as_array()))
            .or_else(|| body.get("cells").and_then(|c| c.as_array()));

        if let Some(cells) = cells {
            for cell in cells {
                let lat = cell.get("lat").and_then(|v| v.as_f64());
                let lon = cell.get("lon").or_else(|| cell.get("lng")).and_then(|v| v.as_f64());
                let h3_index = cell.get("h3").or_else(|| cell.get("hex")).and_then(|v| v.as_str()).map(String::from);
                let percentage = cell.get("percentage")
                    .or_else(|| cell.get("jamming"))
                    .or_else(|| cell.get("value"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                if percentage < 10.0 {
                    continue;
                }

                let severity = if percentage > 50.0 {
                    Severity::Critical
                } else if percentage > 30.0 {
                    Severity::High
                } else if percentage > 15.0 {
                    Severity::Medium
                } else {
                    Severity::Low
                };

                events.push(InsertableEvent {
                    event_time: Utc::now(),
                    source_type: SourceType::Gpsjam,
                    source_id: h3_index.clone().or_else(|| Some(format!("{}-{}", date, events.len()))),
                    longitude: lon,
                    latitude: lat,
                    region_code: None,
                    entity_id: h3_index,
                    entity_name: None,
                    event_type: EventType::GpsInterference,
                    severity,
                    confidence: Some(percentage as f32 / 100.0),
                    tags: vec!["gps".to_string(), "interference".to_string(), "navigation".to_string()],
                    title: Some(format!("GPS interference: {:.0}%", percentage)),
                    description: None,
                    payload: cell.clone(),
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }
        }

        debug!(count = events.len(), "GPSJam parsed");
        Ok(events)
    }
}
