use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

/// Monitored regions: (label, latitude, longitude, distance_km)
const REGIONS: &[(&str, f64, f64, u32)] = &[
    ("iran", 32.0, 51.0, 1000),
    ("turkey", 39.0, 35.0, 1000),
    ("gulf", 26.0, 50.0, 1000),
];

/// Normal background radiation in CPM — below this is unremarkable.
const BASELINE_CPM: f64 = 50.0;

/// Threshold above which we flag a reading as an alert.
const ALERT_CPM: f64 = 100.0;

/// Safecast API measurement response item.
#[derive(Debug, Deserialize)]
struct SafecastMeasurement {
    id: Option<u64>,
    value: Option<f64>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    captured_at: Option<String>,
    unit: Option<String>,
    device_id: Option<u64>,
    location_name: Option<String>,
    height: Option<f64>,
}

/// Classify a CPM reading into a severity level.
fn severity(cpm: f64) -> &'static str {
    if cpm > ALERT_CPM {
        "alert"
    } else if cpm > BASELINE_CPM {
        "elevated"
    } else {
        "normal"
    }
}

pub struct NuclearSource {
    /// Measurement IDs already emitted, to avoid broadcasting duplicates.
    seen: Mutex<HashSet<u64>>,
}

impl NuclearSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for NuclearSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for NuclearSource {
    fn id(&self) -> &str {
        "nuclear"
    }

    fn name(&self) -> &str {
        "Nuclear / Radiation"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(1800) // 30 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        debug!("Polling Safecast radiation data");

        let mut events: Vec<InsertableEvent> = Vec::new();

        // Query each monitored region
        for &(region_label, lat, lon, distance_km) in REGIONS {
            // Look back 24 hours for recent measurements
            let captured_after = (Utc::now() - chrono::Duration::hours(24))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string();

            let url = format!(
                "https://api.safecast.org/measurements.json?distance={}&latitude={}&longitude={}&captured_after={}",
                distance_km, lat, lon, captured_after
            );

            let resp = match ctx.http.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(region = region_label, error = %e, "Failed to fetch Safecast data");
                    continue;
                }
            };

            // Propagate 429 rate limits to the registry for proper backoff
            let resp = crate::rate_limit::check_rate_limit(resp, "nuclear")?;

            let measurements: Vec<SafecastMeasurement> = match resp.json().await {
                Ok(m) => m,
                Err(e) => {
                    warn!(region = region_label, error = %e, "Failed to parse Safecast response");
                    continue;
                }
            };

            debug!(
                region = region_label,
                count = measurements.len(),
                "Safecast measurements received"
            );

            for m in measurements {
                let measurement_id = match m.id {
                    Some(id) => id,
                    None => continue,
                };

                // Deduplication
                {
                    let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                    if seen.contains(&measurement_id) {
                        continue;
                    }
                    seen.insert(measurement_id);
                }

                let cpm = m.value.unwrap_or(0.0);
                let level = severity(cpm);

                let data = json!({
                    "measurement_id": measurement_id,
                    "value_cpm": cpm,
                    "unit": m.unit.as_deref().unwrap_or("cpm"),
                    "latitude": m.latitude,
                    "longitude": m.longitude,
                    "captured_at": m.captured_at,
                    "device_id": m.device_id,
                    "location_name": m.location_name,
                    "height": m.height,
                    "region": region_label,
                    "severity": level,
                    "source_api": "safecast",
                });

                if level == "alert" {
                    warn!(
                        measurement_id = measurement_id,
                        cpm = cpm,
                        region = region_label,
                        "ELEVATED radiation reading detected (>{} CPM)",
                        ALERT_CPM
                    );
                } else if level == "elevated" {
                    info!(
                        measurement_id = measurement_id,
                        cpm = cpm,
                        region = region_label,
                        "Above-baseline radiation reading (>{} CPM)",
                        BASELINE_CPM
                    );
                }

                let sev = match level {
                    "alert" => Severity::Critical,
                    "elevated" => Severity::High,
                    _ => Severity::Low,
                };

                let title = format!(
                    "Radiation: {:.1} CPM ({})",
                    cpm, level
                );

                events.push(InsertableEvent {
                    event_time: Utc::now(),
                    source_type: SourceType::Nuclear,
                    source_id: Some(self.id().to_string()),
                    longitude: m.longitude,
                    latitude: m.latitude,
                    region_code: Some(region_label.to_string()),
                    entity_id: Some(measurement_id.to_string()),
                    entity_name: m.location_name.clone(),
                    event_type: EventType::NuclearEvent,
                    severity: sev,
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
        }

        // Prune the seen set if it grows too large (keep only last 50k IDs)
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 50_000 {
                debug!(
                    old_size = seen.len(),
                    "Pruning seen measurement IDs set"
                );
                // Just clear it — we will re-fetch recent measurements next cycle
                // and some may be re-emitted, but that is preferable to unbounded memory growth.
                seen.clear();
            }
        }

        if !events.is_empty() {
            debug!(count = events.len(), "Nuclear radiation events emitted");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_classification() {
        assert_eq!(severity(20.0), "normal");
        assert_eq!(severity(50.0), "normal");
        assert_eq!(severity(51.0), "elevated");
        assert_eq!(severity(100.0), "elevated");
        assert_eq!(severity(101.0), "alert");
        assert_eq!(severity(500.0), "alert");
    }

    #[test]
    fn test_deduplication() {
        let source = NuclearSource::new();
        {
            let mut seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
            seen.insert(12345);
        }
        let seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
        assert!(seen.contains(&12345));
        assert!(!seen.contains(&99999));
    }

    #[test]
    fn test_regions_defined() {
        assert_eq!(REGIONS.len(), 3);
        assert_eq!(REGIONS[0].0, "iran");
        assert_eq!(REGIONS[1].0, "turkey");
        assert_eq!(REGIONS[2].0, "gulf");
    }

    #[test]
    fn test_safecast_measurement_parsing() {
        let json_data = r#"[
            {
                "id": 12345,
                "value": 35.2,
                "latitude": 32.5,
                "longitude": 51.3,
                "captured_at": "2026-02-28T14:30:00Z",
                "unit": "cpm",
                "device_id": 42,
                "location_name": "Isfahan area",
                "height": null
            }
        ]"#;

        let measurements: Vec<SafecastMeasurement> = serde_json::from_str(json_data).unwrap();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].id, Some(12345));
        assert_eq!(measurements[0].value, Some(35.2));
        assert_eq!(measurements[0].latitude, Some(32.5));
        assert_eq!(measurements[0].longitude, Some(51.3));
        assert_eq!(measurements[0].unit.as_deref(), Some("cpm"));
    }
}
