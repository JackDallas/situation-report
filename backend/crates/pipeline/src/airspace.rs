//! Lightweight in-memory spatial index of restricted/prohibited airspace zones.
//!
//! Loaded once at pipeline startup from the static GeoJSON data.
//! Used to annotate aviation events with airspace context and check
//! if aircraft positions fall within restricted zones.
//!
//! Also maintains a cache of active NOTAMs for cross-referencing.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A restricted/prohibited airspace zone with a bounding box for fast checks.
#[derive(Debug, Clone)]
pub struct AirspaceZone {
    pub designator: String,
    pub name: String,
    pub zone_type: AirspaceZoneType,
    pub country: String,
    /// Bounding box: [min_lon, min_lat, max_lon, max_lat]
    pub bbox: [f64; 4],
    /// Polygon ring(s) for precise point-in-polygon check.
    /// Each ring is a vec of (lon, lat) pairs.
    pub rings: Vec<Vec<(f64, f64)>>,
}

/// The type of airspace zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirspaceZoneType {
    Prohibited,
    Restricted,
    Danger,
    Warning,
}

impl AirspaceZoneType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AirspaceZoneType::Prohibited => "PROHIBITED",
            AirspaceZoneType::Restricted => "RESTRICTED",
            AirspaceZoneType::Danger => "DANGER",
            AirspaceZoneType::Warning => "WARNING",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "PROHIBITED" => Some(AirspaceZoneType::Prohibited),
            "RESTRICTED" => Some(AirspaceZoneType::Restricted),
            "DANGER" => Some(AirspaceZoneType::Danger),
            "WARNING" => Some(AirspaceZoneType::Warning),
            _ => None,
        }
    }
}

/// Result of an airspace check for a given position.
#[derive(Debug, Clone)]
pub struct AirspaceHit {
    pub designator: String,
    pub name: String,
    pub zone_type: AirspaceZoneType,
    pub country: String,
}

/// An active NOTAM zone (from SSE ingestion).
#[derive(Debug, Clone)]
pub struct ActiveNotam {
    pub notam_id: String,
    pub center_lat: f64,
    pub center_lon: f64,
    pub radius_nm: f64,
    pub title: String,
    pub fir: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub ingested_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Spatial Index
// ---------------------------------------------------------------------------

/// In-memory spatial index for airspace zones and active NOTAMs.
pub struct AirspaceIndex {
    /// Static restricted/prohibited zones.
    zones: Vec<AirspaceZone>,
    /// Active NOTAMs keyed by notam_id.
    active_notams: RwLock<HashMap<String, ActiveNotam>>,
}

impl AirspaceIndex {
    /// Create an empty index.
    pub fn empty() -> Self {
        Self {
            zones: Vec::new(),
            active_notams: RwLock::new(HashMap::new()),
        }
    }

    /// Load zones from a GeoJSON string. Filters to PROHIBITED and key RESTRICTED
    /// zones in conflict-relevant countries.
    pub fn from_geojson(json_str: &str) -> Self {
        let parsed: Result<GeoJsonFeatureCollection, _> = serde_json::from_str(json_str);
        let collection = match parsed {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to parse restricted airspace GeoJSON: {e}");
                return Self::empty();
            }
        };

        // Countries of interest for conflict monitoring
        const CONFLICT_COUNTRIES: &[&str] = &[
            "IL", "PS", "IR", "IQ", "SY", "LB", "YE", "SA", "AE", "QA", "BH", "OM", "KW",
            "TR", "UA", "RU", "BY", "KP", "CN", "TW", "MM", "LY", "SD", "SO", "ET",
            "GB", "FR", "DE", "PL", "GR", "RO", "BG", "FI", "NO", "SE", "LT", "LV", "EE",
        ];

        let mut zones = Vec::new();
        for feature in &collection.features {
            let props = &feature.properties;
            let zone_type = match AirspaceZoneType::from_str(&props.r#type) {
                Some(t) => t,
                None => continue,
            };

            // Include all PROHIBITED zones globally.
            // For RESTRICTED/DANGER, only include conflict-relevant countries.
            match zone_type {
                AirspaceZoneType::Prohibited => {}
                AirspaceZoneType::Restricted | AirspaceZoneType::Danger => {
                    if !CONFLICT_COUNTRIES.contains(&props.country.as_str()) {
                        continue;
                    }
                }
                AirspaceZoneType::Warning => continue, // skip warnings — too many, low value
            }

            // Extract polygon rings
            let rings = match &feature.geometry {
                GeoJsonGeometry::Polygon { coordinates } => {
                    extract_rings(coordinates)
                }
                GeoJsonGeometry::MultiPolygon { coordinates } => {
                    coordinates.iter().flat_map(|poly| extract_rings(poly)).collect()
                }
                _ => continue,
            };

            if rings.is_empty() {
                continue;
            }

            // Compute bounding box
            let (mut min_lon, mut min_lat) = (f64::MAX, f64::MAX);
            let (mut max_lon, mut max_lat) = (f64::MIN, f64::MIN);
            for ring in &rings {
                for &(lon, lat) in ring {
                    min_lon = min_lon.min(lon);
                    min_lat = min_lat.min(lat);
                    max_lon = max_lon.max(lon);
                    max_lat = max_lat.max(lat);
                }
            }

            zones.push(AirspaceZone {
                designator: props.designator.clone(),
                name: props.name.clone(),
                zone_type,
                country: props.country.clone(),
                bbox: [min_lon, min_lat, max_lon, max_lat],
                rings,
            });
        }

        info!(
            prohibited = zones.iter().filter(|z| z.zone_type == AirspaceZoneType::Prohibited).count(),
            restricted = zones.iter().filter(|z| z.zone_type == AirspaceZoneType::Restricted).count(),
            danger = zones.iter().filter(|z| z.zone_type == AirspaceZoneType::Danger).count(),
            total = zones.len(),
            "Airspace index loaded"
        );

        Self {
            zones,
            active_notams: RwLock::new(HashMap::new()),
        }
    }

    /// Check if a point (lat, lon) falls within any indexed airspace zone.
    /// Returns all matching zones, sorted by severity (PROHIBITED first).
    pub fn check_position(&self, lat: f64, lon: f64) -> Vec<AirspaceHit> {
        let mut hits = Vec::new();

        for zone in &self.zones {
            // Fast bounding box pre-filter
            if lon < zone.bbox[0] || lat < zone.bbox[1]
                || lon > zone.bbox[2] || lat > zone.bbox[3]
            {
                continue;
            }

            // Precise point-in-polygon check
            if point_in_polygon(lon, lat, &zone.rings) {
                hits.push(AirspaceHit {
                    designator: zone.designator.clone(),
                    name: zone.name.clone(),
                    zone_type: zone.zone_type,
                    country: zone.country.clone(),
                });
            }
        }

        // Sort: PROHIBITED first, then RESTRICTED, then DANGER
        hits.sort_by_key(|h| match h.zone_type {
            AirspaceZoneType::Prohibited => 0,
            AirspaceZoneType::Restricted => 1,
            AirspaceZoneType::Danger => 2,
            AirspaceZoneType::Warning => 3,
        });

        hits
    }

    /// Check if a point is within any active NOTAM zone.
    /// Uses great-circle distance approximation.
    pub fn check_notams(&self, lat: f64, lon: f64) -> Vec<ActiveNotam> {
        let guard = self.active_notams.read().unwrap_or_else(|e| e.into_inner());
        let now = Utc::now();
        let mut hits = Vec::new();

        for notam in guard.values() {
            // Skip expired NOTAMs
            if let Some(exp) = notam.expires_at {
                if exp < now {
                    continue;
                }
            }
            // Skip NOTAMs older than 24h without explicit expiry
            if notam.expires_at.is_none() {
                let age = now - notam.ingested_at;
                if age.num_hours() > 24 {
                    continue;
                }
            }

            let dist_nm = haversine_nm(lat, lon, notam.center_lat, notam.center_lon);
            if dist_nm <= notam.radius_nm {
                hits.push(notam.clone());
            }
        }

        hits
    }

    /// Register an active NOTAM (called when pipeline processes a NOTAM event).
    pub fn register_notam(&self, notam: ActiveNotam) {
        let mut guard = self.active_notams.write().unwrap_or_else(|e| e.into_inner());
        guard.insert(notam.notam_id.clone(), notam);
    }

    /// Prune expired NOTAMs from the cache. Called periodically.
    pub fn prune_notams(&self) {
        let mut guard = self.active_notams.write().unwrap_or_else(|e| e.into_inner());
        let now = Utc::now();
        let before = guard.len();
        guard.retain(|_, n| {
            if let Some(exp) = n.expires_at {
                exp > now
            } else {
                (now - n.ingested_at).num_hours() < 24
            }
        });
        let pruned = before - guard.len();
        if pruned > 0 {
            debug!(pruned, remaining = guard.len(), "Pruned expired NOTAMs");
        }
    }

    /// Number of indexed static zones.
    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    /// Number of active NOTAMs.
    pub fn active_notam_count(&self) -> usize {
        self.active_notams.read().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Shared airspace index — used by pipeline + AppState.
pub type SharedAirspaceIndex = Arc<AirspaceIndex>;

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// Ray-casting point-in-polygon test.
/// Works with multiple rings (first is exterior, rest are holes).
fn point_in_polygon(x: f64, y: f64, rings: &[Vec<(f64, f64)>]) -> bool {
    if rings.is_empty() {
        return false;
    }
    // Check exterior ring
    let inside_exterior = ray_cast(x, y, &rings[0]);
    if !inside_exterior {
        return false;
    }
    // Check holes (subsequent rings) — if inside a hole, point is outside polygon
    for hole in rings.iter().skip(1) {
        if ray_cast(x, y, hole) {
            return false;
        }
    }
    true
}

/// Standard ray-casting algorithm for a single ring.
fn ray_cast(x: f64, y: f64, ring: &[(f64, f64)]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Haversine distance in nautical miles.
fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_NM: f64 = 3440.065; // nautical miles
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_NM * c
}

/// Extract polygon rings from GeoJSON coordinate arrays.
fn extract_rings(coordinates: &[Vec<Vec<f64>>]) -> Vec<Vec<(f64, f64)>> {
    coordinates
        .iter()
        .filter_map(|ring| {
            let points: Vec<(f64, f64)> = ring
                .iter()
                .filter_map(|coord| {
                    if coord.len() >= 2 {
                        Some((coord[0], coord[1])) // (lon, lat)
                    } else {
                        None
                    }
                })
                .collect();
            if points.len() >= 3 {
                Some(points)
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GeoJSON deserialization (minimal, just what we need)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GeoJsonFeatureCollection {
    features: Vec<GeoJsonFeature>,
}

#[derive(Deserialize)]
struct GeoJsonFeature {
    properties: GeoJsonProperties,
    geometry: GeoJsonGeometry,
}

#[derive(Deserialize)]
struct GeoJsonProperties {
    designator: String,
    name: String,
    r#type: String,
    country: String,
    #[allow(dead_code)]
    #[serde(default)]
    by_notam: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum GeoJsonGeometry {
    Polygon {
        coordinates: Vec<Vec<Vec<f64>>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<Vec<f64>>>>,
    },
    #[serde(other)]
    Other,
}

// ---------------------------------------------------------------------------
// Annotate aviation events
// ---------------------------------------------------------------------------

/// Annotate an aviation event's payload with airspace context.
/// Returns true if any airspace alert was added.
pub fn annotate_aviation_event(
    index: &AirspaceIndex,
    event: &mut sr_sources::InsertableEvent,
) -> bool {
    use sr_types::{EventType, Severity, SourceType};

    // Only process aviation events
    let is_aviation = matches!(
        event.source_type,
        SourceType::AirplanesLive | SourceType::Opensky | SourceType::AdsbFi | SourceType::AdsbLol
    ) || matches!(
        event.event_type,
        EventType::FlightPosition
    );

    if !is_aviation {
        return false;
    }

    let (lat, lon) = match (event.latitude, event.longitude) {
        (Some(lat), Some(lon)) => (lat, lon),
        _ => return false,
    };

    let mut annotated = false;

    // Check static restricted zones
    let zone_hits = index.check_position(lat, lon);
    if !zone_hits.is_empty() {
        let primary = &zone_hits[0];
        let payload = event.payload.as_object_mut();
        if let Some(obj) = payload {
            obj.insert("airspace_alert".to_string(), serde_json::Value::Bool(true));
            obj.insert(
                "airspace_zone".to_string(),
                serde_json::Value::String(primary.designator.clone()),
            );
            obj.insert(
                "airspace_zone_name".to_string(),
                serde_json::Value::String(primary.name.clone()),
            );
            obj.insert(
                "airspace_zone_type".to_string(),
                serde_json::Value::String(primary.zone_type.as_str().to_string()),
            );
            obj.insert(
                "airspace_zone_country".to_string(),
                serde_json::Value::String(primary.country.clone()),
            );

            if zone_hits.len() > 1 {
                let all_zones: Vec<serde_json::Value> = zone_hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "designator": h.designator,
                            "name": h.name,
                            "type": h.zone_type.as_str(),
                            "country": h.country,
                        })
                    })
                    .collect();
                obj.insert(
                    "airspace_zones_all".to_string(),
                    serde_json::Value::Array(all_zones),
                );
            }
        }

        // Boost severity for PROHIBITED zones
        if primary.zone_type == AirspaceZoneType::Prohibited {
            event.severity = event.severity.max(Severity::High);
            if !event.tags.contains(&"airspace-prohibited".to_string()) {
                event.tags.push("airspace-prohibited".to_string());
            }
        } else if primary.zone_type == AirspaceZoneType::Restricted {
            event.severity = event.severity.max(Severity::Medium);
            if !event.tags.contains(&"airspace-restricted".to_string()) {
                event.tags.push("airspace-restricted".to_string());
            }
        }

        annotated = true;
    }

    // Check active NOTAMs
    let notam_hits = index.check_notams(lat, lon);
    if !notam_hits.is_empty() {
        let payload = event.payload.as_object_mut();
        if let Some(obj) = payload {
            obj.insert("notam_alert".to_string(), serde_json::Value::Bool(true));
            let notam_info: Vec<serde_json::Value> = notam_hits
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "notam_id": n.notam_id,
                        "title": n.title,
                        "fir": n.fir,
                        "radius_nm": n.radius_nm,
                    })
                })
                .collect();
            obj.insert(
                "active_notams".to_string(),
                serde_json::Value::Array(notam_info),
            );
        }
        if !event.tags.contains(&"notam-active".to_string()) {
            event.tags.push("notam-active".to_string());
        }
        annotated = true;
    }

    annotated
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_cast_simple_square() {
        // Square from (0,0) to (10,10)
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        assert!(ray_cast(5.0, 5.0, &ring));
        assert!(!ray_cast(15.0, 5.0, &ring));
        assert!(!ray_cast(-1.0, 5.0, &ring));
    }

    #[test]
    fn test_point_in_polygon_with_hole() {
        // Outer ring: big square
        let outer = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 0.0)];
        // Hole: small square in center
        let hole = vec![(8.0, 8.0), (12.0, 8.0), (12.0, 12.0), (8.0, 12.0), (8.0, 8.0)];
        let rings = vec![outer, hole];

        assert!(point_in_polygon(5.0, 5.0, &rings)); // Inside outer, outside hole
        assert!(!point_in_polygon(10.0, 10.0, &rings)); // Inside hole
        assert!(!point_in_polygon(25.0, 10.0, &rings)); // Outside outer
    }

    #[test]
    fn test_haversine_zero_distance() {
        let d = haversine_nm(51.5, -0.1, 51.5, -0.1);
        assert!(d < 0.01);
    }

    #[test]
    fn test_haversine_known_distance() {
        // London to Paris is roughly 190 NM
        let d = haversine_nm(51.5, -0.1, 48.9, 2.3);
        assert!(d > 170.0 && d < 210.0, "Distance was {d}");
    }

    #[test]
    fn test_airspace_zone_type_roundtrip() {
        assert_eq!(AirspaceZoneType::from_str("PROHIBITED"), Some(AirspaceZoneType::Prohibited));
        assert_eq!(AirspaceZoneType::from_str("restricted"), Some(AirspaceZoneType::Restricted));
        assert_eq!(AirspaceZoneType::from_str("DANGER"), Some(AirspaceZoneType::Danger));
        assert_eq!(AirspaceZoneType::from_str("unknown"), None);
    }

    #[test]
    fn test_empty_index() {
        let idx = AirspaceIndex::empty();
        assert_eq!(idx.zone_count(), 0);
        assert!(idx.check_position(51.5, -0.1).is_empty());
    }

    #[test]
    fn test_from_geojson_minimal() {
        let geojson = r#"{
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": {
                    "designator": "TEST-P1",
                    "name": "Test Prohibited Zone",
                    "type": "PROHIBITED",
                    "country": "IL",
                    "by_notam": false
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[34.0, 31.0], [35.0, 31.0], [35.0, 32.0], [34.0, 32.0], [34.0, 31.0]]]
                }
            }]
        }"#;

        let idx = AirspaceIndex::from_geojson(geojson);
        assert_eq!(idx.zone_count(), 1);

        // Point inside the zone
        let hits = idx.check_position(31.5, 34.5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].designator, "TEST-P1");
        assert_eq!(hits[0].zone_type, AirspaceZoneType::Prohibited);

        // Point outside the zone
        let hits = idx.check_position(30.0, 34.5);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_notam_registration_and_check() {
        let idx = AirspaceIndex::empty();
        idx.register_notam(ActiveNotam {
            notam_id: "A0001/26".to_string(),
            center_lat: 51.5,
            center_lon: -0.1,
            radius_nm: 10.0,
            title: "Test NOTAM".to_string(),
            fir: "EGTT".to_string(),
            expires_at: None,
            ingested_at: Utc::now(),
        });

        assert_eq!(idx.active_notam_count(), 1);

        // Point within radius
        let hits = idx.check_notams(51.5, -0.1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].notam_id, "A0001/26");

        // Point far away
        let hits = idx.check_notams(40.0, 10.0);
        assert!(hits.is_empty());
    }
}
