use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tracing::{debug, warn};

use chrono::{NaiveDate, NaiveTime, Utc};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

/// Regional bounding boxes for global coverage: (name, west, south, east, north).
/// NASA FIRMS supports a `world` area parameter, but it returns enormous datasets.
/// We use ~8 continental regions and rotate through them to stay within rate limits.
const BOUNDING_BOXES: &[(&str, f64, f64, f64, f64)] = &[
    // Original conflict zones
    ("middle_east", 25.0, 12.0, 63.0, 42.0),
    ("eastern_europe", 22.0, 44.0, 45.0, 56.0),
    // Africa
    ("north_africa", -18.0, 15.0, 52.0, 37.0),
    ("sub_saharan_africa", -18.0, -35.0, 52.0, 15.0),
    // Asia
    ("south_asia", 60.0, 5.0, 100.0, 38.0),
    ("east_asia", 100.0, 18.0, 150.0, 55.0),
    ("southeast_asia", 90.0, -12.0, 140.0, 18.0),
    // Americas
    ("south_america", -82.0, -56.0, -34.0, 15.0),
    ("north_america", -130.0, 15.0, -60.0, 55.0),
    // Oceania
    ("oceania", 110.0, -50.0, 180.0, -5.0),
];

/// A single FIRMS CSV record from the VIIRS SNPP instrument.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FirmsRecord {
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    bright_ti4: Option<f64>,
    #[serde(default)]
    scan: Option<f64>,
    #[serde(default)]
    track: Option<f64>,
    #[serde(default)]
    acq_date: Option<String>,
    #[serde(default)]
    acq_time: Option<String>,
    #[serde(default)]
    satellite: Option<String>,
    #[serde(default)]
    instrument: Option<String>,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    bright_ti5: Option<f64>,
    #[serde(default)]
    frp: Option<f64>,
    #[serde(default)]
    daynight: Option<String>,
    #[serde(default, rename = "type")]
    detection_type: Option<i32>,
}

/// Dedup key: (lat*10000 rounded, lon*10000 rounded, acq_date, acq_time)
/// This deduplicates the same fire pixel across consecutive polls.
type FirmsDedup = (i64, i64, String, String);

pub struct FirmsSource {
    bbox_index: AtomicUsize,
    /// Session-level dedup: skip fire pixels already seen this session
    seen: Mutex<HashSet<FirmsDedup>>,
}

impl FirmsSource {
    pub fn new() -> Self {
        Self {
            bbox_index: AtomicUsize::new(0),
            seen: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for FirmsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for FirmsSource {
    fn id(&self) -> &str {
        "firms"
    }

    fn name(&self) -> &str {
        "NASA FIRMS"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(30 * 60) // 30 minutes
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let map_key = match std::env::var("FIRMS_MAP_KEY") {
            Ok(key) if !key.is_empty() => key,
            _ => {
                warn!("FIRMS_MAP_KEY not set; skipping FIRMS poll");
                return Ok(Vec::new());
            }
        };

        // Rotate through bounding boxes, one per poll cycle
        let idx = self.bbox_index.fetch_add(1, Ordering::Relaxed) % BOUNDING_BOXES.len();
        let (region, west, south, east, north) = BOUNDING_BOXES[idx];

        debug!(region, "Polling NASA FIRMS for bounding box");

        // FIRMS area query: /api/area/csv/{MAP_KEY}/{source}/{area}/{day_range}
        // area format: west,south,east,north
        let url = format!(
            "https://firms.modaps.eosdis.nasa.gov/api/area/csv/{}/VIIRS_SNPP_NRT/{},{},{},{}/1",
            map_key, west, south, east, north
        );

        let resp = ctx.http.get(&url).send().await?;
        let resp = crate::rate_limit::check_rate_limit(resp, "firms")?;

        let csv_text = resp.text().await?;

        // FIRMS returns plain-text error messages on auth failure (e.g. "Invalid MAP_KEY.")
        // instead of CSV data. Detect and surface as an error rather than silently parsing zero rows.
        if csv_text.starts_with("Invalid")
            || csv_text.starts_with("<!DOCTYPE")
            || csv_text.starts_with("<html")
        {
            anyhow::bail!(
                "FIRMS API error: {}",
                csv_text.chars().take(200).collect::<String>()
            );
        }

        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(csv_text.as_bytes());

        let mut events: Vec<InsertableEvent> = Vec::new();

        for result in reader.deserialize::<FirmsRecord>() {
            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    debug!(error = %e, "Skipping malformed FIRMS CSV row");
                    continue;
                }
            };

            // Filter out low-confidence detections.
            // VIIRS uses single-letter codes: h=high, n=nominal, l=low.
            // MODIS uses full words: high, nominal, low.
            let confidence_raw = record.confidence.as_deref().unwrap_or("l");
            let confidence = match confidence_raw {
                "h" | "high" => "high",
                "n" | "nominal" => "nominal",
                "l" | "low" | _ => "low",
            };
            if confidence == "low" {
                continue;
            }

            // Dedup: skip fire pixels already seen this session
            let dedup_key: FirmsDedup = (
                (record.latitude * 10000.0).round() as i64,
                (record.longitude * 10000.0).round() as i64,
                record.acq_date.clone().unwrap_or_default(),
                record.acq_time.clone().unwrap_or_default(),
            );
            {
                let mut seen = self.seen.lock().unwrap();
                if !seen.insert(dedup_key) {
                    continue; // already seen this fire pixel
                }
                // Cap memory: if seen set grows too large, clear oldest half
                if seen.len() > 50_000 {
                    seen.clear();
                }
            }

            // Parse satellite acquisition time for accurate event_time.
            // acq_date format: "2026-03-04", acq_time format: "0926" (HHMM UTC)
            let acq_date_str = record.acq_date.as_deref().unwrap_or("");
            let acq_time_str = record.acq_time.as_deref().unwrap_or("");
            let event_time = parse_acquisition_time(acq_date_str, acq_time_str)
                .unwrap_or_else(Utc::now);

            // Stable source_id from detection metadata for DB-level dedup
            // (ON CONFLICT (source_type, source_id, event_time) DO NOTHING)
            let lat_rounded = (record.latitude * 10000.0).round() as i64;
            let lon_rounded = (record.longitude * 10000.0).round() as i64;
            let satellite_raw = record.satellite.as_deref().unwrap_or("unknown");
            // VIIRS satellite codes: N=NOAA-20, 1=NOAA-21, SN=Suomi NPP
            let satellite = match satellite_raw {
                "N" => "NOAA-20",
                "1" => "NOAA-21",
                "SN" | "Suomi NPP" => "Suomi NPP",
                other => other,
            };
            let source_id = format!(
                "firms:{}:{}:{}:{}:{}",
                satellite_raw, acq_date_str, acq_time_str, lat_rounded, lon_rounded
            );

            let frp = record.frp;
            let severity = if frp > Some(100.0) {
                Severity::High
            } else if frp > Some(50.0) {
                Severity::Medium
            } else {
                Severity::Low
            };

            let confidence_val: Option<f32> = match confidence {
                "high" => Some(0.9),
                "nominal" => Some(0.6),
                "low" => Some(0.3),
                _ => None,
            };

            let title = format!(
                "Thermal anomaly — {} FRP {:.1} MW",
                satellite, frp.unwrap_or(0.0)
            );

            let mut tags = vec!["fire".to_string()];
            tags.push(satellite.to_string());

            let daynight = match record.daynight.as_deref() {
                Some("D") => "Day",
                Some("N") => "Night",
                Some(other) => other,
                None => "Unknown",
            };

            let data = json!({
                "latitude": record.latitude,
                "longitude": record.longitude,
                "frp": frp,
                "confidence": confidence,
                "acq_date": record.acq_date,
                "acq_time": record.acq_time,
                "satellite": satellite,
                "daynight": daynight,
                "bright_ti4": record.bright_ti4,
                "bright_ti5": record.bright_ti5,
                "region": region,
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Firms,
                source_id: Some(source_id),
                longitude: Some(record.longitude),
                latitude: Some(record.latitude),
                region_code: Some(region.to_string()),
                entity_id: None,
                entity_name: None,
                event_type: EventType::ThermalAnomaly,
                severity,
                confidence: confidence_val,
                tags,
                title: Some(title),
                description: None,
                payload: data,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        if !events.is_empty() {
            debug!(region, count = events.len(), "FIRMS thermal anomalies detected");
        }

        Ok(events)
        })
    }
}

/// Parse FIRMS acquisition date + time into a UTC DateTime.
/// `acq_date` format: "2026-03-04", `acq_time` format: "0926" (HHMM UTC).
/// Returns None if parsing fails (caller should fall back to Utc::now()).
fn parse_acquisition_time(acq_date: &str, acq_time: &str) -> Option<chrono::DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(acq_date, "%Y-%m-%d").ok()?;
    // acq_time is HHMM, e.g. "0926" or "1430"
    let time = NaiveTime::parse_from_str(acq_time, "%H%M").ok()?;
    let naive_dt = date.and_time(time);
    Some(naive_dt.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_rotation() {
        let source = FirmsSource::new();
        let num_regions = BOUNDING_BOXES.len();
        assert!(num_regions >= 8, "Should have global coverage with at least 8 regions");
        for expected in 0..num_regions {
            let idx = source.bbox_index.fetch_add(1, Ordering::Relaxed) % num_regions;
            assert_eq!(idx, expected);
        }
        // Wraps around
        let idx = source.bbox_index.fetch_add(1, Ordering::Relaxed) % num_regions;
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_csv_parsing() {
        let csv_data = "\
latitude,longitude,bright_ti4,scan,track,acq_date,acq_time,satellite,instrument,confidence,version,bright_ti5,frp,daynight,type
33.456,44.123,320.5,0.39,0.36,2025-01-15,0430,Suomi NPP,VIIRS,high,2.0NRT,290.1,15.3,N,0
34.789,45.678,310.2,0.40,0.37,2025-01-15,0430,Suomi NPP,VIIRS,low,2.0NRT,285.0,2.1,N,0
35.012,46.234,330.8,0.41,0.38,2025-01-15,0430,Suomi NPP,VIIRS,nominal,2.0NRT,295.5,22.7,D,0
";
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(csv_data.as_bytes());

        let records: Vec<FirmsRecord> = reader
            .deserialize()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(records.len(), 3);

        // First record: high confidence, should be included
        assert_eq!(records[0].confidence.as_deref(), Some("high"));
        assert!((records[0].latitude - 33.456).abs() < 0.001);

        // Second record: low confidence, would be filtered
        assert_eq!(records[1].confidence.as_deref(), Some("low"));

        // Third record: nominal confidence, should be included
        assert_eq!(records[2].confidence.as_deref(), Some("nominal"));
        assert!((records[2].frp.unwrap() - 22.7).abs() < 0.1);
    }

    #[test]
    fn test_parse_acquisition_time() {
        // Normal HHMM time
        let dt = parse_acquisition_time("2026-03-04", "0926").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2026-03-04 09:26");

        // Midnight
        let dt = parse_acquisition_time("2025-01-15", "0000").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2025-01-15 00:00");

        // Late night
        let dt = parse_acquisition_time("2025-12-31", "2359").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2025-12-31 23:59");

        // Invalid date
        assert!(parse_acquisition_time("not-a-date", "0926").is_none());

        // Invalid time
        assert!(parse_acquisition_time("2026-03-04", "").is_none());
        assert!(parse_acquisition_time("2026-03-04", "abcd").is_none());

        // Empty strings
        assert!(parse_acquisition_time("", "").is_none());
    }

    #[test]
    fn test_source_id_stability() {
        // Verify that the same detection metadata produces the same source_id
        let lat = 33.456_f64;
        let lon = 44.123_f64;
        let lat_rounded = (lat * 10000.0).round() as i64;
        let lon_rounded = (lon * 10000.0).round() as i64;
        let source_id = format!(
            "firms:{}:{}:{}:{}:{}",
            "Suomi NPP", "2026-03-04", "0926", lat_rounded, lon_rounded
        );
        assert_eq!(source_id, "firms:Suomi NPP:2026-03-04:0926:334560:441230");

        // Same inputs always produce the same key
        let source_id2 = format!(
            "firms:{}:{}:{}:{}:{}",
            "Suomi NPP", "2026-03-04", "0926", lat_rounded, lon_rounded
        );
        assert_eq!(source_id, source_id2);
    }
}
