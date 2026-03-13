use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, warn};

use chrono::{DateTime, Utc};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

const FEED_URL: &str =
    "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_hour.geojson";

/// Classification regions for tagging earthquakes by location.
/// Events outside these boxes are tagged "global" instead of being dropped.
const CLASSIFICATION_BOXES: &[(&str, f64, f64, f64, f64)] = &[
    ("middle_east_iran", 44.0, 25.0, 64.0, 40.0),
    ("israel_lebanon_syria", 34.0, 29.0, 37.0, 37.0),
    ("ukraine", 22.0, 44.0, 40.0, 53.0),
    ("red_sea_yemen", 38.0, 10.0, 48.0, 20.0),
    ("western_europe", -12.0, 35.0, 22.0, 72.0),
    ("east_asia", 100.0, 18.0, 150.0, 55.0),
    ("southeast_asia", 90.0, -12.0, 140.0, 18.0),
    ("south_asia", 60.0, 5.0, 100.0, 38.0),
    ("north_america", -170.0, 15.0, -52.0, 72.0),
    ("south_america", -82.0, -56.0, -34.0, 15.0),
    ("oceania", 110.0, -50.0, 180.0, -5.0),
    ("africa", -18.0, -35.0, 52.0, 37.0),
];

/// Top-level GeoJSON response from the USGS earthquake feed.
#[derive(Debug, Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    id: String,
    properties: Properties,
    geometry: Geometry,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Properties {
    mag: Option<f64>,
    place: Option<String>,
    time: Option<i64>,
    updated: Option<i64>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    title: Option<String>,
    alert: Option<String>,
    tsunami: Option<i32>,
    sig: Option<i64>,
    net: Option<String>,
    felt: Option<i64>,
    cdi: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Geometry {
    coordinates: Vec<f64>, // [lon, lat, depth]
}

/// Classify a coordinate into a named region for tagging.
/// Returns "global" for coordinates that don't fall in any named region.
fn region_for(lat: f64, lon: f64) -> &'static str {
    for &(name, west, south, east, north) in CLASSIFICATION_BOXES {
        if lon >= west && lon <= east && lat >= south && lat <= north {
            return name;
        }
    }
    "global"
}

/// Determine whether an event looks like a potential explosion rather than a
/// natural earthquake.  See specification item 7.
fn is_potential_explosion(depth: f64, mag: f64, event_type: &str) -> bool {
    let etype = event_type.to_lowercase();

    // Explicit USGS labels
    if etype == "explosion" || etype == "mining explosion" {
        return true;
    }

    // Surface event (depth reported as zero)
    if depth == 0.0 {
        return true;
    }

    // Shallow + small + not explicitly labelled "earthquake"
    if depth < 5.0 && mag < 4.0 && etype != "earthquake" {
        return true;
    }

    false
}

pub struct UsgsSource {
    /// IDs we have already emitted so we don't broadcast duplicates across
    /// successive poll cycles within the same hour window.
    seen: Mutex<HashSet<String>>,
}

impl UsgsSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for UsgsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for UsgsSource {
    fn id(&self) -> &str {
        "usgs"
    }

    fn name(&self) -> &str {
        "USGS Seismic"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(300) // 5 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        debug!("Polling USGS earthquake feed");

        let resp = ctx.http.get(FEED_URL).send().await?;
        let resp = crate::rate_limit::check_rate_limit(resp, "usgs")?;

        let collection: FeatureCollection = resp.json().await?;

        let mut events: Vec<InsertableEvent> = Vec::new();

        for feature in collection.features {
            // Need at least lon, lat, depth in the coordinates array
            if feature.geometry.coordinates.len() < 3 {
                debug!(id = %feature.id, "Skipping feature with insufficient coordinates");
                continue;
            }

            let lon = feature.geometry.coordinates[0];
            let lat = feature.geometry.coordinates[1];
            let depth = feature.geometry.coordinates[2];

            // Classify region for tagging (no longer filters — all quakes are kept)
            let region = region_for(lat, lon);

            // Deduplication — skip events we have already emitted
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&feature.id) {
                    continue;
                }
                seen.insert(feature.id.clone());
            }

            let mag = feature.properties.mag.unwrap_or(0.0);
            let event_type = feature.properties.event_type.as_deref().unwrap_or("unknown");
            let shallow = depth < 5.0;
            let potential_explosion = is_potential_explosion(depth, mag, event_type);

            // Convert UNIX millis to RFC-3339
            let time_rfc = feature.properties.time.map(|ms| {
                chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            });

            let data = json!({
                "id": feature.id,
                "mag": mag,
                "place": feature.properties.place,
                "time": time_rfc,
                "event_type": event_type,
                "title": feature.properties.title,
                "depth": depth,
                "lat": lat,
                "lon": lon,
                "sig": feature.properties.sig,
                "alert": feature.properties.alert,
                "region": region,
                "shallow": shallow,
                "potential_explosion": potential_explosion,
                "tsunami": feature.properties.tsunami,
                "felt": feature.properties.felt,
                "cdi": feature.properties.cdi,
                "net": feature.properties.net,
            });

            if potential_explosion {
                warn!(
                    id = %feature.id,
                    mag = mag,
                    depth = depth,
                    event_type = event_type,
                    region = region,
                    "Potential explosion detected"
                );
            }

            let severity = if potential_explosion {
                Severity::Critical
            } else if mag > 5.0 {
                Severity::High
            } else if mag > 3.0 {
                Severity::Medium
            } else {
                Severity::Low
            };

            let mut tags = Vec::new();
            if potential_explosion {
                tags.push("potential_explosion".to_string());
            }
            if shallow {
                tags.push("shallow".to_string());
            }

            // Parse event time from the data
            let event_time = time_rfc.as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Usgs,
                source_id: Some(feature.id.clone()),
                longitude: Some(lon),
                latitude: Some(lat),
                region_code: Some(region.to_string()),
                entity_id: Some(feature.id.clone()),
                entity_name: None,
                event_type: EventType::SeismicEvent,
                severity,
                confidence: None,
                tags,
                title: feature.properties.title.clone(),
                description: None,
                payload: data,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Prune the seen set if it grows too large to avoid unbounded memory growth.
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 10_000 {
                debug!(
                    old_size = seen.len(),
                    "Pruning seen USGS event IDs set"
                );
                // Clear and re-process next cycle; some events may be re-emitted,
                // but that is preferable to unbounded memory growth.
                seen.clear();
            }
        }

        if !events.is_empty() {
            debug!(count = events.len(), "USGS seismic events globally");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_matching() {
        // Isfahan, Iran — should match middle_east_iran
        assert_eq!(region_for(32.7, 51.6), "middle_east_iran");

        // Kyiv area — should match ukraine
        assert_eq!(region_for(50.4, 30.5), "ukraine");

        // Tel Aviv — should match israel_lebanon_syria
        assert_eq!(region_for(32.0, 34.8), "israel_lebanon_syria");

        // Aden, Yemen — should match red_sea_yemen
        assert_eq!(region_for(12.8, 45.0), "red_sea_yemen");

        // New York — classified into north_america
        assert_eq!(region_for(40.7, -74.0), "north_america");

        // Middle of Pacific — falls back to "global"
        assert_eq!(region_for(0.0, -170.0), "global");
    }

    #[test]
    fn test_explosion_heuristic() {
        // Surface event
        assert!(is_potential_explosion(0.0, 2.0, "earthquake"));

        // Shallow non-earthquake
        assert!(is_potential_explosion(3.0, 2.5, "unknown"));

        // Explicit explosion label
        assert!(is_potential_explosion(10.0, 5.0, "explosion"));
        assert!(is_potential_explosion(10.0, 5.0, "mining explosion"));

        // Normal earthquake — not flagged
        assert!(!is_potential_explosion(15.0, 5.5, "earthquake"));

        // Shallow but explicitly labelled earthquake — not flagged
        assert!(!is_potential_explosion(3.0, 2.5, "earthquake"));

        // Shallow, small, but earthquake type — not flagged
        assert!(!is_potential_explosion(4.0, 3.0, "earthquake"));
    }

    #[test]
    fn test_deduplication() {
        let source = UsgsSource::new();
        {
            let mut seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
            seen.insert("us7000test".to_string());
        }
        let seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
        assert!(seen.contains("us7000test"));
        assert!(!seen.contains("us7000other"));
    }

    #[test]
    fn test_geojson_parsing() {
        let json_data = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "id": "us7000xxxx",
                    "properties": {
                        "mag": 2.1,
                        "place": "10 km NW of Isfahan, Iran",
                        "time": 1709164800000,
                        "updated": 1709165400000,
                        "type": "earthquake",
                        "title": "M 2.1 - 10 km NW of Isfahan, Iran",
                        "alert": null,
                        "tsunami": 0,
                        "sig": 65,
                        "net": "us",
                        "felt": null,
                        "cdi": null
                    },
                    "geometry": {
                        "type": "Point",
                        "coordinates": [51.6, 32.7, 0.5]
                    }
                }
            ]
        }"#;

        let collection: FeatureCollection = serde_json::from_str(json_data).unwrap();
        assert_eq!(collection.features.len(), 1);

        let f = &collection.features[0];
        assert_eq!(f.id, "us7000xxxx");
        assert_eq!(f.properties.mag, Some(2.1));
        assert_eq!(
            f.properties.place.as_deref(),
            Some("10 km NW of Isfahan, Iran")
        );
        assert_eq!(f.properties.event_type.as_deref(), Some("earthquake"));
        assert_eq!(f.geometry.coordinates, vec![51.6, 32.7, 0.5]);
    }
}
