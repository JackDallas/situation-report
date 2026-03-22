use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

/// Base URL for the GeoConfirmed Azure-hosted API.
const BASE_URL: &str = "https://geoconfirmed.azurewebsites.net";

/// Number of placemarks to fetch per page per conflict.
const PAGE_SIZE: u32 = 50;

/// Conflicts to poll. These are the `shortName` values from /api/Conflict.
/// We focus on active, high-relevance conflicts.
const CONFLICTS: &[(&str, &str)] = &[
    ("Ukraine", "eastern-europe"),
    ("Israel", "middle-east"),
    ("Syria", "middle-east"),
    ("Yemen", "middle-east"),
    ("DRC", "africa"),
    ("Sahel", "africa"),
    ("Myanmar", "southeast-asia"),
];

/// Top-level API response envelope from /api/Placemark/{conflict}/{page}/{size}.
#[derive(Debug, Deserialize)]
struct PlacemarkPage {
    #[serde(default)]
    items: Vec<Placemark>,
    #[serde(default)]
    count: u64,
}

/// A single GeoConfirmed placemark.
#[derive(Debug, Deserialize)]
struct Placemark {
    /// UUID unique identifier.
    id: String,
    /// Date string in YYYY-MM-DD format.
    date: Option<String>,
    /// Latitude.
    la: Option<f64>,
    /// Longitude.
    lo: Option<f64>,
    /// Icon path encodes faction and equipment type.
    /// Format: /icons/{COLOR}/{DESTROYED}/icons/{FOLDER}/{NUMBER}.png
    icon: Option<String>,
}

/// GeoConfirmed source. Deduplication is handled by the DB via
/// `ON CONFLICT (source_type, source_id, event_time) DO NOTHING`,
/// so no in-memory watermark is needed.
#[derive(Default)]
pub struct GeoConfirmedSource;

impl GeoConfirmedSource {
    pub fn new() -> Self {
        Self
    }

    /// Derive a human-readable event title from the icon path.
    /// Icon path encodes: faction color, destroyed state, and equipment number.
    fn title_from_icon(icon: &str, conflict: &str) -> String {
        let destroyed = icon.contains("/True/");
        let equipment = icon
            .rsplit('/')
            .next()
            .and_then(|f| f.strip_suffix(".png"))
            .and_then(|n| n.parse::<u32>().ok())
            .map(|n| Self::equipment_category(n))
            .unwrap_or("asset");

        if destroyed {
            format!("Destroyed {} ({conflict})", equipment)
        } else {
            format!("{} spotted ({conflict})", capitalize(equipment))
        }
    }

    /// Map numeric icon codes to equipment category strings.
    /// Ranges are derived from GeoConfirmed icon numbering conventions.
    fn equipment_category(n: u32) -> &'static str {
        match n {
            10..=19 => "tank",
            20..=29 => "armored vehicle",
            30..=39 => "artillery",
            40..=49 => "air defense",
            50..=59 => "helicopter",
            60..=69 => "aircraft",
            70..=79 => "infantry",
            80..=89 => "naval vessel",
            90..=99 => "drone",
            100..=109 => "missile",
            110..=119 => "logistics vehicle",
            120..=129 => "radar",
            130..=139 => "checkpoint",
            140..=149 => "command post",
            150..=159 => "fortification",
            160..=169 => "ammunition depot",
            170..=179 => "naval asset",
            180..=189 => "bridge",
            190..=199 => "explosion",
            200..=209 => "geolocated event",
            _ => "military asset",
        }
    }

    /// Extract severity from the icon: destroyed assets are "high", active are "medium".
    fn severity_from_icon(icon: &str) -> Severity {
        if icon.contains("/True/") {
            Severity::High
        } else {
            Severity::Medium
        }
    }
}

impl DataSource for GeoConfirmedSource {
    fn id(&self) -> &str {
        "geoconfirmed"
    }

    fn name(&self) -> &str {
        "GeoConfirmed"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(60 * 60) // 60 minutes
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let mut all_events: Vec<InsertableEvent> = Vec::new();

        for (conflict, region) in CONFLICTS {
            debug!(conflict, region, "Polling GeoConfirmed conflict");

            // Fetch page 1 only per poll cycle to stay within rate limits.
            // The API is paginated: /api/Placemark/{conflict}/{page}/{pageSize}
            let url = format!(
                "{base}/api/Placemark/{conflict}/1/{size}",
                base = BASE_URL,
                conflict = conflict,
                size = PAGE_SIZE,
            );

            let resp = match ctx.http.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(conflict, error = %e, "GeoConfirmed request failed; skipping conflict");
                    continue;
                }
            };

            // Propagate 429 rate limits to the registry for proper backoff
            let resp = crate::rate_limit::check_rate_limit(resp, "geoconfirmed")?;

            let page: PlacemarkPage = match resp.json().await {
                Ok(p) => p,
                Err(e) => {
                    warn!(conflict, error = %e, "Failed to parse GeoConfirmed response JSON");
                    continue;
                }
            };

            debug!(
                conflict,
                fetched = page.items.len(),
                total = page.count,
                "GeoConfirmed page fetched"
            );

            for placemark in page.items {
                let icon_path = placemark.icon.as_deref().unwrap_or("");
                let title = Self::title_from_icon(icon_path, conflict);
                let severity = Self::severity_from_icon(icon_path);

                // Parse the date; fall back to now if unparseable.
                let event_time: DateTime<Utc> = placemark
                    .date
                    .as_deref()
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                    .unwrap_or_else(Utc::now);

                // source_id uses the UUID for deduplication.
                let source_id = format!("geoconfirmed:{}", placemark.id);

                let payload = serde_json::json!({
                    "id": placemark.id,
                    "conflict": conflict,
                    "date": placemark.date,
                    "lat": placemark.la,
                    "lon": placemark.lo,
                    "icon": icon_path,
                    "region": region,
                });

                all_events.push(InsertableEvent {
                    event_time,
                    source_type: SourceType::Geoconfirmed,
                    source_id: Some(source_id),
                    latitude: placemark.la,
                    longitude: placemark.lo,
                    region_code: Some(region.to_string()),
                    entity_id: Some(placemark.id),
                    entity_name: None,
                    event_type: EventType::GeoEvent,
                    severity,
                    confidence: None,
                    tags: vec![conflict.to_string()],
                    title: Some(title),
                    description: None,
                    payload,
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }
        }

        debug!(count = all_events.len(), "GeoConfirmed poll complete");
        Ok(all_events)
        })
    }
}

/// Capitalize the first character of a string slice.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
