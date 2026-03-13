use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use sr_types::{EventType, Severity, SourceType};

use crate::common::region_from_coords;
use crate::{DataSource, InsertableEvent, SourceContext};

/// GDACS GeoJSON endpoint — queries multi-hazard disaster alerts.
/// Supports event types: EQ (earthquake), TC (tropical cyclone), FL (flood),
/// VO (volcano), WF (wildfire), DR (drought).
const BASE_URL: &str = "https://www.gdacs.org/gdacsapi/api/events/geteventlist/SEARCH";

/// Map GDACS event type codes to our internal EventType.
fn map_event_type(gdacs_type: &str) -> EventType {
    match gdacs_type.to_uppercase().as_str() {
        "EQ" => EventType::SeismicEvent,
        "WF" => EventType::ThermalAnomaly,
        // Tropical cyclones, floods, volcanoes, droughts are all geo events
        "TC" | "FL" | "VO" | "DR" => EventType::GeoEvent,
        _ => EventType::GeoEvent,
    }
}

/// Map GDACS alert level strings to our Severity.
fn map_alert_level(level: &str) -> Severity {
    match level.to_lowercase().as_str() {
        "red" => Severity::Critical,
        "orange" => Severity::High,
        "green" => Severity::Medium,
        _ => Severity::Low,
    }
}

/// Human-readable label for GDACS event type codes.
fn event_type_label(gdacs_type: &str) -> &'static str {
    match gdacs_type.to_uppercase().as_str() {
        "EQ" => "Earthquake",
        "TC" => "Tropical Cyclone",
        "FL" => "Flood",
        "VO" => "Volcano",
        "WF" => "Wildfire",
        "DR" => "Drought",
        _ => "Disaster",
    }
}

// ── GeoJSON response structs ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GdacsResponse {
    features: Option<Vec<GdacsFeature>>,
}

#[derive(Debug, Deserialize)]
struct GdacsFeature {
    properties: GdacsProperties,
    geometry: Option<GdacsGeometry>,
}

#[derive(Debug, Deserialize)]
struct GdacsProperties {
    eventid: Option<u64>,
    eventtype: Option<String>,
    alertlevel: Option<String>,
    alertscore: Option<f64>,
    severity: Option<serde_json::Value>,
    #[serde(rename = "severitydata")]
    severity_data: Option<serde_json::Value>,
    country: Option<String>,
    name: Option<String>,
    description: Option<String>,
    fromdate: Option<String>,
    todate: Option<String>,
    url: Option<serde_json::Value>,
    population: Option<serde_json::Value>,
    #[serde(rename = "iscurrent")]
    is_current: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GdacsGeometry {
    coordinates: Option<serde_json::Value>,
}

/// Extract (lon, lat) from GeoJSON geometry coordinates.
/// Handles both Point [lon, lat] and other geometry types.
fn extract_coords(geometry: &Option<GdacsGeometry>) -> Option<(f64, f64)> {
    let geom = geometry.as_ref()?;
    let coords = geom.coordinates.as_ref()?;

    // Point geometry: [lon, lat] or [lon, lat, alt]
    if let Some(arr) = coords.as_array() {
        if arr.len() >= 2 {
            if let (Some(lon), Some(lat)) = (arr[0].as_f64(), arr[1].as_f64()) {
                return Some((lon, lat));
            }
        }
    }

    None
}

pub struct GdacsSource {
    /// Event IDs we have already emitted to avoid broadcasting duplicates
    /// across successive poll cycles.
    seen: Mutex<HashSet<String>>,
}

impl GdacsSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for GdacsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for GdacsSource {
    fn id(&self) -> &str {
        "gdacs"
    }

    fn name(&self) -> &str {
        "GDACS Disaster Alerts"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(600) // 10 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        debug!("Polling GDACS disaster alerts");

        // Query for events in the last 7 days
        let now = Utc::now();
        let from = now - chrono::Duration::days(7);
        let from_str = from.format("%Y-%m-%d").to_string();
        let to_str = now.format("%Y-%m-%d").to_string();

        let url = format!(
            "{}?eventlist=EQ;TC;FL;VO;WF;DR&fromdate={}&todate={}&alertlevel=red;orange;green",
            BASE_URL, from_str, to_str
        );

        let resp = ctx
            .http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;
        let resp = crate::rate_limit::check_rate_limit(resp, "gdacs")?;

        let data: GdacsResponse = resp.json().await?;

        let features = match data.features {
            Some(f) => f,
            None => return Ok(Vec::new()),
        };

        let mut events: Vec<InsertableEvent> = Vec::new();

        for feature in features {
            let event_id = match feature.properties.eventid {
                Some(id) => id,
                None => continue,
            };

            let event_type_code = feature
                .properties
                .eventtype
                .as_deref()
                .unwrap_or("UN");

            // Build stable dedup key
            let source_id = format!("gdacs:{}:{}", event_type_code, event_id);

            // Deduplication — skip events we have already emitted
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&source_id) {
                    continue;
                }
                seen.insert(source_id.clone());
            }

            let alert_level = feature
                .properties
                .alertlevel
                .as_deref()
                .unwrap_or("green");

            let severity = map_alert_level(alert_level);
            let event_type = map_event_type(event_type_code);

            // Extract coordinates from geometry
            let (lon, lat) = match extract_coords(&feature.geometry) {
                Some(coords) => coords,
                None => {
                    debug!(
                        event_id = event_id,
                        event_type = event_type_code,
                        "Skipping GDACS event with no valid coordinates"
                    );
                    continue;
                }
            };

            // Derive region from coordinates
            let region = region_from_coords(lat, lon).unwrap_or("global");

            // Build title from event name/description
            let title = feature
                .properties
                .name
                .clone()
                .or_else(|| feature.properties.description.clone())
                .unwrap_or_else(|| {
                    format!(
                        "GDACS {} Alert — {}",
                        event_type_label(event_type_code),
                        alert_level.to_uppercase()
                    )
                });

            let country = feature.properties.country.as_deref().unwrap_or("");

            // Parse event time from fromdate field
            let event_time = feature
                .properties
                .fromdate
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|| {
                    // Try alternative date formats GDACS may use
                    feature.properties.fromdate.as_ref().and_then(|s| {
                        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                            .ok()
                            .map(|dt| dt.and_utc())
                    })
                })
                .unwrap_or_else(Utc::now);

            // Build tags
            let mut tags = vec![
                "gdacs".to_string(),
                event_type_label(event_type_code).to_lowercase().replace(' ', "_"),
                alert_level.to_lowercase(),
            ];
            if !country.is_empty() {
                tags.push(format!("country:{}", country.to_lowercase()));
            }

            // Build payload with all GDACS properties
            let payload = json!({
                "eventid": event_id,
                "eventtype": event_type_code,
                "alertlevel": alert_level,
                "alertscore": feature.properties.alertscore,
                "severity": feature.properties.severity,
                "severity_data": feature.properties.severity_data,
                "country": country,
                "name": feature.properties.name,
                "description": feature.properties.description,
                "fromdate": feature.properties.fromdate,
                "todate": feature.properties.todate,
                "population": feature.properties.population,
                "is_current": feature.properties.is_current,
                "url": feature.properties.url,
                "lat": lat,
                "lon": lon,
                "region": region,
                "source_api": "gdacs",
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Gdacs,
                source_id: Some(source_id),
                longitude: Some(lon),
                latitude: Some(lat),
                region_code: Some(region.to_string()),
                entity_id: Some(format!("gdacs-{}-{}", event_type_code.to_lowercase(), event_id)),
                entity_name: feature.properties.name.clone(),
                event_type,
                severity,
                confidence: None,
                tags,
                title: Some(title),
                description: feature.properties.description.clone(),
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Prune the seen set if it grows too large
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 10_000 {
                debug!(
                    old_size = seen.len(),
                    "Pruning seen GDACS event IDs set"
                );
                seen.clear();
            }
        }

        if !events.is_empty() {
            debug!(count = events.len(), "GDACS disaster alert events");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_event_type() {
        assert_eq!(map_event_type("EQ"), EventType::SeismicEvent);
        assert_eq!(map_event_type("eq"), EventType::SeismicEvent);
        assert_eq!(map_event_type("WF"), EventType::ThermalAnomaly);
        assert_eq!(map_event_type("TC"), EventType::GeoEvent);
        assert_eq!(map_event_type("FL"), EventType::GeoEvent);
        assert_eq!(map_event_type("VO"), EventType::GeoEvent);
        assert_eq!(map_event_type("DR"), EventType::GeoEvent);
        assert_eq!(map_event_type("XX"), EventType::GeoEvent);
    }

    #[test]
    fn test_map_alert_level() {
        assert_eq!(map_alert_level("Red"), Severity::Critical);
        assert_eq!(map_alert_level("red"), Severity::Critical);
        assert_eq!(map_alert_level("Orange"), Severity::High);
        assert_eq!(map_alert_level("orange"), Severity::High);
        assert_eq!(map_alert_level("Green"), Severity::Medium);
        assert_eq!(map_alert_level("green"), Severity::Medium);
        assert_eq!(map_alert_level("unknown"), Severity::Low);
    }

    #[test]
    fn test_event_type_label() {
        assert_eq!(event_type_label("EQ"), "Earthquake");
        assert_eq!(event_type_label("TC"), "Tropical Cyclone");
        assert_eq!(event_type_label("FL"), "Flood");
        assert_eq!(event_type_label("VO"), "Volcano");
        assert_eq!(event_type_label("WF"), "Wildfire");
        assert_eq!(event_type_label("DR"), "Drought");
        assert_eq!(event_type_label("XX"), "Disaster");
    }

    #[test]
    fn test_extract_coords_point() {
        let geom = Some(GdacsGeometry {
            coordinates: Some(serde_json::json!([51.4, 35.7])),
        });
        let (lon, lat) = extract_coords(&geom).unwrap();
        assert!((lon - 51.4).abs() < f64::EPSILON);
        assert!((lat - 35.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_coords_point_with_altitude() {
        let geom = Some(GdacsGeometry {
            coordinates: Some(serde_json::json!([51.4, 35.7, 10.0])),
        });
        let (lon, lat) = extract_coords(&geom).unwrap();
        assert!((lon - 51.4).abs() < f64::EPSILON);
        assert!((lat - 35.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_coords_none() {
        assert!(extract_coords(&None).is_none());

        let geom = Some(GdacsGeometry {
            coordinates: None,
        });
        assert!(extract_coords(&geom).is_none());
    }

    #[test]
    fn test_extract_coords_insufficient() {
        let geom = Some(GdacsGeometry {
            coordinates: Some(serde_json::json!([51.4])),
        });
        assert!(extract_coords(&geom).is_none());
    }

    #[test]
    fn test_deduplication() {
        let source = GdacsSource::new();
        {
            let mut seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
            seen.insert("gdacs:EQ:12345".to_string());
        }
        let seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
        assert!(seen.contains("gdacs:EQ:12345"));
        assert!(!seen.contains("gdacs:TC:99999"));
    }

    #[test]
    fn test_geojson_parsing() {
        let json_data = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "properties": {
                        "eventid": 1234567,
                        "eventtype": "EQ",
                        "alertlevel": "Orange",
                        "alertscore": 2.5,
                        "severity": {"value": 6.2, "unit": "M"},
                        "country": "Turkey",
                        "name": "M 6.2 - Eastern Turkey",
                        "description": "Earthquake in Eastern Turkey",
                        "fromdate": "2026-03-01T12:00:00Z",
                        "todate": "2026-03-01T12:05:00Z",
                        "iscurrent": "true",
                        "population": {"value": 500000}
                    },
                    "geometry": {
                        "type": "Point",
                        "coordinates": [39.0, 38.5]
                    }
                }
            ]
        }"#;

        let response: GdacsResponse = serde_json::from_str(json_data).unwrap();
        let features = response.features.unwrap();
        assert_eq!(features.len(), 1);

        let f = &features[0];
        assert_eq!(f.properties.eventid, Some(1234567));
        assert_eq!(f.properties.eventtype.as_deref(), Some("EQ"));
        assert_eq!(f.properties.alertlevel.as_deref(), Some("Orange"));
        assert_eq!(f.properties.country.as_deref(), Some("Turkey"));
        assert_eq!(f.properties.name.as_deref(), Some("M 6.2 - Eastern Turkey"));

        let (lon, lat) = extract_coords(&f.geometry).unwrap();
        assert!((lon - 39.0).abs() < f64::EPSILON);
        assert!((lat - 38.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_source_id_format() {
        // Verify the stable dedup key format
        let source_id = format!("gdacs:{}:{}", "EQ", 1234567);
        assert_eq!(source_id, "gdacs:EQ:1234567");
    }
}
