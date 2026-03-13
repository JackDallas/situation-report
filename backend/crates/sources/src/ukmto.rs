use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, info};

use sr_types::{EventType, Severity, SourceType};

use crate::common;
use crate::{DataSource, InsertableEvent, SourceContext};

/// ArcGIS Feature Service base URL for NGA Anti-Shipping Activity Messages.
///
/// The ASAM dataset contains geo-located reports of hostile acts against ships
/// and mariners worldwide: piracy, missile strikes, hijackings, suspicious
/// approaches, naval engagements, and more. Each record includes coordinates,
/// hostility type, victim vessel details, and a narrative description.
///
/// Data was originally maintained by NGA; the database update funding ended
/// June 30 2024, but the ArcGIS feature service remains queryable for
/// historical records. New incidents from the ONI Worldwide Threat to Shipping
/// reports may be added to a successor service in the future.
const ASAM_FEATURE_URL: &str = "https://services9.arcgis.com/RHVPKKiFTONKtxq3/arcgis/rest/services/ASAM_events_V1/FeatureServer/0/query";

/// Maximum records to fetch per poll cycle.
const PAGE_SIZE: u32 = 50;

/// Hostility type codes (from the ArcGIS coded value domain).
mod hostility_code {
    pub const PIRATE_ASSAULT: i32 = 1;
    pub const NAVAL_ENGAGEMENT: i32 = 2;
    pub const SUSPICIOUS_APPROACH: i32 = 3;
    pub const KIDNAPPING: i32 = 4;
    pub const UNKNOWN: i32 = 5;
    pub const OTHER: i32 = 6;
    pub const HIJACKING: i32 = 7;
    pub const ATTEMPTED_BOARDING: i32 = 9;
    pub const MOTHERSHIP_ACTIVITY: i32 = 10;
    // 8 and 11 are also observed in the data
}

/// Victim vessel type codes (from the ArcGIS coded value domain).
mod victim_code {
    pub const _ANCHORED_SHIP: i32 = 1;
    pub const _BARGE: i32 = 2;
    pub const _CARGO_SHIP: i32 = 3;
    pub const _FISHING_VESSEL: i32 = 4;
    pub const _MERCHANT_VESSEL: i32 = 5;
    pub const _OFFSHORE_VESSEL: i32 = 6;
    pub const _PASSENGER_SHIP: i32 = 7;
    pub const _SAILING_VESSEL: i32 = 8;
    pub const _TANKER: i32 = 9;
    pub const _TUGBOAT: i32 = 10;
    pub const _VESSEL: i32 = 11;
    pub const _UNKNOWN: i32 = 12;
    pub const _OTHER: i32 = 13;
}

/// ArcGIS query response envelope.
#[derive(Debug, Deserialize)]
struct ArcGisResponse {
    #[serde(default)]
    features: Vec<ArcGisFeature>,
    /// True when more records exist beyond the returned set.
    #[serde(default, rename = "exceededTransferLimit")]
    _exceeded_transfer_limit: bool,
}

/// A single feature (ASAM record) from the ArcGIS response.
#[derive(Debug, Deserialize)]
struct ArcGisFeature {
    attributes: AsamAttributes,
    geometry: Option<AsamGeometry>,
}

/// Attributes of an ASAM record.
#[derive(Debug, Deserialize)]
struct AsamAttributes {
    /// Unique ASAM reference number (e.g. "2024-253").
    reference: Option<String>,
    /// Event date as epoch milliseconds.
    dateofocc: Option<i64>,
    /// Subregion code (NGA navigational subregion number).
    subreg: Option<String>,
    /// Description of the hostility (free text, e.g. "Missile impact nearby").
    hostility_d: Option<String>,
    /// Victim vessel description (free text, e.g. "Liberia-flagged bulk carrier").
    victim_d: Option<String>,
    /// Full narrative description of the incident.
    description: Option<String>,
    /// Hostility type coded value.
    hostilitytype_l: Option<i32>,
    /// Victim vessel type coded value.
    victim_l: Option<i32>,
    /// Navigation area (Roman numeral string, e.g. "IX").
    navarea: Option<String>,
}

/// Point geometry from the ArcGIS response.
#[derive(Debug, Deserialize)]
struct AsamGeometry {
    x: f64,
    y: f64,
}

/// Maritime security incidents source (NGA Anti-Shipping Activity Messages).
///
/// Polls the ArcGIS Feature Service for ASAM records, which include piracy,
/// missile attacks on vessels, hijackings, and other hostile maritime activity.
/// Provides critical cross-correlation data: when a vessel is attacked, the
/// ASAM report can be correlated with AIS position data and FIRMS thermal
/// anomaly detections at the same coordinates.
pub struct UkmtoSource {
    /// Set of ASAM reference numbers already seen (dedup).
    seen: Mutex<HashSet<String>>,
    /// High-water mark: most recent event timestamp seen (epoch ms).
    /// Used to query only newer records on subsequent polls.
    watermark: Mutex<i64>,
}

impl Default for UkmtoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UkmtoSource {
    pub fn new() -> Self {
        // Start watermark at 2020-01-01 to avoid pulling the entire archive
        // on first run. The registry will catch up via pagination.
        let initial_watermark = 1577836800000_i64; // 2020-01-01T00:00:00Z
        Self {
            seen: Mutex::new(HashSet::new()),
            watermark: Mutex::new(initial_watermark),
        }
    }

    /// Map hostility type code to severity.
    fn severity_for_hostility(code: Option<i32>, description: &str) -> Severity {
        let desc_lower = description.to_lowercase();

        // Check description for high-severity keywords first
        if desc_lower.contains("missile") || desc_lower.contains("ballistic")
            || desc_lower.contains("uav struck") || desc_lower.contains("drone struck")
            || desc_lower.contains("torpedo") || desc_lower.contains("mine struck")
        {
            return Severity::Critical;
        }
        if desc_lower.contains("explosion") || desc_lower.contains("projectile")
            || desc_lower.contains("fired upon") || desc_lower.contains("rpg")
            || desc_lower.contains("fire onboard")
        {
            return Severity::High;
        }

        match code {
            Some(hostility_code::NAVAL_ENGAGEMENT) => Severity::Critical,
            Some(hostility_code::HIJACKING) => Severity::Critical,
            Some(hostility_code::KIDNAPPING) => Severity::High,
            Some(hostility_code::PIRATE_ASSAULT) => Severity::High,
            Some(hostility_code::MOTHERSHIP_ACTIVITY) => Severity::Medium,
            Some(hostility_code::ATTEMPTED_BOARDING) => Severity::Medium,
            Some(hostility_code::SUSPICIOUS_APPROACH) => Severity::Low,
            Some(hostility_code::OTHER) | Some(hostility_code::UNKNOWN) => Severity::Low,
            _ => Severity::Low,
        }
    }

    /// Map hostility type code to event type.
    fn event_type_for_hostility(code: Option<i32>, description: &str) -> EventType {
        let desc_lower = description.to_lowercase();

        // Attacks (missile, drone, projectile, explosion) are conflict events
        if desc_lower.contains("missile") || desc_lower.contains("ballistic")
            || desc_lower.contains("explosion") || desc_lower.contains("projectile")
            || desc_lower.contains("uav struck") || desc_lower.contains("drone struck")
            || desc_lower.contains("fired upon") || desc_lower.contains("rpg")
            || desc_lower.contains("torpedo") || desc_lower.contains("mine")
        {
            return EventType::ConflictEvent;
        }

        match code {
            Some(hostility_code::NAVAL_ENGAGEMENT) => EventType::ConflictEvent,
            Some(hostility_code::HIJACKING) => EventType::ConflictEvent,
            Some(hostility_code::PIRATE_ASSAULT) => EventType::ConflictEvent,
            Some(hostility_code::KIDNAPPING) => EventType::ConflictEvent,
            // Advisory-level events are geo_events
            _ => EventType::GeoEvent,
        }
    }

    /// Decode hostility type code to human-readable string.
    fn hostility_type_name(code: Option<i32>) -> &'static str {
        match code {
            Some(hostility_code::PIRATE_ASSAULT) => "Pirate Assault",
            Some(hostility_code::NAVAL_ENGAGEMENT) => "Naval Engagement",
            Some(hostility_code::SUSPICIOUS_APPROACH) => "Suspicious Approach",
            Some(hostility_code::KIDNAPPING) => "Kidnapping",
            Some(hostility_code::UNKNOWN) => "Unknown",
            Some(hostility_code::OTHER) => "Other",
            Some(hostility_code::HIJACKING) => "Hijacking",
            Some(hostility_code::ATTEMPTED_BOARDING) => "Attempted Boarding",
            Some(hostility_code::MOTHERSHIP_ACTIVITY) => "Mothership Activity",
            _ => "Unknown",
        }
    }

    /// Decode victim vessel type code to human-readable string.
    fn victim_type_name(code: Option<i32>) -> &'static str {
        match code {
            Some(1) => "Anchored Ship",
            Some(2) => "Barge",
            Some(3) => "Cargo Ship",
            Some(4) => "Fishing Vessel",
            Some(5) => "Merchant Vessel",
            Some(6) => "Offshore Vessel",
            Some(7) => "Passenger Ship",
            Some(8) => "Sailing Vessel",
            Some(9) => "Tanker",
            Some(10) => "Tugboat",
            Some(11) => "Vessel",
            Some(12) => "Unknown",
            Some(13) => "Other",
            _ => "Unknown",
        }
    }

    /// Build a concise event title from ASAM record fields.
    fn build_title(attrs: &AsamAttributes) -> String {
        let hostility = attrs.hostility_d.as_deref().unwrap_or_else(|| {
            Self::hostility_type_name(attrs.hostilitytype_l)
        });

        // Try to extract vessel name from victim_d
        let vessel_info = attrs.victim_d.as_deref().unwrap_or("");

        // Extract region from description (first word before colon is typically the region)
        let region = attrs.description.as_deref()
            .and_then(|d| d.split(':').next())
            .unwrap_or("");

        if !vessel_info.is_empty() && !region.is_empty() {
            format!("{}: {} — {}", region.trim(), hostility, vessel_info)
        } else if !vessel_info.is_empty() {
            format!("{} — {}", hostility, vessel_info)
        } else if !region.is_empty() {
            format!("{}: {}", region.trim(), hostility)
        } else {
            hostility.to_string()
        }
    }

    /// Extract a vessel name from the victim_d field if one is embedded.
    /// Looks for patterns like "Liberia-flagged bulk carrier TRANSWORLD NAVIGATOR".
    fn extract_vessel_name(victim_d: &str) -> Option<String> {
        // Pattern: look for an ALL-CAPS name at the end
        let parts: Vec<&str> = victim_d.rsplitn(2, char::is_whitespace).collect();
        if parts.len() >= 1 {
            // Walk backwards collecting uppercase words
            let words: Vec<&str> = victim_d.split_whitespace().collect();
            let mut name_words = Vec::new();
            for word in words.iter().rev() {
                // Check if the word is primarily uppercase letters (vessel name)
                let upper_count = word.chars().filter(|c| c.is_ascii_uppercase()).count();
                let alpha_count = word.chars().filter(|c| c.is_ascii_alphabetic()).count();
                if alpha_count > 0 && upper_count == alpha_count && word.len() >= 2 {
                    name_words.push(*word);
                } else {
                    break;
                }
            }
            if !name_words.is_empty() {
                name_words.reverse();
                let name = name_words.join(" ");
                // Filter out single-word generic names
                if name.len() >= 3 && name != "MSC" && name != "MV" && name != "MT" {
                    return Some(name);
                }
            }
        }
        None
    }

    /// Build tags from the ASAM record.
    fn build_tags(attrs: &AsamAttributes) -> Vec<String> {
        let mut tags = vec![
            "maritime".to_string(),
            "maritime-security".to_string(),
            "source:ASAM".to_string(),
        ];

        // Add hostility type as tag
        let hostility = Self::hostility_type_name(attrs.hostilitytype_l);
        if hostility != "Unknown" {
            tags.push(hostility.to_lowercase().replace(' ', "-"));
        }

        // Add victim vessel type as tag
        let victim = Self::victim_type_name(attrs.victim_l);
        if victim != "Unknown" {
            tags.push(format!("vessel:{}", victim.to_lowercase().replace(' ', "-")));
        }

        // Add navigation area
        if let Some(ref navarea) = attrs.navarea {
            tags.push(format!("navarea:{}", navarea));
        }

        // Add subregion
        if let Some(ref subreg) = attrs.subreg {
            tags.push(format!("subreg:{}", subreg));
        }

        // Detect Houthi / Red Sea / Gulf of Aden references
        if let Some(ref desc) = attrs.description {
            let desc_lower = desc.to_lowercase();
            if desc_lower.contains("houthi") {
                tags.push("actor:houthi".to_string());
            }
            if desc_lower.contains("red sea") {
                tags.push("red-sea".to_string());
            }
            if desc_lower.contains("gulf of aden") {
                tags.push("gulf-of-aden".to_string());
            }
            if desc_lower.contains("arabian sea") {
                tags.push("arabian-sea".to_string());
            }
            if desc_lower.contains("strait") {
                tags.push("strait".to_string());
            }
            if desc_lower.contains("piracy") || desc_lower.contains("pirate") {
                tags.push("piracy".to_string());
            }
        }

        tags
    }
}

#[async_trait]
impl DataSource for UkmtoSource {
    fn id(&self) -> &str {
        "ukmto"
    }

    fn name(&self) -> &str {
        "Maritime Security (ASAM)"
    }

    fn default_interval(&self) -> Duration {
        // 5 minutes — the ArcGIS service is not rate-limited aggressively
        Duration::from_secs(300)
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let watermark = {
            let wm = self.watermark.lock().unwrap_or_else(|e| e.into_inner());
            *wm
        };

        // Query ArcGIS for records newer than our watermark, sorted by date descending.
        // The where clause filters by event date > watermark (epoch ms).
        let where_clause = format!("dateofocc > {}", watermark);
        let url = format!(
            "{}?where={}&outFields=*&f=json&resultRecordCount={}&orderByFields=dateofocc+DESC&returnGeometry=true",
            ASAM_FEATURE_URL,
            common::urlencode(&where_clause),
            PAGE_SIZE,
        );

        debug!(url = %url, "Polling ASAM ArcGIS feature service");

        let resp = ctx.http.get(&url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("ASAM query returned HTTP {}", resp.status());
        }

        let body: ArcGisResponse = resp.json().await?;
        let mut events = Vec::new();
        let mut max_timestamp = watermark;

        for feature in body.features {
            let attrs = &feature.attributes;

            // Dedup by reference number
            let reference = match attrs.reference.as_deref() {
                Some(r) if !r.is_empty() => r,
                _ => continue,
            };

            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(reference) {
                    continue;
                }
                seen.insert(reference.to_string());
                // Keep seen set bounded
                if seen.len() > 5000 {
                    seen.clear();
                }
            }

            // Parse event time from epoch ms
            let event_time = match attrs.dateofocc {
                Some(ms) => {
                    if ms > max_timestamp {
                        max_timestamp = ms;
                    }
                    DateTime::from_timestamp_millis(ms).unwrap_or_else(Utc::now)
                }
                None => Utc::now(),
            };

            // Extract coordinates from geometry
            let (latitude, longitude) = match &feature.geometry {
                Some(geo) => (Some(geo.y), Some(geo.x)),
                None => (None, None),
            };

            // Determine region from coordinates
            let region_code = latitude.zip(longitude)
                .and_then(|(lat, lon)| common::region_from_coords(lat, lon))
                .map(String::from);

            let description_text = attrs.description.as_deref().unwrap_or("");
            let severity = Self::severity_for_hostility(attrs.hostilitytype_l, description_text);
            let event_type = Self::event_type_for_hostility(attrs.hostilitytype_l, description_text);
            let title = Self::build_title(attrs);
            let tags = Self::build_tags(attrs);

            // Try to extract vessel name
            let entity_name = attrs.victim_d.as_deref()
                .and_then(Self::extract_vessel_name);

            let payload = serde_json::json!({
                "reference": reference,
                "hostility_type": Self::hostility_type_name(attrs.hostilitytype_l),
                "hostility_code": attrs.hostilitytype_l,
                "hostility_detail": attrs.hostility_d,
                "victim_type": Self::victim_type_name(attrs.victim_l),
                "victim_code": attrs.victim_l,
                "victim_detail": attrs.victim_d,
                "navarea": attrs.navarea,
                "subregion": attrs.subreg,
                "description": attrs.description,
                "source": "NGA ASAM",
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Ukmto,
                source_id: Some(format!("asam-{}", reference)),
                longitude,
                latitude,
                region_code,
                entity_id: None,
                entity_name,
                event_type,
                severity,
                confidence: Some(0.9), // ASAM data is well-verified
                tags,
                title: Some(title),
                description: Some(description_text.to_string()),
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Update watermark
        if max_timestamp > watermark {
            let mut wm = self.watermark.lock().unwrap_or_else(|e| e.into_inner());
            *wm = max_timestamp;
        }

        if !events.is_empty() {
            info!(count = events.len(), "Fetched maritime security incidents from ASAM");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_missile_is_critical() {
        assert_eq!(
            UkmtoSource::severity_for_hostility(Some(2), "A missile impacted the water nearby"),
            Severity::Critical,
        );
    }

    #[test]
    fn severity_explosion_is_high() {
        assert_eq!(
            UkmtoSource::severity_for_hostility(Some(6), "An explosion occurred nearby"),
            Severity::High,
        );
    }

    #[test]
    fn severity_hijacking_is_critical() {
        assert_eq!(
            UkmtoSource::severity_for_hostility(Some(hostility_code::HIJACKING), "Vessel hijacked"),
            Severity::Critical,
        );
    }

    #[test]
    fn severity_suspicious_approach_is_low() {
        assert_eq!(
            UkmtoSource::severity_for_hostility(Some(hostility_code::SUSPICIOUS_APPROACH), "Small boat approached"),
            Severity::Low,
        );
    }

    #[test]
    fn severity_attempted_boarding_is_medium() {
        assert_eq!(
            UkmtoSource::severity_for_hostility(Some(hostility_code::ATTEMPTED_BOARDING), "Robbers attempted to board"),
            Severity::Medium,
        );
    }

    #[test]
    fn event_type_missile_is_conflict() {
        assert_eq!(
            UkmtoSource::event_type_for_hostility(Some(2), "A missile struck the vessel"),
            EventType::ConflictEvent,
        );
    }

    #[test]
    fn event_type_suspicious_approach_is_geo() {
        assert_eq!(
            UkmtoSource::event_type_for_hostility(Some(hostility_code::SUSPICIOUS_APPROACH), "Small boat approached"),
            EventType::GeoEvent,
        );
    }

    #[test]
    fn event_type_naval_engagement_is_conflict() {
        assert_eq!(
            UkmtoSource::event_type_for_hostility(Some(hostility_code::NAVAL_ENGAGEMENT), "Naval engagement reported"),
            EventType::ConflictEvent,
        );
    }

    #[test]
    fn extract_vessel_name_all_caps_suffix() {
        assert_eq!(
            UkmtoSource::extract_vessel_name("Liberia-flagged bulk carrier TRANSWORLD NAVIGATOR"),
            Some("TRANSWORLD NAVIGATOR".to_string()),
        );
    }

    #[test]
    fn extract_vessel_name_with_prefix() {
        assert_eq!(
            UkmtoSource::extract_vessel_name("Liberia-flagged container ship MSC SARAH V"),
            None, // "V" is only 1 char, gets filtered
        );
    }

    #[test]
    fn extract_vessel_name_no_caps() {
        assert_eq!(
            UkmtoSource::extract_vessel_name("St Kitts and Nevis-flagged bulk carrier"),
            None,
        );
    }

    #[test]
    fn hostility_type_name_pirate() {
        assert_eq!(UkmtoSource::hostility_type_name(Some(1)), "Pirate Assault");
    }

    #[test]
    fn hostility_type_name_hijack() {
        assert_eq!(UkmtoSource::hostility_type_name(Some(7)), "Hijacking");
    }

    #[test]
    fn victim_type_name_tanker() {
        assert_eq!(UkmtoSource::victim_type_name(Some(9)), "Tanker");
    }

    #[test]
    fn build_title_with_all_fields() {
        let attrs = AsamAttributes {
            reference: Some("2024-253".to_string()),
            dateofocc: Some(1719334854000),
            subreg: Some("62".to_string()),
            hostility_d: Some("Missile impact nearby".to_string()),
            victim_d: Some("St Kitts and Nevis-flagged bulk carrier".to_string()),
            description: Some("GULF OF ADEN: On 25 June at 1700 UTC, a missile impacted the water".to_string()),
            hostilitytype_l: Some(2),
            victim_l: Some(3),
            navarea: Some("IX".to_string()),
        };
        let title = UkmtoSource::build_title(&attrs);
        assert!(title.contains("GULF OF ADEN"));
        assert!(title.contains("Missile impact nearby"));
    }

    #[test]
    fn build_tags_includes_maritime() {
        let attrs = AsamAttributes {
            reference: Some("2024-253".to_string()),
            dateofocc: Some(1719334854000),
            subreg: Some("62".to_string()),
            hostility_d: Some("Missile impact".to_string()),
            victim_d: None,
            description: Some("GULF OF ADEN: missile".to_string()),
            hostilitytype_l: Some(2),
            victim_l: Some(3),
            navarea: Some("IX".to_string()),
        };
        let tags = UkmtoSource::build_tags(&attrs);
        assert!(tags.contains(&"maritime".to_string()));
        assert!(tags.contains(&"maritime-security".to_string()));
        assert!(tags.contains(&"source:ASAM".to_string()));
        assert!(tags.contains(&"navarea:IX".to_string()));
    }

    #[test]
    fn build_tags_detects_red_sea() {
        let attrs = AsamAttributes {
            reference: None,
            dateofocc: None,
            subreg: None,
            hostility_d: None,
            victim_d: None,
            description: Some("RED SEA: UAV struck a vessel near Hodeida".to_string()),
            hostilitytype_l: None,
            victim_l: None,
            navarea: None,
        };
        let tags = UkmtoSource::build_tags(&attrs);
        assert!(tags.contains(&"red-sea".to_string()));
    }

    #[test]
    fn build_tags_detects_gulf_of_aden() {
        let attrs = AsamAttributes {
            reference: None,
            dateofocc: None,
            subreg: None,
            hostility_d: None,
            victim_d: None,
            description: Some("GULF OF ADEN: On 30 August, two ballistic missiles targeted a vessel".to_string()),
            hostilitytype_l: None,
            victim_l: None,
            navarea: None,
        };
        let tags = UkmtoSource::build_tags(&attrs);
        assert!(tags.contains(&"gulf-of-aden".to_string()));
    }

    #[test]
    fn region_from_gulf_of_aden_coords() {
        // 12°N, 45°E — Gulf of Aden
        assert_eq!(common::region_from_coords(12.0, 45.0), Some("middle-east"));
    }

    #[test]
    fn region_from_singapore_strait_coords() {
        // 1°N, 104°E — Singapore Strait
        let region = common::region_from_coords(1.0, 104.0);
        assert!(region.is_some());
    }

    #[test]
    fn region_from_gulf_of_guinea_coords() {
        // 4°N, 3°E — Gulf of Guinea
        let region = common::region_from_coords(4.0, 3.0);
        assert!(region.is_some());
    }

    #[test]
    fn default_watermark_is_2020() {
        let source = UkmtoSource::new();
        let wm = source.watermark.lock().unwrap();
        assert_eq!(*wm, 1577836800000);
    }
}
