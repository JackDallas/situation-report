use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// All event types emitted by data sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    ConflictEvent,
    ThermalAnomaly,
    GeoEvent,
    GpsInterference,
    InternetOutage,
    CensorshipEvent,
    BgpLeak,
    BgpAnomaly,
    ThreatIntel,
    SeismicEvent,
    NuclearEvent,
    FishingEvent,
    NewsArticle,
    GeoNews,
    TelegramMessage,
    NotamEvent,
    FlightPosition,
    VesselPosition,
    CertIssued,
    ShodanBanner,
    ShodanCount,
    SourceHealth,
    BlueskyPost,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConflictEvent => "conflict_event",
            Self::ThermalAnomaly => "thermal_anomaly",
            Self::GeoEvent => "geo_event",
            Self::GpsInterference => "gps_interference",
            Self::InternetOutage => "internet_outage",
            Self::CensorshipEvent => "censorship_event",
            Self::BgpLeak => "bgp_leak",
            Self::BgpAnomaly => "bgp_anomaly",
            Self::ThreatIntel => "threat_intel",
            Self::SeismicEvent => "seismic_event",
            Self::NuclearEvent => "nuclear_event",
            Self::FishingEvent => "fishing_event",
            Self::NewsArticle => "news_article",
            Self::GeoNews => "geo_news",
            Self::TelegramMessage => "telegram_message",
            Self::NotamEvent => "notam_event",
            Self::FlightPosition => "flight_position",
            Self::VesselPosition => "vessel_position",
            Self::CertIssued => "cert_issued",
            Self::ShodanBanner => "shodan_banner",
            Self::ShodanCount => "shodan_count",
            Self::SourceHealth => "source_health",
            Self::BlueskyPost => "bluesky_post",
        }
    }

    /// Whether this is a high-volume type that gets summarized rather than
    /// published individually on SSE.
    pub fn is_high_volume(&self) -> bool {
        matches!(
            self,
            Self::FlightPosition
                | Self::VesselPosition
                | Self::BgpAnomaly
                | Self::CertIssued
                | Self::ShodanBanner
                | Self::ShodanCount
                | Self::SourceHealth
        )
    }

    /// Default summary interval in seconds for high-volume types.
    pub fn summary_interval_secs(&self) -> u64 {
        match self {
            Self::FlightPosition | Self::VesselPosition => 30,
            Self::BgpAnomaly | Self::CertIssued | Self::ShodanBanner => 60,
            _ => 60,
        }
    }

    /// Whether this event is individually important enough to publish on SSE.
    pub fn is_important_category(&self) -> bool {
        matches!(
            self,
            Self::ConflictEvent
                | Self::GeoEvent
                | Self::GpsInterference
                | Self::InternetOutage
                | Self::CensorshipEvent
                | Self::BgpLeak
                | Self::SeismicEvent
                | Self::NuclearEvent
                | Self::NewsArticle
                | Self::GeoNews
                | Self::TelegramMessage
                | Self::NotamEvent
                | Self::ThreatIntel
                | Self::FishingEvent
                | Self::BlueskyPost
        )
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All variants of [`EventType`] for exhaustive testing.
pub const ALL_EVENT_TYPES: [EventType; 23] = [
    EventType::ConflictEvent,
    EventType::ThermalAnomaly,
    EventType::GeoEvent,
    EventType::GpsInterference,
    EventType::InternetOutage,
    EventType::CensorshipEvent,
    EventType::BgpLeak,
    EventType::BgpAnomaly,
    EventType::ThreatIntel,
    EventType::SeismicEvent,
    EventType::NuclearEvent,
    EventType::FishingEvent,
    EventType::NewsArticle,
    EventType::GeoNews,
    EventType::TelegramMessage,
    EventType::NotamEvent,
    EventType::FlightPosition,
    EventType::VesselPosition,
    EventType::CertIssued,
    EventType::ShodanBanner,
    EventType::ShodanCount,
    EventType::SourceHealth,
    EventType::BlueskyPost,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn roundtrip_serde() {
        for et in ALL_EVENT_TYPES {
            let json = serde_json::to_string(&et).unwrap();
            let back: EventType = serde_json::from_str(&json).unwrap();
            assert_eq!(et, back);
        }
    }

    #[test]
    fn high_volume_check() {
        assert!(EventType::FlightPosition.is_high_volume());
        assert!(!EventType::ConflictEvent.is_high_volume());
    }

    #[test]
    fn all_event_types_have_unique_str() {
        let strs: Vec<&str> = ALL_EVENT_TYPES.iter().map(|e| e.as_str()).collect();
        let unique: HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(strs.len(), unique.len(), "Duplicate as_str() values found");
    }

    #[test]
    fn display_matches_as_str() {
        for et in ALL_EVENT_TYPES {
            assert_eq!(et.to_string(), et.as_str());
        }
    }

    #[test]
    fn high_volume_types_have_summary_interval() {
        for et in ALL_EVENT_TYPES {
            if et.is_high_volume() {
                assert!(
                    et.summary_interval_secs() > 0,
                    "{:?} is high-volume but has 0 summary interval",
                    et,
                );
            }
        }
    }

    #[test]
    fn important_and_high_volume_are_disjoint() {
        // No event type should be both high-volume (summarized) and important (published individually)
        for et in ALL_EVENT_TYPES {
            assert!(
                !(et.is_high_volume() && et.is_important_category()),
                "{:?} is marked as both high-volume and important",
                et,
            );
        }
    }

    #[test]
    fn every_type_is_either_important_or_high_volume() {
        // Every event type should be classified in at least one category
        for et in ALL_EVENT_TYPES {
            assert!(
                et.is_high_volume() || et.is_important_category(),
                "{:?} is neither high-volume nor important -- unclassified event type",
                et,
            );
        }
    }

    #[test]
    fn serde_json_string_matches_as_str() {
        for et in ALL_EVENT_TYPES {
            let json = serde_json::to_string(&et).unwrap();
            let expected = format!("\"{}\"", et.as_str());
            assert_eq!(json, expected, "serde JSON for {:?} should match as_str()", et);
        }
    }

    #[test]
    fn variant_count() {
        assert_eq!(ALL_EVENT_TYPES.len(), 23, "Expected 23 event type variants");
    }
}
