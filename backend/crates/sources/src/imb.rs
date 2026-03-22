use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, Utc};
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::common::region_from_coords;
use crate::rate_limit::check_rate_limit;
use crate::{DataSource, InsertableEvent, SourceContext};

/// ICC-CCS IMB Piracy Reporting Centre — WP Go Maps REST API.
///
/// The IMB Live Piracy Map at <https://icc-ccs.org/map/> is backed by a
/// WordPress plugin (WP Go Maps) that exposes a REST endpoint at
/// `/wp-json/wpgmza/v1/markers`. Each marker has lat/lng coordinates, an
/// incident number title, and a `custom_field_data` array containing the
/// incident number and a sitrep (situation report) with date, time, position,
/// and narrative text.
///
/// Map IDs correspond to years (3=2024, 4=2023, 5=2022, 9=2021, 10=2020).
/// We poll the most recent map IDs for incident data.
const API_BASE: &str = "https://icc-ccs.org/wp-json/wpgmza/v1/markers";

/// Map IDs for recent years on the ICC-CCS piracy map.
/// Format: (map_id, year) — ordered most recent first.
const MAP_IDS: [(u32, u16); 5] = [
    (3, 2024),
    (4, 2023),
    (5, 2022),
    (9, 2021),
    (10, 2020),
];

/// How far back (in days) to consider incidents. Older incidents are skipped
/// to avoid inserting into compressed TimescaleDB chunks.
const LOOKBACK_DAYS: i64 = 30;

/// WP Go Maps marker record.
#[derive(Debug, Deserialize)]
struct Marker {
    id: Option<String>,
    lat: Option<String>,
    lng: Option<String>,
    title: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    custom_field_data: Vec<CustomField>,
}

/// Custom field on a WP Go Maps marker.
#[derive(Debug, Deserialize)]
struct CustomField {
    #[allow(dead_code)]
    id: Option<u64>,
    name: Option<String>,
    value: Option<String>,
}

/// Incident category based on ICC-CCS classification.
/// Categories 1-5 map to different incident severity levels.
fn category_severity(cats: &[String]) -> Severity {
    for c in cats {
        match c.as_str() {
            // Category 4/5: Hijacking / Armed piracy — most severe
            "4" | "5" => return Severity::High,
            // Category 3: Piracy with weapons
            "3" => return Severity::Medium,
            // Category 1/2: Attempted boarding / Robbery
            "1" | "2" => return Severity::Low,
            _ => {}
        }
    }
    Severity::Low
}

/// Parse a date from the sitrep text.
///
/// Sitreps typically start with a date in DD.MM.YYYY format, e.g.:
///   "03.01.2019: 0445 UTC: Posn: ..."
///   "21.11.2012: 2325 LT: Posn: ..."
fn parse_sitrep_date(sitrep: &str) -> Option<NaiveDate> {
    let re = Regex::new(r"^(\d{2})\.(\d{2})\.(\d{4})").ok()?;
    let caps = re.captures(sitrep.trim())?;
    let day: u32 = caps.get(1)?.as_str().parse().ok()?;
    let month: u32 = caps.get(2)?.as_str().parse().ok()?;
    let year: i32 = caps.get(3)?.as_str().parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

/// Parse time from sitrep text (e.g. "0445 UTC" or "2325 LT").
fn parse_sitrep_time(sitrep: &str) -> Option<(u32, u32)> {
    let re = Regex::new(r"(\d{4})\s*(?:UTC|LT|Local)").ok()?;
    let caps = re.captures(sitrep)?;
    let time_str = caps.get(1)?.as_str();
    if time_str.len() == 4 {
        let hours: u32 = time_str[..2].parse().ok()?;
        let minutes: u32 = time_str[2..4].parse().ok()?;
        if hours < 24 && minutes < 60 {
            return Some((hours, minutes));
        }
    }
    None
}

/// Extract coordinates from sitrep position text.
///
/// Looks for patterns like:
///   "Posn: 10:16N – 064:42W"
///   "Posn: 12:32.0N - 043:27.5E"
///   "Posn: 03:44S - 114:27E"
fn parse_sitrep_position(sitrep: &str) -> Option<(f64, f64)> {
    // Pattern: DD:MM.M'N/S - DDD:MM.M'E/W  (with optional decimal minutes)
    let re = Regex::new(
        r"(?i)(?:Posn|Position)\s*:\s*(\d{1,3}):(\d{1,2}(?:\.\d+)?)\s*([NS])\s*[-–]\s*(\d{1,3}):(\d{1,2}(?:\.\d+)?)\s*([EW])"
    ).ok()?;

    let caps = re.captures(sitrep)?;
    let lat_deg: f64 = caps.get(1)?.as_str().parse().ok()?;
    let lat_min: f64 = caps.get(2)?.as_str().parse().ok()?;
    let mut lat = lat_deg + lat_min / 60.0;
    let lon_deg: f64 = caps.get(4)?.as_str().parse().ok()?;
    let lon_min: f64 = caps.get(5)?.as_str().parse().ok()?;
    let mut lon = lon_deg + lon_min / 60.0;

    if caps.get(3)?.as_str().eq_ignore_ascii_case("S") {
        lat = -lat;
    }
    if caps.get(6)?.as_str().eq_ignore_ascii_case("W") {
        lon = -lon;
    }

    if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
        Some((lat, lon))
    } else {
        None
    }
}

/// Extract a location description from sitrep text.
///
/// Looks for text after the position coordinates, typically a place name
/// like "Puerto La Cruz Anchorage, Venezuela" or "Bab El Mandeb Strait, Red Sea".
fn extract_location(sitrep: &str) -> Option<String> {
    // Pattern: after coordinates, grab the location text up to the next period or sentence
    let re = Regex::new(
        r"(?i)(?:Posn|Position)\s*:\s*\d{1,3}:\d{1,2}(?:\.\d+)?\s*[NS]\s*[-–]\s*\d{1,3}:\d{1,2}(?:\.\d+)?\s*[EW]\s*,\s*([^.]+)"
    ).ok()?;

    let caps = re.captures(sitrep)?;
    let location = caps.get(1)?.as_str().trim().to_string();
    if location.len() >= 3 {
        Some(location)
    } else {
        None
    }
}

/// Determine severity from sitrep content (keywords override category-based severity).
fn severity_from_sitrep(sitrep: &str, category_severity: Severity) -> Severity {
    let lower = sitrep.to_lowercase();

    if lower.contains("hijack") || lower.contains("kidnap") {
        return Severity::Critical;
    }
    if lower.contains("missile") || lower.contains("torpedo") || lower.contains("explosion") {
        return Severity::Critical;
    }
    if lower.contains("fired") || lower.contains("rpg") || lower.contains("shot")
        || lower.contains("gun") || lower.contains("ak-47") || lower.contains("ak 47")
        || lower.contains("automatic weapon")
    {
        return Severity::High.max(category_severity);
    }
    if lower.contains("knife") || lower.contains("knives") || lower.contains("machete") {
        return Severity::Medium.max(category_severity);
    }

    category_severity
}

/// Build tags from sitrep text.
fn build_tags(sitrep: &str) -> Vec<String> {
    let lower = sitrep.to_lowercase();
    let mut tags = vec![
        "maritime".to_string(),
        "maritime-security".to_string(),
        "piracy".to_string(),
        "source:IMB".to_string(),
    ];

    if lower.contains("hijack") {
        tags.push("hijack".to_string());
    }
    if lower.contains("kidnap") {
        tags.push("kidnapping".to_string());
    }
    if lower.contains("fired") || lower.contains("gun") || lower.contains("rpg")
        || lower.contains("ak-47") || lower.contains("ak 47")
    {
        tags.push("armed".to_string());
    }
    if lower.contains("robber") || lower.contains("robbery") || lower.contains("stole")
        || lower.contains("stolen")
    {
        tags.push("robbery".to_string());
    }
    if lower.contains("boarding") || lower.contains("boarded") {
        tags.push("boarding".to_string());
    }
    if lower.contains("anchored") || lower.contains("anchorage") {
        tags.push("anchorage".to_string());
    }
    if lower.contains("skiff") || lower.contains("small boat") {
        tags.push("skiff".to_string());
    }
    if lower.contains("red sea") {
        tags.push("red-sea".to_string());
    }
    if lower.contains("gulf of aden") {
        tags.push("gulf-of-aden".to_string());
    }
    if lower.contains("strait") {
        tags.push("strait".to_string());
    }
    if lower.contains("somalia") {
        tags.push("somalia".to_string());
    }
    if lower.contains("nigeria") {
        tags.push("nigeria".to_string());
    }
    if lower.contains("indonesia") {
        tags.push("indonesia".to_string());
    }
    if lower.contains("singapore") {
        tags.push("singapore".to_string());
    }

    tags
}

/// ICC-CCS IMB Piracy Reporting Centre data source.
///
/// Polls the ICC-CCS WP Go Maps REST API for piracy and armed robbery
/// incident markers. Each marker contains structured sitrep data with
/// date, time, coordinates, and a narrative description.
///
/// Note: As of March 2026, the ICC-CCS map data covers 2012-2024.
/// The API is queried for the most recent map IDs (2020-2024).
/// When ICC-CCS adds newer year maps, this source will automatically
/// pick them up without code changes (just add the map_id to MAP_IDS).
pub struct ImbPiracySource {
    /// Source IDs already emitted (dedup).
    seen: Mutex<HashSet<String>>,
    /// Which map IDs we have already fully ingested (avoid re-fetching).
    completed_maps: Mutex<HashSet<u32>>,
}

impl ImbPiracySource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
            completed_maps: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for ImbPiracySource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for ImbPiracySource {
    fn id(&self) -> &str {
        "imb-piracy"
    }

    fn name(&self) -> &str {
        "IMB Piracy Reporting Centre"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(3600) // 1 hour — data updates infrequently
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        debug!("Polling ICC-CCS IMB piracy map");

        let cutoff = Utc::now() - chrono::Duration::days(LOOKBACK_DAYS);
        let mut all_events = Vec::new();

        // Only poll the most recent map (current year first, then fall back)
        // After first successful full ingest, skip completed maps
        for &(map_id, year) in &MAP_IDS {
            {
                let completed = self.completed_maps.lock().unwrap_or_else(|e| e.into_inner());
                if completed.contains(&map_id) {
                    continue;
                }
            }

            let url = format!("{}?map_id={}", API_BASE, map_id);
            let resp = match ctx.http
                .get(&url)
                .timeout(Duration::from_secs(30))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(map_id, year, error = %e, "Failed to fetch IMB map markers");
                    continue;
                }
            };

            let resp = match check_rate_limit(resp, "imb_piracy") {
                Ok(r) => r,
                Err(e) => {
                    warn!(map_id, year, error = %e, "IMB piracy API error");
                    continue;
                }
            };

            let markers: Vec<Marker> = match resp.json().await {
                Ok(m) => m,
                Err(e) => {
                    warn!(map_id, year, error = %e, "Failed to parse IMB markers JSON");
                    continue;
                }
            };

            if markers.is_empty() {
                debug!(map_id, year, "No markers found for IMB map");
                continue;
            }

            let mut map_event_count = 0usize;

            for marker in &markers {
                // Extract sitrep from custom fields
                let sitrep = marker.custom_field_data.iter()
                    .find(|f| f.name.as_deref().map(|n| n.to_lowercase().contains("sitrep")).unwrap_or(false))
                    .and_then(|f| f.value.as_deref())
                    .unwrap_or("");

                // Extract incident number from custom fields or title
                let incident_number = marker.custom_field_data.iter()
                    .find(|f| f.name.as_deref().map(|n| n.to_lowercase().contains("incident")).unwrap_or(false))
                    .and_then(|f| f.value.as_deref())
                    .or(marker.title.as_deref())
                    .unwrap_or("");

                if incident_number.is_empty() && sitrep.is_empty() {
                    continue;
                }

                // Build source_id for dedup
                let source_id = if !incident_number.is_empty() {
                    format!("imb:{}:{}", year, incident_number)
                } else {
                    format!("imb:{}:{}", year, marker.id.as_deref().unwrap_or("unknown"))
                };

                // Dedup check
                {
                    let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                    if seen.contains(&source_id) {
                        continue;
                    }
                    seen.insert(source_id.clone());
                }

                // Parse date from sitrep
                let incident_date = parse_sitrep_date(sitrep);

                // Apply recency filter
                if let Some(date) = incident_date {
                    if let Some(dt) = date.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc()) {
                        if dt < cutoff {
                            continue;
                        }
                    }
                } else {
                    // No parseable date — skip unless from the current year's map
                    let current_year = Utc::now().year() as u16;
                    if year < current_year.saturating_sub(1) {
                        continue;
                    }
                }

                // Parse time
                let (hours, minutes) = parse_sitrep_time(sitrep).unwrap_or((0, 0));

                // Build event_time
                let event_time = incident_date
                    .and_then(|d| d.and_hms_opt(hours, minutes, 0).map(|dt| dt.and_utc()))
                    .unwrap_or_else(Utc::now);

                // Coordinates: prefer sitrep-extracted, fall back to marker lat/lng
                let sitrep_pos = parse_sitrep_position(sitrep);
                let (lat, lon) = sitrep_pos.unwrap_or_else(|| {
                    let mlat = marker.lat.as_deref()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let mlon = marker.lng.as_deref()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    (mlat, mlon)
                });

                // Skip markers without valid coordinates
                if lat == 0.0 && lon == 0.0 {
                    continue;
                }

                let location = extract_location(sitrep);
                let cat_sev = category_severity(&marker.categories);
                let severity = severity_from_sitrep(sitrep, cat_sev);
                let tags = build_tags(sitrep);
                let region_code = region_from_coords(lat, lon).map(String::from);

                // Title: incident number + location
                let title = if let Some(ref loc) = location {
                    if !incident_number.is_empty() {
                        format!("IMB {}: Piracy incident — {}", incident_number, loc)
                    } else {
                        format!("IMB Piracy incident — {}", loc)
                    }
                } else if !incident_number.is_empty() {
                    format!("IMB {}: Maritime piracy/armed robbery", incident_number)
                } else {
                    "IMB: Maritime piracy/armed robbery incident".to_string()
                };

                // Description: first 500 chars of sitrep
                let description = if sitrep.len() > 500 {
                    format!("{}...", &sitrep[..497])
                } else if !sitrep.is_empty() {
                    sitrep.to_string()
                } else {
                    title.clone()
                };

                // Event type: hijackings/kidnappings are conflict events
                let event_type = {
                    let lower = sitrep.to_lowercase();
                    if lower.contains("hijack") || lower.contains("kidnap")
                        || lower.contains("fired") || lower.contains("missile")
                    {
                        EventType::ConflictEvent
                    } else {
                        EventType::MaritimeSecurity
                    }
                };

                let payload = json!({
                    "incident_number": incident_number,
                    "year": year,
                    "sitrep": sitrep,
                    "location": location,
                    "categories": marker.categories,
                    "marker_id": marker.id,
                    "source": "ICC-CCS IMB Piracy Reporting Centre",
                });

                all_events.push(InsertableEvent {
                    event_time,
                    source_type: SourceType::ImbPiracy,
                    source_id: Some(source_id),
                    longitude: Some(lon),
                    latitude: Some(lat),
                    region_code,
                    entity_id: if !incident_number.is_empty() {
                        Some(format!("IMB-{}", incident_number))
                    } else {
                        None
                    },
                    entity_name: if !incident_number.is_empty() {
                        Some(format!("IMB Incident {}", incident_number))
                    } else {
                        None
                    },
                    event_type,
                    severity,
                    confidence: Some(0.85), // IMB data is well-verified but may lack precision
                    tags,
                    title: Some(title),
                    description: Some(description),
                    payload,
                    heading: None,
                    speed: None,
                    altitude: None,
                });

                map_event_count += 1;
            }

            // Mark map as completed after full ingest (all markers processed)
            {
                let mut completed = self.completed_maps.lock().unwrap_or_else(|e| e.into_inner());
                completed.insert(map_id);
            }

            if map_event_count > 0 {
                debug!(map_id, year, count = map_event_count, "IMB piracy incidents from map");
            }

            // Only fetch the most recent map that has data — avoid
            // hammering the API for all 5 years on every poll cycle
            if !all_events.is_empty() {
                break;
            }
        }

        // Prune seen set to avoid unbounded memory growth
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 10_000 {
                debug!(old_size = seen.len(), "Pruning seen IMB incident IDs");
                seen.clear();
                // Also clear completed maps so we re-fetch
                let mut completed = self.completed_maps.lock().unwrap_or_else(|e| e.into_inner());
                completed.clear();
            }
        }

        if !all_events.is_empty() {
            info!(count = all_events.len(), "IMB piracy incidents ingested");
        }

        Ok(all_events)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn parse_sitrep_date_standard() {
        let date = parse_sitrep_date("03.01.2019: 0445 UTC: Posn: 10:16N").unwrap();
        assert_eq!(date.year(), 2019);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 3);
    }

    #[test]
    fn parse_sitrep_date_november() {
        let date = parse_sitrep_date("21.11.2012: 2325 LT: Posn: 06:03.36N").unwrap();
        assert_eq!(date.year(), 2012);
        assert_eq!(date.month(), 11);
        assert_eq!(date.day(), 21);
    }

    #[test]
    fn parse_sitrep_date_invalid() {
        assert!(parse_sitrep_date("No date here").is_none());
        assert!(parse_sitrep_date("").is_none());
    }

    #[test]
    fn parse_sitrep_time_utc() {
        let (h, m) = parse_sitrep_time("03.01.2019: 0445 UTC: Posn:").unwrap();
        assert_eq!(h, 4);
        assert_eq!(m, 45);
    }

    #[test]
    fn parse_sitrep_time_lt() {
        let (h, m) = parse_sitrep_time("21.11.2012: 2325 LT: Posn:").unwrap();
        assert_eq!(h, 23);
        assert_eq!(m, 25);
    }

    #[test]
    fn parse_sitrep_time_none() {
        assert!(parse_sitrep_time("No time here").is_none());
    }

    #[test]
    fn parse_position_standard() {
        let (lat, lon) = parse_sitrep_position("Posn: 10:16N – 064:42W").unwrap();
        assert!((lat - 10.2667).abs() < 0.01);
        assert!((lon - -64.70).abs() < 0.01);
    }

    #[test]
    fn parse_position_decimal_minutes() {
        let (lat, lon) = parse_sitrep_position("Posn: 12:32.0N - 043:27.5E").unwrap();
        assert!((lat - 12.5333).abs() < 0.01);
        assert!((lon - 43.4583).abs() < 0.01);
    }

    #[test]
    fn parse_position_south_east() {
        let (lat, lon) = parse_sitrep_position("Posn: 03:44S - 114:27E").unwrap();
        assert!(lat < 0.0);
        assert!(lon > 0.0);
        assert!((lat - -3.7333).abs() < 0.01);
        assert!((lon - 114.45).abs() < 0.01);
    }

    #[test]
    fn parse_position_none() {
        assert!(parse_sitrep_position("No position data").is_none());
        assert!(parse_sitrep_position("").is_none());
    }

    #[test]
    fn extract_location_standard() {
        let loc = extract_location(
            "03.01.2019: 0445 UTC: Posn: 10:16N – 064:42W, Puerto La Cruz Anchorage, Venezuela"
        ).unwrap();
        assert!(loc.contains("Puerto La Cruz"));
        assert!(loc.contains("Venezuela"));
    }

    #[test]
    fn extract_location_with_period() {
        let loc = extract_location(
            "Posn: 06:03.36N – 001:16.46E, Lome anchorage, Togo. Six robbers"
        ).unwrap();
        assert!(loc.contains("Lome"));
        assert!(loc.contains("Togo"));
    }

    #[test]
    fn extract_location_none() {
        assert!(extract_location("No position data here").is_none());
    }

    #[test]
    fn category_severity_hijack() {
        assert_eq!(category_severity(&["4".to_string()]), Severity::High);
        assert_eq!(category_severity(&["5".to_string()]), Severity::High);
    }

    #[test]
    fn category_severity_armed() {
        assert_eq!(category_severity(&["3".to_string()]), Severity::Medium);
    }

    #[test]
    fn category_severity_robbery() {
        assert_eq!(category_severity(&["1".to_string()]), Severity::Low);
        assert_eq!(category_severity(&["2".to_string()]), Severity::Low);
    }

    #[test]
    fn severity_escalation_hijack() {
        assert_eq!(
            severity_from_sitrep("Pirates hijacked the vessel", Severity::Low),
            Severity::Critical
        );
    }

    #[test]
    fn severity_escalation_fired() {
        assert_eq!(
            severity_from_sitrep("Pirates fired upon the vessel with AK-47", Severity::Low),
            Severity::High
        );
    }

    #[test]
    fn severity_no_escalation() {
        assert_eq!(
            severity_from_sitrep("Robbers attempted to board", Severity::Low),
            Severity::Low
        );
    }

    #[test]
    fn tags_armed_robbery() {
        let tags = build_tags("Robbers boarded the anchored vessel with knives");
        assert!(tags.contains(&"maritime".to_string()));
        assert!(tags.contains(&"piracy".to_string()));
        assert!(tags.contains(&"robbery".to_string()));
        assert!(tags.contains(&"boarding".to_string()));
        assert!(tags.contains(&"anchorage".to_string()));
    }

    #[test]
    fn tags_somalia() {
        let tags = build_tags("Skiff with pirates approached off Somalia coast");
        assert!(tags.contains(&"somalia".to_string()));
        assert!(tags.contains(&"skiff".to_string()));
    }

    #[test]
    fn tags_singapore_strait() {
        let tags = build_tags("Incident in the Singapore Strait area");
        assert!(tags.contains(&"singapore".to_string()));
        assert!(tags.contains(&"strait".to_string()));
    }

    #[test]
    fn source_metadata() {
        let source = ImbPiracySource::new();
        assert_eq!(source.id(), "imb-piracy");
        assert_eq!(source.name(), "IMB Piracy Reporting Centre");
        assert_eq!(source.default_interval(), Duration::from_secs(3600));
        assert!(!source.is_streaming());
    }

    #[test]
    fn deduplication() {
        let source = ImbPiracySource::new();
        {
            let mut seen = source.seen.lock().unwrap();
            seen.insert("imb:2024:001-24".to_string());
        }
        let seen = source.seen.lock().unwrap();
        assert!(seen.contains("imb:2024:001-24"));
        assert!(!seen.contains("imb:2024:002-24"));
    }

    #[test]
    fn marker_deserialization() {
        let json_data = r#"[{
            "id": "8267",
            "lat": "12.533333333333333",
            "lng": "43.450000000000045",
            "title": "002-12",
            "categories": ["5"],
            "custom_field_data": [
                {"id": 9, "name": "Incident Number", "value": "002-12"},
                {"id": 66, "name": "Sitrep:", "value": "04.01.2012: 0730 UTC: Posn: 12:32.0N - 043:27.5E, Bab El Mandeb Strait, Red Sea. Non-aggressive persons in three skiffs approached a bulk carrier."}
            ]
        }]"#;

        let markers: Vec<Marker> = serde_json::from_str(json_data).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].title.as_deref(), Some("002-12"));
        assert_eq!(markers[0].categories, vec!["5"]);
        assert_eq!(markers[0].custom_field_data.len(), 2);

        let sitrep = markers[0].custom_field_data[1].value.as_deref().unwrap();
        let date = parse_sitrep_date(sitrep).unwrap();
        assert_eq!(date.day(), 4);
        assert_eq!(date.month(), 1);
        assert_eq!(date.year(), 2012);

        let (lat, lon) = parse_sitrep_position(sitrep).unwrap();
        assert!((lat - 12.5333).abs() < 0.01);
        assert!((lon - 43.4583).abs() < 0.01);

        let loc = extract_location(sitrep).unwrap();
        assert!(loc.contains("Bab El Mandeb"));
    }
}
