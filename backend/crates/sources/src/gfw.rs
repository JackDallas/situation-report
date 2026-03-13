use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Utc, Duration as ChronoDuration};
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

struct BBox {
    name: &'static str,
    region: &'static str,
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
}

/// Global query bounding boxes, rotated one per poll to cover major ocean areas.
/// The GFW API supports large bounding boxes, so we use broad oceanic regions.
const QUERY_BBOXES: &[BBox] = &[
    // Middle East / Indian Ocean
    BBox { name: "middle_east_indian_ocean", region: "middle-east", min_lat: -10.0, min_lon: 30.0, max_lat: 32.0, max_lon: 80.0 },
    // East Asia / Western Pacific
    BBox { name: "east_asia_pacific", region: "east-asia", min_lat: -10.0, min_lon: 95.0, max_lat: 50.0, max_lon: 155.0 },
    // Europe / North Atlantic / Mediterranean
    BBox { name: "europe_atlantic", region: "europe", min_lat: 25.0, min_lon: -30.0, max_lat: 72.0, max_lon: 42.0 },
    // West Africa / South Atlantic
    BBox { name: "west_africa_atlantic", region: "africa", min_lat: -40.0, min_lon: -30.0, max_lat: 25.0, max_lon: 25.0 },
    // East Africa / Mozambique Channel
    BBox { name: "east_africa", region: "africa", min_lat: -35.0, min_lon: 25.0, max_lat: 15.0, max_lon: 65.0 },
    // Americas — Eastern Pacific / Caribbean
    BBox { name: "americas_pacific", region: "americas", min_lat: -60.0, min_lon: -130.0, max_lat: 50.0, max_lon: -30.0 },
    // Oceania / South Pacific
    BBox { name: "oceania", region: "oceania", min_lat: -55.0, min_lon: 110.0, max_lat: 0.0, max_lon: 180.0 },
    // Arctic / North Sea
    BBox { name: "arctic_north", region: "arctic", min_lat: 60.0, min_lon: -30.0, max_lat: 85.0, max_lon: 80.0 },
];

/// Individual bounding boxes for sub-region classification.
/// Narrow chokepoints and named seas are checked first.
const BBOXES: &[BBox] = &[
    // Chokepoints
    BBox { name: "hormuz", region: "middle-east", min_lat: 25.5, min_lon: 54.0, max_lat: 27.0, max_lon: 57.0 },
    BBox { name: "bab_el_mandeb", region: "middle-east", min_lat: 12.0, min_lon: 43.0, max_lat: 13.5, max_lon: 44.0 },
    BBox { name: "suez", region: "middle-east", min_lat: 29.5, min_lon: 32.0, max_lat: 31.5, max_lon: 33.0 },
    BBox { name: "malacca", region: "southeast-asia", min_lat: -2.0, min_lon: 95.0, max_lat: 8.0, max_lon: 105.0 },
    BBox { name: "gibraltar", region: "europe", min_lat: 35.5, min_lon: -6.0, max_lat: 36.5, max_lon: -5.0 },
    // Named seas
    BBox { name: "red_sea", region: "middle-east", min_lat: 12.0, min_lon: 38.0, max_lat: 22.0, max_lon: 44.0 },
    BBox { name: "persian_gulf", region: "middle-east", min_lat: 24.0, min_lon: 48.0, max_lat: 30.0, max_lon: 56.0 },
    BBox { name: "mediterranean", region: "europe", min_lat: 30.0, min_lon: -6.0, max_lat: 46.0, max_lon: 36.0 },
    BBox { name: "south_china_sea", region: "east-asia", min_lat: 0.0, min_lon: 100.0, max_lat: 23.0, max_lon: 122.0 },
    BBox { name: "east_china_sea", region: "east-asia", min_lat: 23.0, min_lon: 118.0, max_lat: 35.0, max_lon: 130.0 },
    BBox { name: "north_sea", region: "europe", min_lat: 51.0, min_lon: -5.0, max_lat: 62.0, max_lon: 10.0 },
    BBox { name: "baltic", region: "europe", min_lat: 53.0, min_lon: 10.0, max_lat: 66.0, max_lon: 30.0 },
    BBox { name: "caribbean", region: "americas", min_lat: 10.0, min_lon: -88.0, max_lat: 22.0, max_lon: -60.0 },
    BBox { name: "gulf_of_mexico", region: "americas", min_lat: 18.0, min_lon: -98.0, max_lat: 30.5, max_lon: -80.0 },
    BBox { name: "gulf_of_guinea", region: "africa", min_lat: -5.0, min_lon: -10.0, max_lat: 10.0, max_lon: 15.0 },
    BBox { name: "sea_of_japan", region: "east-asia", min_lat: 33.0, min_lon: 128.0, max_lat: 45.0, max_lon: 142.0 },
    BBox { name: "bay_of_bengal", region: "south-asia", min_lat: 5.0, min_lon: 78.0, max_lat: 22.0, max_lon: 95.0 },
];

/// GFW event datasets to query. We alternate between them on each poll.
const DATASETS: &[&str] = &[
    "public-global-fishing-events:latest",
    "public-global-loitering-events:latest",
];

/// Maximum events per request page.
const PAGE_LIMIT: u32 = 100;

/// GFW Events API v3 base URL (POST endpoint).
const EVENTS_API_URL: &str = "https://gateway.api.globalfishingwatch.org/v3/events";

pub struct GfwSource {
    dataset_index: AtomicUsize,
    query_bbox_index: AtomicUsize,
}

impl Default for GfwSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GfwSource {
    pub fn new() -> Self {
        Self {
            dataset_index: AtomicUsize::new(0),
            query_bbox_index: AtomicUsize::new(0),
        }
    }

    /// Determine which sub-region a coordinate falls in.
    /// Returns "open_ocean" for positions not in any named maritime zone.
    fn classify_region(lat: f64, lon: f64) -> &'static str {
        for bbox in BBOXES {
            if lat >= bbox.min_lat && lat <= bbox.max_lat
                && lon >= bbox.min_lon && lon <= bbox.max_lon
            {
                return bbox.name;
            }
        }
        "open_ocean"
    }

    /// Build a GeoJSON Polygon from a query bounding box for server-side
    /// spatial filtering. The GFW v3 Events API accepts a `geometry` field in
    /// the POST body containing a GeoJSON geometry object.
    fn bbox_to_geojson(bbox: &BBox) -> serde_json::Value {
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [[
                [bbox.min_lon, bbox.min_lat],
                [bbox.max_lon, bbox.min_lat],
                [bbox.max_lon, bbox.max_lat],
                [bbox.min_lon, bbox.max_lat],
                [bbox.min_lon, bbox.min_lat],
            ]]
        })
    }
}

#[async_trait]
impl DataSource for GfwSource {
    fn id(&self) -> &str { "gfw" }
    fn name(&self) -> &str { "Global Fishing Watch" }
    fn default_interval(&self) -> Duration { Duration::from_secs(30 * 60) }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let token = match std::env::var("GFW_API_TOKEN") {
            Ok(t) if !t.is_empty() => t,
            _ => {
                warn!("GFW_API_TOKEN not set; skipping GFW poll");
                return Ok(Vec::new());
            }
        };

        let ds_idx = self.dataset_index.fetch_add(1, Ordering::Relaxed) % DATASETS.len();
        let dataset = DATASETS[ds_idx];

        let bbox_idx = self.query_bbox_index.fetch_add(1, Ordering::Relaxed) % QUERY_BBOXES.len();
        let query_bbox = &QUERY_BBOXES[bbox_idx];

        let end = Utc::now();
        let start = end - ChronoDuration::hours(24);

        // Build the POST request body per GFW Events API v3 spec.
        // Fields use camelCase as required by the API.
        let body = serde_json::json!({
            "datasets": [dataset],
            "startDate": start.format("%Y-%m-%d").to_string(),
            "endDate": end.format("%Y-%m-%d").to_string(),
            "geometry": Self::bbox_to_geojson(query_bbox),
        });

        debug!(dataset, region = query_bbox.name, "Polling GFW events (POST)");

        let mut all_events = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let url = format!("{}?limit={}&offset={}&sort=+start",
                EVENTS_API_URL, PAGE_LIMIT, offset);

            let resp = match ctx.http.post(&url)
                .bearer_auth(&token)
                .json(&body)
                .send().await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "GFW request failed");
                    return Ok(all_events);
                }
            };

            // Propagate 429 rate limits to the registry for proper backoff
            let resp = crate::rate_limit::check_rate_limit(resp, "gfw")?;

            let status = resp.status();
            if !status.is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                warn!(status = %status, body = %body_text, "GFW API returned error");
                return Ok(all_events);
            }

            let resp_body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "Failed to parse GFW response");
                    return Ok(all_events);
                }
            };

            let entries = resp_body.get("entries").and_then(|e| e.as_array())
                .or_else(|| resp_body.as_array());

            let page_count = if let Some(entries) = entries {
                let count = entries.len();
                for entry in entries {
                    if let Some(event) = Self::parse_entry(entry) {
                        all_events.push(event);
                    }
                }
                count
            } else {
                0
            };

            // Check for pagination: if the API returns a nextOffset, continue.
            // Otherwise, stop if we got fewer results than the page limit.
            let next_offset = resp_body.get("nextOffset").and_then(|v| v.as_u64());
            if let Some(next) = next_offset {
                offset = next as u32;
            } else if page_count < PAGE_LIMIT as usize {
                break;
            } else {
                offset += PAGE_LIMIT;
            }
        }

        debug!(count = all_events.len(), dataset, "GFW poll complete");
        Ok(all_events)
    }
}

impl GfwSource {
    /// Parse a single GFW event entry into an InsertableEvent.
    /// Returns None if the entry lacks position data.
    fn parse_entry(entry: &serde_json::Value) -> Option<InsertableEvent> {
        let lat = entry.pointer("/position/lat").and_then(|v| v.as_f64());
        let lon = entry.pointer("/position/lon").and_then(|v| v.as_f64());

        // Skip entries without position data.
        let (la, lo) = match (lat, lon) {
            (Some(la), Some(lo)) => (la, lo),
            _ => return None,
        };

        let sub_region = Self::classify_region(la, lo);

        // Look up the parent region from the classification BBOXES
        let region_code = BBOXES.iter()
            .find(|b| b.name == sub_region)
            .map(|b| b.region)
            .unwrap_or("global");

        let event_id = entry.get("id").and_then(|v| v.as_str()).map(String::from);
        let vessel_id = entry.pointer("/vessel/id").and_then(|v| v.as_str()).map(String::from);
        let vessel_name = entry.pointer("/vessel/name").and_then(|v| v.as_str()).map(String::from);
        let event_type_raw = entry.get("type").and_then(|v| v.as_str()).unwrap_or("fishing");

        let event_time = entry.get("start").and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        Some(InsertableEvent {
            event_time,
            source_type: SourceType::Gfw,
            source_id: event_id,
            longitude: Some(lo),
            latitude: Some(la),
            region_code: Some(region_code.to_string()),
            entity_id: vessel_id,
            entity_name: vessel_name.clone(),
            event_type: EventType::FishingEvent,
            severity: Severity::Low,
            confidence: None,
            tags: vec!["maritime".to_string(), event_type_raw.to_string(), sub_region.to_string()],
            title: Some(format!("{}: {}", event_type_raw, vessel_name.as_deref().unwrap_or("unknown"))),
            description: None,
            payload: entry.clone(),
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_region() {
        // Strait of Hormuz
        assert_eq!(GfwSource::classify_region(26.0, 56.0), "hormuz");
        // Red Sea
        assert_eq!(GfwSource::classify_region(15.0, 42.0), "red_sea");
        // Persian Gulf
        assert_eq!(GfwSource::classify_region(27.0, 50.0), "persian_gulf");
        // Outside specific sub-regions
        assert_eq!(GfwSource::classify_region(23.0, 46.0), "open_ocean");
    }

    #[test]
    fn test_dataset_rotation() {
        let source = GfwSource::new();
        // First poll uses fishing events
        let idx0 = source.dataset_index.fetch_add(1, Ordering::Relaxed) % DATASETS.len();
        assert_eq!(DATASETS[idx0], "public-global-fishing-events:latest");
        // Second poll uses loitering events
        let idx1 = source.dataset_index.fetch_add(1, Ordering::Relaxed) % DATASETS.len();
        assert_eq!(DATASETS[idx1], "public-global-loitering-events:latest");
        // Wraps back
        let idx2 = source.dataset_index.fetch_add(1, Ordering::Relaxed) % DATASETS.len();
        assert_eq!(DATASETS[idx2], "public-global-fishing-events:latest");
    }

    #[test]
    fn test_bbox_to_geojson() {
        let bbox = &QUERY_BBOXES[0];
        let geojson = GfwSource::bbox_to_geojson(bbox);
        assert_eq!(geojson["type"], "Polygon");

        let coords = geojson["coordinates"][0].as_array().unwrap();
        assert_eq!(coords.len(), 5, "Polygon ring must have 5 points (closed)");

        // First and last point must be the same (closed ring).
        assert_eq!(coords[0], coords[4]);

        // Verify corners match the bbox.
        assert_eq!(coords[0][0].as_f64().unwrap(), bbox.min_lon);
        assert_eq!(coords[0][1].as_f64().unwrap(), bbox.min_lat);
        assert_eq!(coords[1][0].as_f64().unwrap(), bbox.max_lon);
        assert_eq!(coords[1][1].as_f64().unwrap(), bbox.min_lat);
        assert_eq!(coords[2][0].as_f64().unwrap(), bbox.max_lon);
        assert_eq!(coords[2][1].as_f64().unwrap(), bbox.max_lat);
        assert_eq!(coords[3][0].as_f64().unwrap(), bbox.min_lon);
        assert_eq!(coords[3][1].as_f64().unwrap(), bbox.max_lat);
    }

    #[test]
    fn test_parse_entry_valid() {
        let entry = serde_json::json!({
            "id": "abc123",
            "type": "fishing",
            "start": "2024-06-15T10:00:00.000Z",
            "end": "2024-06-15T12:00:00.000Z",
            "position": { "lat": 20.0, "lon": 42.0 },
            "vessel": { "id": "vessel-1", "name": "Test Vessel" }
        });

        let event = GfwSource::parse_entry(&entry).expect("should parse valid entry");
        assert_eq!(event.source_type, SourceType::Gfw);
        assert_eq!(event.source_id, Some("abc123".to_string()));
        assert_eq!(event.event_type, EventType::FishingEvent);
        assert_eq!(event.entity_id, Some("vessel-1".to_string()));
        assert_eq!(event.entity_name, Some("Test Vessel".to_string()));
        assert_eq!(event.latitude, Some(20.0));
        assert_eq!(event.longitude, Some(42.0));
        assert!(event.tags.contains(&"maritime".to_string()));
        assert!(event.tags.contains(&"fishing".to_string()));
        assert!(event.tags.contains(&"red_sea".to_string()));
    }

    #[test]
    fn test_parse_entry_loitering() {
        let entry = serde_json::json!({
            "id": "loit-1",
            "type": "loitering",
            "start": "2024-06-15T10:00:00.000Z",
            "position": { "lat": 26.5, "lon": 55.0 },
            "vessel": { "id": "v-2", "name": "Carrier Ship" }
        });

        let event = GfwSource::parse_entry(&entry).expect("should parse loitering entry");
        assert_eq!(event.event_type, EventType::FishingEvent);
        assert!(event.tags.contains(&"hormuz".to_string()));
    }

    #[test]
    fn test_parse_entry_no_position() {
        let entry = serde_json::json!({
            "id": "no-pos",
            "type": "fishing",
            "start": "2024-06-15T10:00:00.000Z",
        });

        assert!(GfwSource::parse_entry(&entry).is_none());
    }

    #[test]
    fn test_parse_entry_in_sub_region() {
        // (5.0, 10.0) falls in the gulf_of_guinea sub-region (africa parent)
        let entry = serde_json::json!({
            "id": "guinea",
            "type": "fishing",
            "start": "2024-06-15T10:00:00.000Z",
            "position": { "lat": 5.0, "lon": 10.0 },
            "vessel": { "id": "v-3" }
        });

        let event = GfwSource::parse_entry(&entry).expect("should still parse");
        assert_eq!(event.region_code, Some("africa".to_string()));
        assert!(event.tags.contains(&"gulf_of_guinea".to_string()));
    }

    #[test]
    fn test_parse_entry_open_ocean() {
        // Position far from any named sub-region classifies as open_ocean/global
        let entry = serde_json::json!({
            "id": "deep-ocean",
            "type": "fishing",
            "start": "2024-06-15T10:00:00.000Z",
            "position": { "lat": -45.0, "lon": -100.0 },
            "vessel": { "id": "v-4" }
        });

        let event = GfwSource::parse_entry(&entry).expect("should parse");
        assert_eq!(event.region_code, Some("global".to_string()));
        assert!(event.tags.contains(&"open_ocean".to_string()));
    }

    #[test]
    fn test_post_body_structure() {
        // Verify the POST body we construct matches the GFW v3 API expectations.
        let dataset = DATASETS[0];
        let bbox = &QUERY_BBOXES[0];
        let body = serde_json::json!({
            "datasets": [dataset],
            "startDate": "2024-06-15",
            "endDate": "2024-06-16",
            "geometry": GfwSource::bbox_to_geojson(bbox),
        });

        // datasets must be an array
        assert!(body["datasets"].is_array());
        assert_eq!(body["datasets"][0], "public-global-fishing-events:latest");

        // dates are strings in YYYY-MM-DD format
        assert_eq!(body["startDate"], "2024-06-15");
        assert_eq!(body["endDate"], "2024-06-16");

        // geometry must be a GeoJSON Polygon
        assert_eq!(body["geometry"]["type"], "Polygon");
        assert!(body["geometry"]["coordinates"].is_array());
    }
}
