use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

const API_URL: &str =
    "https://rapidmapping.emergency.copernicus.eu/backend/dashboard-api/public-activations-info/";

/// How far back (in days) to consider activations on the first poll.
const LOOKBACK_DAYS: i64 = 7;

/// Paginated response wrapper from the Copernicus dashboard API.
#[derive(Debug, Deserialize)]
struct ActivationsResponse {
    results: Vec<Activation>,
}

/// Copernicus EMS activation record returned by the dashboard API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Activation {
    code: Option<String>,
    #[serde(default)]
    countries: Vec<String>,
    #[serde(rename = "eventTime")]
    event_time: Option<String>,
    name: Option<String>,
    centroid: Option<String>,
    #[serde(rename = "activationTime")]
    activation_time: Option<String>,
    category: Option<String>,
    #[serde(rename = "lastUpdate")]
    last_update: Option<String>,
    closed: Option<bool>,
    #[serde(rename = "gdacsId")]
    gdacs_id: Option<String>,
    n_aois: Option<i64>,
    n_products: Option<i64>,
}

/// Parse a WKT POINT string into (latitude, longitude).
///
/// WKT convention is `POINT (lon lat)` — longitude first, then latitude.
/// Returns `None` if the string does not match the expected format.
fn parse_wkt_point(wkt: &str) -> Option<(f64, f64)> {
    let re = Regex::new(r"POINT\s*\(\s*(-?[\d.]+)\s+(-?[\d.]+)\s*\)").ok()?;
    let caps = re.captures(wkt)?;
    let lon: f64 = caps.get(1)?.as_str().parse().ok()?;
    let lat: f64 = caps.get(2)?.as_str().parse().ok()?;
    Some((lat, lon))
}

/// Map a Copernicus EMS category string to an internal EventType.
fn category_to_event_type(category: &str) -> EventType {
    match category.to_lowercase().as_str() {
        "flood" | "storm" | "humanitarian" | "other" => EventType::GeoEvent,
        "wildfire" | "fire" => EventType::ThermalAnomaly,
        "earthquake" => EventType::SeismicEvent,
        "volcano" => EventType::GeoEvent,
        "landslide" | "avalanche" => EventType::GeoEvent,
        "industrial accident" | "industrial_accident" => EventType::GeoEvent,
        "tsunami" => EventType::SeismicEvent,
        _ => EventType::GeoEvent,
    }
}

/// Determine severity based on activation state and scale.
fn determine_severity(closed: bool, n_aois: i64, n_products: i64, category: &str) -> Severity {
    if closed {
        return Severity::Medium;
    }
    // Active emergencies with significant mapping effort are high severity
    let cat_lower = category.to_lowercase();
    let is_severe_category =
        cat_lower == "earthquake" || cat_lower == "volcano" || cat_lower == "tsunami";
    if is_severe_category || n_aois >= 5 || n_products >= 10 {
        Severity::High
    } else if n_aois >= 2 || n_products >= 3 {
        Severity::Medium
    } else {
        Severity::Low
    }
}

/// Try to parse a datetime string from the Copernicus API.
/// The API uses various formats; we try the most common ones.
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 / RFC 3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try common format without timezone (assume UTC)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc());
    }
    // Date only
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc());
    }
    None
}

pub struct CopernicusSource {
    /// Activation codes we have already emitted to avoid duplicates.
    seen: Mutex<HashSet<String>>,
}

impl CopernicusSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for CopernicusSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for CopernicusSource {
    fn id(&self) -> &str {
        "copernicus"
    }

    fn name(&self) -> &str {
        "Copernicus EMS"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(1800) // 30 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        debug!("Polling Copernicus EMS activations");

        let resp = ctx.http.get(API_URL).send().await?;
        let resp = crate::rate_limit::check_rate_limit(resp, "copernicus")?;

        let wrapper: ActivationsResponse = resp.json().await?;
        let activations = wrapper.results;

        let cutoff = Utc::now() - chrono::Duration::days(LOOKBACK_DAYS);
        let mut events: Vec<InsertableEvent> = Vec::new();

        for activation in activations {
            let code = match &activation.code {
                Some(c) if !c.is_empty() => c.clone(),
                _ => continue,
            };

            // Filter: only activations updated within the lookback window
            let last_update_dt = activation
                .last_update
                .as_deref()
                .and_then(parse_datetime);
            if let Some(dt) = last_update_dt {
                if dt < cutoff {
                    continue;
                }
            }

            // Deduplication
            let source_id = format!("copernicus:{}", code);
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&source_id) {
                    continue;
                }
                seen.insert(source_id.clone());
            }

            let category = activation.category.as_deref().unwrap_or("other");
            let name = activation
                .name
                .as_deref()
                .unwrap_or("Copernicus EMS Activation");
            let countries = activation.countries.join(", ");
            let closed = activation.closed.unwrap_or(false);
            let n_aois = activation.n_aois.unwrap_or(0);
            let n_products = activation.n_products.unwrap_or(0);

            // Parse centroid coordinates from WKT
            let (lat, lon) = activation
                .centroid
                .as_deref()
                .and_then(parse_wkt_point)
                .unwrap_or((0.0, 0.0));

            // Skip activations without valid coordinates
            if lat == 0.0 && lon == 0.0 {
                debug!(code = %code, "Skipping activation with no centroid");
                continue;
            }

            let event_type = category_to_event_type(category);
            let severity = determine_severity(closed, n_aois, n_products, category);

            // Determine event time: prefer eventTime, fall back to activationTime, then lastUpdate
            let event_time = activation
                .event_time
                .as_deref()
                .and_then(parse_datetime)
                .or(activation.activation_time.as_deref().and_then(parse_datetime))
                .or(last_update_dt)
                .unwrap_or_else(Utc::now);

            // Region from coordinates
            let region = crate::common::region_from_coords(lat, lon)
                .map(|s| s.to_string());

            // Build tags
            let mut tags = vec!["copernicus".to_string()];
            tags.push(category.to_lowercase());
            for country in &activation.countries {
                let trimmed = country.trim();
                if !trimmed.is_empty() {
                    tags.push(trimmed.to_lowercase());
                }
            }
            if closed {
                tags.push("closed".to_string());
            }

            let payload = json!({
                "code": code,
                "countries": countries,
                "category": category,
                "gdacs_id": activation.gdacs_id,
                "activation_time": activation.activation_time,
                "last_update": activation.last_update,
                "n_aois": n_aois,
                "n_products": n_products,
                "closed": closed,
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Copernicus,
                source_id: Some(source_id),
                longitude: Some(lon),
                latitude: Some(lat),
                region_code: region,
                entity_id: Some(format!("copernicus:{}", code)),
                entity_name: Some(name.to_string()),
                event_type,
                severity,
                confidence: None,
                tags,
                title: Some(name.to_string()),
                description: Some(format!(
                    "{} activation {} — {} (AOIs: {}, Products: {})",
                    if closed { "Closed" } else { "Active" },
                    code,
                    category,
                    n_aois,
                    n_products,
                )),
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Prune seen set to avoid unbounded memory growth
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 5_000 {
                debug!(old_size = seen.len(), "Pruning seen Copernicus activation codes");
                seen.clear();
            }
        }

        if !events.is_empty() {
            debug!(count = events.len(), "Copernicus EMS activations");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_parse_wkt_point() {
        // Standard WKT POINT
        let (lat, lon) = parse_wkt_point("POINT (23.456 45.678)").unwrap();
        assert!((lon - 23.456).abs() < 1e-6);
        assert!((lat - 45.678).abs() < 1e-6);

        // Negative coordinates
        let (lat, lon) = parse_wkt_point("POINT (-73.935 40.730)").unwrap();
        assert!((lon - -73.935).abs() < 1e-6);
        assert!((lat - 40.730).abs() < 1e-6);

        // Extra whitespace
        let (lat, lon) = parse_wkt_point("POINT (  10.5   20.3  )").unwrap();
        assert!((lon - 10.5).abs() < 1e-6);
        assert!((lat - 20.3).abs() < 1e-6);

        // No space after POINT
        let (lat, lon) = parse_wkt_point("POINT(1.0 2.0)").unwrap();
        assert!((lon - 1.0).abs() < 1e-6);
        assert!((lat - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_wkt_point_invalid() {
        assert!(parse_wkt_point("").is_none());
        assert!(parse_wkt_point("not a point").is_none());
        assert!(parse_wkt_point("POINT ()").is_none());
        assert!(parse_wkt_point("POINT (abc def)").is_none());
        assert!(parse_wkt_point("LINESTRING (1 2, 3 4)").is_none());
    }

    #[test]
    fn test_category_to_event_type() {
        assert_eq!(category_to_event_type("Flood"), EventType::GeoEvent);
        assert_eq!(category_to_event_type("flood"), EventType::GeoEvent);
        assert_eq!(category_to_event_type("Storm"), EventType::GeoEvent);
        assert_eq!(category_to_event_type("Wildfire"), EventType::ThermalAnomaly);
        assert_eq!(category_to_event_type("Fire"), EventType::ThermalAnomaly);
        assert_eq!(category_to_event_type("Earthquake"), EventType::SeismicEvent);
        assert_eq!(category_to_event_type("Tsunami"), EventType::SeismicEvent);
        assert_eq!(category_to_event_type("Volcano"), EventType::GeoEvent);
        assert_eq!(category_to_event_type("Humanitarian"), EventType::GeoEvent);
        assert_eq!(category_to_event_type("Unknown"), EventType::GeoEvent);
    }

    #[test]
    fn test_determine_severity() {
        // Closed activation
        assert_eq!(determine_severity(true, 10, 20, "Flood"), Severity::Medium);

        // Active, severe category
        assert_eq!(determine_severity(false, 1, 1, "Earthquake"), Severity::High);
        assert_eq!(determine_severity(false, 1, 1, "Volcano"), Severity::High);
        assert_eq!(determine_severity(false, 1, 1, "Tsunami"), Severity::High);

        // Active, many AOIs
        assert_eq!(determine_severity(false, 5, 1, "Flood"), Severity::High);

        // Active, many products
        assert_eq!(determine_severity(false, 1, 10, "Storm"), Severity::High);

        // Active, moderate scale
        assert_eq!(determine_severity(false, 2, 1, "Flood"), Severity::Medium);
        assert_eq!(determine_severity(false, 1, 3, "Flood"), Severity::Medium);

        // Active, small scale
        assert_eq!(determine_severity(false, 1, 1, "Flood"), Severity::Low);
    }

    #[test]
    fn test_parse_datetime() {
        // ISO 8601 with timezone
        let dt = parse_datetime("2026-01-15T10:30:00+00:00").unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 15);

        // Without timezone
        let dt = parse_datetime("2026-03-01T12:00:00").unwrap();
        assert_eq!(dt.year(), 2026);

        // Space separator
        let dt = parse_datetime("2026-03-01 12:00:00").unwrap();
        assert_eq!(dt.year(), 2026);

        // Date only
        let dt = parse_datetime("2026-03-01").unwrap();
        assert_eq!(dt.year(), 2026);

        // Invalid
        assert!(parse_datetime("not-a-date").is_none());
        assert!(parse_datetime("").is_none());
    }

    #[test]
    fn test_deduplication() {
        let source = CopernicusSource::new();
        {
            let mut seen = source.seen.lock().unwrap();
            seen.insert("copernicus:EMSR742".to_string());
        }
        let seen = source.seen.lock().unwrap();
        assert!(seen.contains("copernicus:EMSR742"));
        assert!(!seen.contains("copernicus:EMSR999"));
    }

    #[test]
    fn test_activation_deserialization() {
        let json_data = r#"{
            "count": 1,
            "next": null,
            "previous": null,
            "results": [
                {
                    "code": "EMSR742",
                    "countries": ["Bangladesh"],
                    "eventTime": "2026-01-15T10:30:00+00:00",
                    "name": "Flood in Bangladesh",
                    "centroid": "POINT (90.356 23.685)",
                    "activationTime": "2026-01-15T12:00:00+00:00",
                    "category": "Flood",
                    "lastUpdate": "2026-01-20T08:00:00+00:00",
                    "closed": false,
                    "gdacsId": "FL-2026-000001-BGD",
                    "n_aois": 3,
                    "n_products": 7
                }
            ]
        }"#;

        let resp: ActivationsResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(resp.results.len(), 1);

        let a = &resp.results[0];
        assert_eq!(a.code.as_deref(), Some("EMSR742"));
        assert_eq!(a.countries, vec!["Bangladesh"]);
        assert_eq!(a.name.as_deref(), Some("Flood in Bangladesh"));
        assert_eq!(a.category.as_deref(), Some("Flood"));
        assert_eq!(a.closed, Some(false));
        assert_eq!(a.n_aois, Some(3));
        assert_eq!(a.n_products, Some(7));

        // Verify centroid parsing
        let (lat, lon) = parse_wkt_point(a.centroid.as_deref().unwrap()).unwrap();
        assert!((lon - 90.356).abs() < 1e-6);
        assert!((lat - 23.685).abs() < 1e-6);
    }

    #[test]
    fn test_activation_with_missing_fields() {
        let json_data = r#"{
            "count": 1,
            "results": [
                {
                    "code": "EMSR001",
                    "eventTime": null,
                    "name": null,
                    "centroid": null,
                    "activationTime": null,
                    "category": null,
                    "lastUpdate": null,
                    "closed": null,
                    "gdacsId": null,
                    "n_aois": null,
                    "n_products": null
                }
            ]
        }"#;

        let resp: ActivationsResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].code.as_deref(), Some("EMSR001"));
        assert!(resp.results[0].countries.is_empty());
        assert!(resp.results[0].centroid.is_none());
    }
}
