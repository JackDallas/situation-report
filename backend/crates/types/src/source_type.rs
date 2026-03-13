use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// All data source types in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub enum SourceType {
    #[serde(rename = "acled")]
    Acled,
    #[serde(rename = "adsb-fi")]
    AdsbFi,
    #[serde(rename = "adsb-lol")]
    AdsbLol,
    #[serde(rename = "ais")]
    Ais,
    #[serde(rename = "airplaneslive")]
    AirplanesLive,
    #[serde(rename = "bgp")]
    Bgp,
    #[serde(rename = "certstream")]
    Certstream,
    #[serde(rename = "cloudflare")]
    Cloudflare,
    #[serde(rename = "cloudflare-bgp")]
    CloudflareBgp,
    #[serde(rename = "firms")]
    Firms,
    #[serde(rename = "gdacs")]
    Gdacs,
    #[serde(rename = "gdelt")]
    Gdelt,
    #[serde(rename = "gdelt-geo")]
    GdeltGeo,
    #[serde(rename = "geoconfirmed")]
    Geoconfirmed,
    #[serde(rename = "gpsjam")]
    Gpsjam,
    #[serde(rename = "gfw")]
    Gfw,
    #[serde(rename = "ioda")]
    Ioda,
    #[serde(rename = "notam")]
    Notam,
    #[serde(rename = "nuclear")]
    Nuclear,
    #[serde(rename = "ooni")]
    Ooni,
    #[serde(rename = "opensky")]
    Opensky,
    #[serde(rename = "otx")]
    Otx,
    #[serde(rename = "rss-news")]
    RssNews,
    #[serde(rename = "shodan")]
    Shodan,
    #[serde(rename = "telegram")]
    Telegram,
    #[serde(rename = "ukmto")]
    Ukmto,
    #[serde(rename = "ukmto-warnings")]
    UkmtoWarnings,
    #[serde(rename = "usgs")]
    Usgs,
    #[serde(rename = "copernicus")]
    Copernicus,
    #[serde(rename = "bluesky")]
    Bluesky,
}

impl SourceType {
    /// Returns `true` for any ADS-B / flight-tracking data source.
    pub fn is_flight_source(&self) -> bool {
        matches!(self, Self::AirplanesLive | Self::AdsbLol | Self::AdsbFi | Self::Opensky)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Acled => "acled",
            Self::AdsbFi => "adsb-fi",
            Self::AdsbLol => "adsb-lol",
            Self::Ais => "ais",
            Self::AirplanesLive => "airplaneslive",
            Self::Bgp => "bgp",
            Self::Certstream => "certstream",
            Self::Cloudflare => "cloudflare",
            Self::CloudflareBgp => "cloudflare-bgp",
            Self::Gdacs => "gdacs",
            Self::Firms => "firms",
            Self::Gdelt => "gdelt",
            Self::GdeltGeo => "gdelt-geo",
            Self::Geoconfirmed => "geoconfirmed",
            Self::Gpsjam => "gpsjam",
            Self::Gfw => "gfw",
            Self::Ioda => "ioda",
            Self::Notam => "notam",
            Self::Nuclear => "nuclear",
            Self::Ooni => "ooni",
            Self::Opensky => "opensky",
            Self::Otx => "otx",
            Self::RssNews => "rss-news",
            Self::Shodan => "shodan",
            Self::Telegram => "telegram",
            Self::Ukmto => "ukmto",
            Self::UkmtoWarnings => "ukmto-warnings",
            Self::Usgs => "usgs",
            Self::Copernicus => "copernicus",
            Self::Bluesky => "bluesky",
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All variants of [`SourceType`] for exhaustive testing.
pub const ALL_SOURCE_TYPES: [SourceType; 30] = [
    SourceType::Acled,
    SourceType::AdsbFi,
    SourceType::AdsbLol,
    SourceType::Ais,
    SourceType::AirplanesLive,
    SourceType::Bgp,
    SourceType::Certstream,
    SourceType::Cloudflare,
    SourceType::CloudflareBgp,
    SourceType::Firms,
    SourceType::Gdacs,
    SourceType::Gdelt,
    SourceType::GdeltGeo,
    SourceType::Geoconfirmed,
    SourceType::Gpsjam,
    SourceType::Gfw,
    SourceType::Ioda,
    SourceType::Notam,
    SourceType::Nuclear,
    SourceType::Ooni,
    SourceType::Opensky,
    SourceType::Otx,
    SourceType::RssNews,
    SourceType::Shodan,
    SourceType::Telegram,
    SourceType::Ukmto,
    SourceType::UkmtoWarnings,
    SourceType::Usgs,
    SourceType::Copernicus,
    SourceType::Bluesky,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn roundtrip_serde() {
        // Test that hyphenated names survive round-trip
        let json = serde_json::to_string(&SourceType::CloudflareBgp).unwrap();
        assert_eq!(json, "\"cloudflare-bgp\"");
        let back: SourceType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SourceType::CloudflareBgp);
    }

    #[test]
    fn all_variants_roundtrip() {
        for st in ALL_SOURCE_TYPES {
            let json = serde_json::to_string(&st).unwrap();
            let back: SourceType = serde_json::from_str(&json).unwrap();
            assert_eq!(st, back);
        }
    }

    #[test]
    fn all_source_types_have_unique_str() {
        let strs: Vec<&str> = ALL_SOURCE_TYPES.iter().map(|s| s.as_str()).collect();
        let unique: HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(strs.len(), unique.len(), "Duplicate as_str() values found");
    }

    #[test]
    fn display_matches_as_str() {
        for st in ALL_SOURCE_TYPES {
            assert_eq!(st.to_string(), st.as_str());
        }
    }

    #[test]
    fn serde_json_string_matches_as_str() {
        for st in ALL_SOURCE_TYPES {
            let json = serde_json::to_string(&st).unwrap();
            let expected = format!("\"{}\"", st.as_str());
            assert_eq!(json, expected, "serde JSON for {:?} should match as_str()", st);
        }
    }

    #[test]
    fn variant_count() {
        assert_eq!(ALL_SOURCE_TYPES.len(), 30, "Expected 30 source type variants");
    }
}
