use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};
use crate::common::region_for_country;

/// Countries monitored for censorship via OONI.
const COUNTRIES: &[&str] = &["IR", "RU", "UA", "MM", "SD", "SY", "YE"];

/// OONI censorship measurement source.
pub struct OoniSource {
    /// Rotating index into the COUNTRIES list.
    country_index: Mutex<usize>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OoniResponse {
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default)]
    results: Vec<OoniMeasurement>,
}

#[derive(Debug, Deserialize)]
struct OoniMeasurement {
    #[serde(default)]
    measurement_uid: String,
    #[serde(default)]
    probe_cc: String,
    #[serde(default)]
    test_name: String,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    measurement_start_time: String,
    #[serde(default)]
    anomaly: bool,
    #[serde(default)]
    confirmed: bool,
    #[serde(default)]
    failure: bool,
}

impl Default for OoniSource {
    fn default() -> Self {
        Self::new()
    }
}

impl OoniSource {
    pub fn new() -> Self {
        Self {
            country_index: Mutex::new(0),
        }
    }

    /// Advance the country index and return the current country code.
    fn next_country(&self) -> &'static str {
        let mut idx = self.country_index.lock().unwrap_or_else(|e| e.into_inner());
        let cc = COUNTRIES[*idx % COUNTRIES.len()];
        *idx = (*idx + 1) % COUNTRIES.len();
        cc
    }
}

impl DataSource for OoniSource {
    fn id(&self) -> &str {
        "ooni"
    }

    fn name(&self) -> &str {
        "OONI Censorship"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(30 * 60) // 30 minutes
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let cc = self.next_country();

        // Look back 2 hours to capture recent measurements.
        let since = (Utc::now() - chrono::Duration::hours(2))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();

        let url = format!(
            "https://api.ooni.io/api/v1/measurements?probe_cc={}&test_name=web_connectivity&since={}&limit=100",
            cc, since,
        );

        debug!(country = cc, "Polling OONI measurements");

        let resp = match ctx.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(country = cc, error = %e, "OONI request failed");
                return Ok(Vec::new());
            }
        };

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "ooni")?;

        let body: OoniResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(country = cc, error = %e, "Failed to parse OONI response");
                return Ok(Vec::new());
            }
        };

        let region = region_for_country(cc);
        let mut events: Vec<InsertableEvent> = Vec::new();

        for measurement in &body.results {
            // Only emit events for anomalous or confirmed-blocked measurements.
            if !measurement.anomaly && !measurement.confirmed {
                continue;
            }

            let input_url = measurement.input.as_deref().unwrap_or("");
            let confirmed = measurement.confirmed;
            let anomaly = measurement.anomaly;

            let severity = if confirmed {
                Severity::High
            } else if anomaly {
                Severity::Medium
            } else {
                Severity::Low
            };

            let title = format!(
                "Censorship {}: {} in {}",
                if confirmed { "confirmed" } else { "anomaly" },
                input_url,
                cc
            );

            let data = serde_json::json!({
                "measurement_uid": measurement.measurement_uid,
                "probe_cc": measurement.probe_cc,
                "test_name": measurement.test_name,
                "input": input_url,
                "anomaly": anomaly,
                "confirmed": confirmed,
                "failure": measurement.failure,
                "measurement_start_time": measurement.measurement_start_time,
            });

            events.push(InsertableEvent {
                event_time: Utc::now(),
                source_type: SourceType::Ooni,
                source_id: if measurement.measurement_uid.is_empty() { None } else { Some(measurement.measurement_uid.clone()) },
                longitude: None,
                latitude: None,
                region_code: region.map(String::from),
                entity_id: None,
                entity_name: None,
                event_type: EventType::CensorshipEvent,
                severity,
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

        debug!(count = events.len(), country = cc, "OONI poll complete");
        Ok(events)
        })
    }
}
