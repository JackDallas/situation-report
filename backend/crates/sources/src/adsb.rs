//! Generic ADS-B aggregator for readsb-compatible flight tracking services.
//!
//! All supported services (AirplanesLive, adsb.lol, adsb.fi) use the identical
//! readsb JSON response format. This module provides a parameterized
//! [`AdsbAggregator`] struct with convenience constructors for each service.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use std::future::Future;
use std::pin::Pin;

use crate::rate_limit::RateLimited;

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use crate::aircraft_db::AircraftDb;
use crate::common::{callsign_country, icao_hex_country};
use crate::{DataSource, InsertableEvent, SourceContext};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// User-Agent sent with every request to avoid default-UA blocks.
const USER_AGENT: &str = "SituationReport/1.0 (military-aviation-monitor)";

/// ICAO type codes for high-value military platforms.
const HIGH_VALUE_TYPES: &[&str] = &[
    "C135", "K35R", "KC10", "KC46", // Tankers
    "E3", "E6", "E8",               // C2/ISR
    "P8",                            // Maritime patrol
    "RQ4", "MQ9",                    // UAVs
    "C17", "C5",                     // Airlifters
    "B52", "B1", "B2",              // Bombers
    "F35", "F22",                    // Fighters
];

/// Callsign prefixes associated with military / government flights.
const MILITARY_CALLSIGN_PREFIXES: &[&str] = &[
    "REACH", "RCH",    // US Air Mobility Command
    "JAKE",            // P-8 Poseidon
    "ETHYL",           // US military
    "NCHO",            // NATO
    "FORTE",           // RQ-4 Global Hawk
    "HOMER",           // US military
    "SENTRY",          // E-3 AWACS
    "GORDO",           // US military
    "IAF",             // Israeli Air Force
    "EVAC",            // Military evacuation
    "NATO",            // NATO
    "TOPPS",           // US military
    "LAGR",            // US military C-17
    "VIPER",           // Various military
    "DUKE",            // NATO ISR
];

/// Regional point queries for global coverage: (name, lat, lon, radius_nm)
/// Each circle covers ~250nm radius. Overlapping circles ensure no gaps.
const POINT_QUERIES: &[(&str, f64, f64, u32)] = &[
    // Original Middle East / conflict zones
    ("iran", 32.5, 53.0, 250),
    ("levant", 33.0, 36.0, 250),
    ("persian_gulf", 26.5, 52.0, 250),
    ("red_sea_yemen", 15.0, 43.0, 250),
    // Europe
    ("western_europe", 48.0, 2.0, 250),
    ("eastern_europe", 48.0, 35.0, 250),
    // Asia
    ("east_asia", 35.0, 125.0, 250),
    ("southeast_asia", 5.0, 110.0, 250),
    ("south_asia", 20.0, 78.0, 250),
    ("central_asia", 42.0, 68.0, 250),
    // Africa
    ("north_africa", 30.0, 10.0, 250),
    ("east_africa", 0.0, 38.0, 250),
    // Northern / Atlantic
    ("arctic_nordic", 65.0, 15.0, 250),
    ("north_atlantic", 50.0, -30.0, 250),
    // Pacific / Oceania
    ("pacific", 20.0, 150.0, 250),
    ("australia", -25.0, 135.0, 250),
    // Americas
    ("south_america", -15.0, -50.0, 250),
    ("north_america", 38.0, -97.0, 250),
];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a callsign matches known military patterns.
pub(crate) fn is_military_callsign(callsign: &str) -> bool {
    let cs = callsign.trim().to_uppercase();
    if cs.is_empty() {
        return false;
    }
    MILITARY_CALLSIGN_PREFIXES
        .iter()
        .any(|prefix| cs.starts_with(prefix))
}

/// Check if an ICAO type code is a high-value military platform.
pub(crate) fn is_high_value_type(type_code: &str) -> bool {
    let tc = type_code.trim().to_uppercase();
    HIGH_VALUE_TYPES.iter().any(|t| tc == *t)
}

/// Convert a single aircraft JSON object from the readsb `ac` array
/// into an `InsertableEvent`.
///
/// When `aircraft_db` is provided, the ICAO hex code is looked up in the
/// Bellingcat modes.csv database to get authoritative registration, category,
/// military flag, and owner information — replacing the unreliable
/// callsign-prefix and dbFlags heuristics.
pub(crate) fn aircraft_to_event(
    ac: &serde_json::Value,
    source_type: SourceType,
    source_label: &str,
    region: Option<&str>,
    aircraft_db: Option<&AircraftDb>,
) -> Option<InsertableEvent> {
    // Require at least lat/lon to be useful
    let lat = ac.get("lat").and_then(|v| v.as_f64())?;
    let lon = ac.get("lon").and_then(|v| v.as_f64())?;

    let hex = ac.get("hex").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let callsign = ac.get("flight").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let alt_baro = ac.get("alt_baro").and_then(|v| {
        // alt_baro can be a number or the string "ground"
        v.as_f64().or_else(|| {
            v.as_str().and_then(|s| if s == "ground" { Some(0.0) } else { None })
        })
    });
    let alt_geom = ac.get("alt_geom").and_then(|v| v.as_f64());
    let ground_speed = ac.get("gs").and_then(|v| v.as_f64());
    let track = ac.get("track").and_then(|v| v.as_f64());
    let baro_rate = ac.get("baro_rate").and_then(|v| v.as_f64());
    let squawk = ac.get("squawk").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let category = ac.get("category").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let type_code = ac.get("t").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let registration = ac.get("r").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let db_flags = ac.get("dbFlags").and_then(|v| v.as_u64()).unwrap_or(0);
    let emergency = ac.get("emergency").and_then(|v| v.as_str()).unwrap_or("none").to_string();

    // Look up ICAO hex in the Bellingcat aircraft database
    let db_entry = aircraft_db.and_then(|db| db.lookup(&hex));

    // Military flag: Bellingcat DB is authoritative, fall back to API dbFlags / callsign heuristic
    let military = if let Some(entry) = db_entry {
        entry.military
    } else {
        (db_flags & 1) != 0 || is_military_callsign(&callsign)
    };

    // Type code: prefer API (live data), fall back to DB
    let effective_typecode = if !type_code.is_empty() {
        type_code.clone()
    } else {
        db_entry.and_then(|e| e.typecode.clone()).unwrap_or_default()
    };

    let high_value = is_high_value_type(&effective_typecode);

    // Registration: prefer API (live), fall back to DB
    let effective_registration = if !registration.is_empty() {
        registration.clone()
    } else {
        db_entry.and_then(|e| e.registration.clone()).unwrap_or_default()
    };

    // Category: prefer DB (curated by Bellingcat), fall back to API
    let effective_category = if let Some(entry) = db_entry {
        entry.category.clone()
    } else {
        category.clone()
    };

    // Owner from DB (not available in API)
    let owner = db_entry.and_then(|e| e.owner.clone());

    // Determine affiliation from callsign prefix, falling back to ICAO hex range
    let affiliation = callsign_country(&callsign)
        .or_else(|| icao_hex_country(&hex));

    let altitude = alt_geom.or(alt_baro);

    let mut data = json!({
        "source": source_label,
        "hex": hex,
        "callsign": callsign,
        "lat": lat,
        "lon": lon,
        "altitude": altitude,
        "alt_baro": alt_baro,
        "alt_geom": alt_geom,
        "ground_speed": ground_speed,
        "track": track,
        "baro_rate": baro_rate,
        "squawk": squawk,
        "category": effective_category,
        "type_code": effective_typecode,
        "registration": effective_registration,
        "db_flags": db_flags,
        "emergency": emergency,
        "military": military,
        "high_value": high_value,
        "affiliation": affiliation,
    });

    if let Some(ref o) = owner {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("owner".to_string(), json!(o));
        }
    }

    if let Some(r) = region {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("region".to_string(), json!(r));
        }
    }

    // Flag whether this record was enriched by the Bellingcat DB
    if db_entry.is_some() {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("db_matched".to_string(), json!(true));
        }
    }

    let severity = if military { Severity::Medium } else { Severity::Low };
    let mut tags = Vec::new();
    if military {
        tags.push("military".to_string());
    }
    if high_value {
        tags.push("high_value".to_string());
    }
    if let Some(country) = affiliation {
        tags.push(format!("affiliation:{}", country));
    }
    if !effective_typecode.is_empty() {
        tags.push(format!("type:{}", effective_typecode));
    }
    if let Some(ref entry) = db_entry {
        tags.push(format!("aircraft_category:{}", entry.category));
    }

    let title = if !callsign.is_empty() {
        Some(format!("Flight {} ({})", callsign, hex))
    } else {
        None
    };

    // entity_name: prefer registration (authoritative identifier) over callsign
    let entity_name = if !effective_registration.is_empty() {
        if !callsign.is_empty() {
            Some(format!("{} ({})", effective_registration, callsign))
        } else {
            Some(effective_registration.clone())
        }
    } else if !callsign.is_empty() {
        Some(callsign.clone())
    } else {
        None
    };

    Some(InsertableEvent {
        event_time: Utc::now(),
        source_type,
        source_id: Some(source_type.as_str().to_string()),
        longitude: Some(lon),
        latitude: Some(lat),
        region_code: region.map(String::from),
        entity_id: if hex.is_empty() { None } else { Some(hex.clone()) },
        entity_name,
        event_type: EventType::FlightPosition,
        severity,
        confidence: None,
        tags,
        title,
        description: None,
        payload: data,
        heading: track.map(|v| v as f32),
        speed: ground_speed.map(|v| v as f32),
        altitude: altitude.map(|v| v as f32),
    })
}

/// Parse a readsb response JSON and convert each aircraft to an event.
pub(crate) fn parse_aircraft_response(
    body: &serde_json::Value,
    source_type: SourceType,
    source_label: &str,
    region: Option<&str>,
    aircraft_db: Option<&AircraftDb>,
) -> Vec<InsertableEvent> {
    let ac_array = match body.get("ac").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    ac_array
        .iter()
        .filter_map(|ac| aircraft_to_event(ac, source_type, source_label, region, aircraft_db))
        .collect()
}

// ===========================================================================
// AdsbAggregator -- parameterized ADS-B source
// ===========================================================================

/// A generic ADS-B flight tracking data source that works with any
/// readsb-compatible aggregator (AirplanesLive, adsb.lol, adsb.fi, etc.).
pub struct AdsbAggregator {
    source_id: &'static str,
    display_name: &'static str,
    base_url: &'static str,
    failover_url: Option<&'static str>,
    source_type: SourceType,
    min_request_gap: Duration,
    poll_interval: Duration,
    military_path: &'static str,
    point_path_template: &'static str,
    squawk_path_template: &'static str,
    /// For services (like adsb.fi) that use a different base URL for geo queries.
    point_url_base: Option<&'static str>,
    source_label: &'static str,
    point_index: AtomicUsize,
    last_request: Mutex<tokio::time::Instant>,
    /// Self-managed rate-limit cooldown. When set, `poll()` returns `Ok(vec![])`
    /// until the deadline passes, preventing the registry from stacking additive
    /// backoff on top of the API's Retry-After delay.
    rate_limit_until: std::sync::Mutex<Option<tokio::time::Instant>>,
    /// Optional Bellingcat aircraft database for ICAO hex lookups.
    aircraft_db: Option<Arc<AircraftDb>>,
}

/// Rate-limit a request, enforcing minimum gap between consecutive HTTP calls.
/// On non-429 failure, tries the failover URL if configured.
async fn rate_limited_get(
    ctx: &SourceContext,
    url: &str,
    last_request: &Mutex<tokio::time::Instant>,
    min_gap: Duration,
    source_id: &str,
    failover_base: Option<&str>,
    original_base: &str,
) -> Result<reqwest::Response, anyhow::Error> {
    // Enforce minimum gap between requests
    {
        let mut last = last_request.lock().await;
        let elapsed = last.elapsed();
        if elapsed < min_gap {
            tokio::time::sleep(min_gap - elapsed).await;
        }
        *last = tokio::time::Instant::now();
    }

    let resp = ctx
        .http
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await;

    match resp {
        Ok(r) => {
            match crate::rate_limit::check_rate_limit(r, source_id) {
                Ok(r) => Ok(r),
                Err(e) => {
                    // On 429, propagate directly (no failover for rate limits)
                    if e.downcast_ref::<crate::rate_limit::RateLimited>().is_some() {
                        return Err(e);
                    }
                    // For other HTTP errors, try failover
                    if let Some(failover) = failover_base {
                        let failover_url = url.replacen(original_base, failover, 1);
                        warn!(source = source_id, failover = %failover_url, "Primary failed, trying failover");
                        let r2 = ctx
                            .http
                            .get(&failover_url)
                            .header(reqwest::header::USER_AGENT, USER_AGENT)
                            .send()
                            .await?;
                        crate::rate_limit::check_rate_limit(r2, source_id)
                    } else {
                        Err(e)
                    }
                }
            }
        }
        Err(e) => {
            // Network-level error — try failover
            if let Some(failover) = failover_base {
                let failover_url = url.replacen(original_base, failover, 1);
                warn!(source = source_id, failover = %failover_url, "Primary failed, trying failover");
                let r2 = ctx
                    .http
                    .get(&failover_url)
                    .header(reqwest::header::USER_AGENT, USER_AGENT)
                    .send()
                    .await?;
                crate::rate_limit::check_rate_limit(r2, source_id)
            } else {
                Err(e.into())
            }
        }
    }
}

impl AdsbAggregator {
    /// Build the URL for a point/geo query given lat, lon, radius.
    fn point_url(&self, lat: f64, lon: f64, radius: u32) -> String {
        let base = self.point_url_base.unwrap_or(self.base_url);
        let path = self
            .point_path_template
            .replace("{lat}", &lat.to_string())
            .replace("{lon}", &lon.to_string())
            .replace("{radius}", &radius.to_string());
        format!("{}{}", base, path)
    }

    /// Build the URL for a squawk query.
    fn squawk_url(&self, code: &str) -> String {
        let path = self.squawk_path_template.replace("{code}", code);
        format!("{}{}", self.base_url, path)
    }

    /// Build the URL for the military endpoint.
    fn military_url(&self) -> String {
        format!("{}{}", self.base_url, self.military_path)
    }

    /// Set the rate-limit cooldown timer. While active, `poll()` short-circuits
    /// with `Ok(vec![])` so the registry never sees an error and never escalates
    /// its own backoff.
    fn set_rate_limit_cooldown(&self, retry_after: Duration) {
        let deadline = tokio::time::Instant::now() + retry_after;
        if let Ok(mut guard) = self.rate_limit_until.lock() {
            *guard = Some(deadline);
        }
    }

    /// Returns `true` if we are still inside a rate-limit cooldown window.
    fn in_rate_limit_cooldown(&self) -> bool {
        if let Ok(guard) = self.rate_limit_until.lock() {
            if let Some(deadline) = *guard {
                return tokio::time::Instant::now() < deadline;
            }
        }
        false
    }

    /// Perform a rate-limited GET with failover support.
    async fn get(
        &self,
        ctx: &SourceContext,
        url: &str,
    ) -> Result<reqwest::Response, anyhow::Error> {
        rate_limited_get(
            ctx,
            url,
            &self.last_request,
            self.min_request_gap,
            self.source_id,
            self.failover_url,
            self.base_url,
        )
        .await
    }
}

impl DataSource for AdsbAggregator {
    fn id(&self) -> &str {
        self.source_id
    }

    fn name(&self) -> &str {
        self.display_name
    }

    fn default_interval(&self) -> Duration {
        self.poll_interval
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        // ---- 0. Rate-limit cooldown check ----
        if self.in_rate_limit_cooldown() {
            debug!(source = self.source_id, "Rate-limit cooldown active, skipping poll");
            return Ok(vec![]);
        }

        // ---- 1. Fetch all military aircraft (priority) ----
        let mil_url = self.military_url();
        debug!(source = self.source_id, "Polling military endpoint");

        // Military endpoint is the primary request.
        // On 429: set self-managed cooldown and return Ok(vec![]) so the
        // registry's consecutive_failures counter stays at 0.
        let resp = match self.get(ctx, &mil_url).await {
            Ok(r) => r,
            Err(e) => {
                if let Some(rl) = e.downcast_ref::<RateLimited>() {
                    warn!(
                        source = self.source_id,
                        retry_after_secs = rl.retry_after.as_secs(),
                        "Military endpoint rate-limited, entering self-managed cooldown"
                    );
                    self.set_rate_limit_cooldown(rl.retry_after);
                    return Ok(vec![]);
                }
                return Err(e);
            }
        };

        let body: serde_json::Value = resp.json().await?;
        let db_ref = self.aircraft_db.as_deref();
        let mut events =
            parse_aircraft_response(&body, self.source_type, self.source_label, None, db_ref);

        let mil_count = events.len();

        // ---- 2. Fetch one rotated regional point query ----
        let idx = self.point_index.fetch_add(1, Ordering::Relaxed) % POINT_QUERIES.len();
        let (region, lat, lon, radius) = POINT_QUERIES[idx];

        debug!(source = self.source_id, region, lat, lon, radius, "Polling point query");

        let point_url = self.point_url(lat, lon, radius);

        // Sub-requests: on 429, set cooldown and return partial results
        match self.get(ctx, &point_url).await {
            Ok(resp) => {
                let point_body: serde_json::Value = resp.json().await?;
                let regional_events = parse_aircraft_response(
                    &point_body,
                    self.source_type,
                    self.source_label,
                    Some(region),
                    db_ref,
                );

                let existing_hexes: std::collections::HashSet<String> = events
                    .iter()
                    .filter_map(|e| e.entity_id.clone())
                    .collect();

                let mut regional_count = 0usize;
                for evt in regional_events {
                    let hex = evt.entity_id.as_deref().unwrap_or("");
                    if !existing_hexes.contains(hex) {
                        regional_count += 1;
                        events.push(evt);
                    }
                }

                if regional_count > 0 {
                    debug!(source = self.source_id, region, count = regional_count, "Regional aircraft added");
                }
            }
            Err(e) => {
                if let Some(rl) = e.downcast_ref::<RateLimited>() {
                    warn!(source = self.source_id, region, retry_after_secs = rl.retry_after.as_secs(),
                        "Regional query rate-limited, setting cooldown");
                    self.set_rate_limit_cooldown(rl.retry_after);
                } else {
                    warn!(source = self.source_id, region, error = %e, "Regional query failed, returning military data only");
                }
            }
        }

        // ---- 3. Fetch squawk 7700 (emergency) ----
        let emergency_url = self.squawk_url("7700");
        debug!(source = self.source_id, "Polling emergency squawk 7700");

        match self.get(ctx, &emergency_url).await {
            Ok(resp) => {
                let emergency_body: serde_json::Value = resp.json().await?;
                let emergency_events = parse_aircraft_response(
                    &emergency_body,
                    self.source_type,
                    self.source_label,
                    None,
                    db_ref,
                );

                let existing_hexes: std::collections::HashSet<String> = events
                    .iter()
                    .filter_map(|e| e.entity_id.clone())
                    .collect();

                let mut emergency_count = 0usize;
                for evt in emergency_events {
                    let hex = evt.entity_id.as_deref().unwrap_or("");
                    if !existing_hexes.contains(hex) {
                        emergency_count += 1;
                        events.push(evt);
                    }
                }

                if emergency_count > 0 {
                    info!(source = self.source_id, count = emergency_count, "Emergency (7700) aircraft detected");
                }
            }
            Err(e) => {
                if let Some(rl) = e.downcast_ref::<RateLimited>() {
                    warn!(source = self.source_id, retry_after_secs = rl.retry_after.as_secs(),
                        "Emergency query rate-limited, setting cooldown");
                    self.set_rate_limit_cooldown(rl.retry_after);
                } else {
                    warn!(source = self.source_id, error = %e, "Emergency query failed, skipping");
                }
            }
        }

        if !events.is_empty() {
            debug!(source = self.source_id, military = mil_count, total = events.len(), "Aircraft tracked");
        }

        Ok(events)
        })
    }
}

// ===========================================================================
// Convenience constructors
// ===========================================================================

/// Create an [`AdsbAggregator`] for the AirplanesLive service.
///
/// - Primary: `https://api.airplanes.live/v2`
/// - Failover: `https://api.adsb.one/v2`
pub fn airplaneslive(aircraft_db: Option<Arc<AircraftDb>>) -> AdsbAggregator {
    AdsbAggregator {
        source_id: "airplaneslive",
        display_name: "Airplanes.live",
        base_url: "https://api.airplanes.live/v2",
        failover_url: Some("https://api.adsb.one/v2"),
        source_type: SourceType::AirplanesLive,
        min_request_gap: Duration::from_millis(3000),
        poll_interval: Duration::from_secs(120),
        military_path: "/mil",
        point_path_template: "/point/{lat}/{lon}/{radius}",
        squawk_path_template: "/sqk/{code}",
        point_url_base: None,
        source_label: "airplaneslive",
        point_index: AtomicUsize::new(0),
        last_request: Mutex::new(tokio::time::Instant::now()),
        rate_limit_until: std::sync::Mutex::new(None),
        aircraft_db,
    }
}

/// Create an [`AdsbAggregator`] for the adsb.lol service.
///
/// - Primary: `https://api.adsb.lol/v2`
pub fn adsb_lol(aircraft_db: Option<Arc<AircraftDb>>) -> AdsbAggregator {
    AdsbAggregator {
        source_id: "adsb-lol",
        display_name: "adsb.lol",
        base_url: "https://api.adsb.lol/v2",
        failover_url: None,
        source_type: SourceType::AdsbLol,
        min_request_gap: Duration::from_millis(3000),
        poll_interval: Duration::from_secs(120),
        military_path: "/mil",
        point_path_template: "/point/{lat}/{lon}/{radius}",
        squawk_path_template: "/sqk/{code}",
        point_url_base: None,
        source_label: "adsb-lol",
        // Offset from AirplanesLive to spread regional queries
        point_index: AtomicUsize::new(3),
        last_request: Mutex::new(tokio::time::Instant::now()),
        rate_limit_until: std::sync::Mutex::new(None),
        aircraft_db,
    }
}

/// Create an [`AdsbAggregator`] for the adsb.fi service.
///
/// - Primary: `https://opendata.adsb.fi/api/v2`
/// - Geo queries use v3 endpoint: `https://opendata.adsb.fi/api/v3`
pub fn adsb_fi(aircraft_db: Option<Arc<AircraftDb>>) -> AdsbAggregator {
    AdsbAggregator {
        source_id: "adsb-fi",
        display_name: "adsb.fi",
        base_url: "https://opendata.adsb.fi/api/v2",
        failover_url: None,
        source_type: SourceType::AdsbFi,
        min_request_gap: Duration::from_millis(3000),
        poll_interval: Duration::from_secs(120),
        military_path: "/mil",
        // adsb.fi uses a different path pattern for geo queries
        point_path_template: "/lat/{lat}/lon/{lon}/dist/{radius}",
        squawk_path_template: "/sqk/{code}",
        // v3 endpoint for geo queries
        point_url_base: Some("https://opendata.adsb.fi/api/v3"),
        source_label: "adsb-fi",
        // Offset from other sources to spread regional queries
        point_index: AtomicUsize::new(6),
        last_request: Mutex::new(tokio::time::Instant::now()),
        rate_limit_until: std::sync::Mutex::new(None),
        aircraft_db,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_military_callsign_detection() {
        assert!(is_military_callsign("REACH123"));
        assert!(is_military_callsign("RCH456"));
        assert!(is_military_callsign("FORTE11"));
        assert!(is_military_callsign("JAKE21"));
        assert!(is_military_callsign("ETHYL01"));
        assert!(is_military_callsign("NCHO01"));
        assert!(is_military_callsign("HOMER01"));
        assert!(is_military_callsign("SENTRY01"));
        assert!(is_military_callsign("GORDO01"));
        assert!(is_military_callsign("IAF001"));
        assert!(is_military_callsign("EVAC01"));
        assert!(is_military_callsign("NATO01"));
        assert!(is_military_callsign("TOPPS01"));
        assert!(is_military_callsign("LAGR01"));
        assert!(is_military_callsign("VIPER01"));
        assert!(is_military_callsign("DUKE01"));
        assert!(!is_military_callsign("UAL123"));
        assert!(!is_military_callsign("BAW456"));
        assert!(!is_military_callsign(""));
    }

    #[test]
    fn test_high_value_type_detection() {
        assert!(is_high_value_type("C135"));
        assert!(is_high_value_type("K35R"));
        assert!(is_high_value_type("KC10"));
        assert!(is_high_value_type("KC46"));
        assert!(is_high_value_type("E3"));
        assert!(is_high_value_type("E6"));
        assert!(is_high_value_type("E8"));
        assert!(is_high_value_type("P8"));
        assert!(is_high_value_type("RQ4"));
        assert!(is_high_value_type("MQ9"));
        assert!(is_high_value_type("C17"));
        assert!(is_high_value_type("C5"));
        assert!(is_high_value_type("B52"));
        assert!(is_high_value_type("B1"));
        assert!(is_high_value_type("B2"));
        assert!(is_high_value_type("F35"));
        assert!(is_high_value_type("F22"));
        // Case-insensitive
        assert!(is_high_value_type("c135"));
        assert!(is_high_value_type("f22"));
        // Non-military types
        assert!(!is_high_value_type("B738"));
        assert!(!is_high_value_type("A320"));
        assert!(!is_high_value_type(""));
    }

    #[test]
    fn test_aircraft_to_event_airplaneslive() {
        let ac = json!({
            "hex": "ae1234",
            "flight": "FORTE11 ",
            "lat": 35.5,
            "lon": 51.3,
            "alt_baro": 55000,
            "alt_geom": 55100,
            "gs": 350.0,
            "track": 90.0,
            "baro_rate": 0,
            "squawk": "4567",
            "category": "A5",
            "t": "RQ4",
            "r": "11-2047",
            "dbFlags": 1,
            "emergency": "none",
        });

        let event = aircraft_to_event(&ac, SourceType::AirplanesLive, "airplaneslive", None, None);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.source_type, SourceType::AirplanesLive);
        assert_eq!(event.entity_id.as_deref(), Some("ae1234"));
        assert_eq!(event.entity_name.as_deref(), Some("11-2047 (FORTE11)"));
        assert!(event.tags.contains(&"military".to_string()));
        assert!(event.tags.contains(&"high_value".to_string()));
        assert_eq!(event.payload["type_code"], "RQ4");
        assert_eq!(event.payload["registration"], "11-2047");
        assert_eq!(event.payload["source"], "airplaneslive");
    }

    #[test]
    fn test_aircraft_to_event_adsb_lol() {
        let ac = json!({
            "hex": "ae5678",
            "flight": "RCH999",
            "lat": 33.0,
            "lon": 44.0,
            "alt_baro": 35000,
            "dbFlags": 1,
        });

        let event = aircraft_to_event(&ac, SourceType::AdsbLol, "adsb-lol", Some("levant"), None);
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.source_type, SourceType::AdsbLol);
        assert_eq!(event.payload["source"], "adsb-lol");
        assert_eq!(event.region_code.as_deref(), Some("levant"));
    }

    #[test]
    fn test_aircraft_to_event_adsb_fi() {
        let ac = json!({
            "hex": "abcdef",
            "flight": "SENTRY01",
            "lat": 26.5,
            "lon": 52.0,
            "alt_baro": 30000,
            "t": "E3",
            "dbFlags": 1,
        });

        let event = aircraft_to_event(&ac, SourceType::AdsbFi, "adsb-fi", None, None);
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.source_type, SourceType::AdsbFi);
        assert_eq!(event.payload["source"], "adsb-fi");
        assert!(event.tags.contains(&"military".to_string()));
        assert!(event.tags.contains(&"high_value".to_string()));
    }

    #[test]
    fn test_aircraft_without_position_skipped() {
        let ac = json!({
            "hex": "ae1234",
            "flight": "TEST01",
        });
        assert!(aircraft_to_event(&ac, SourceType::AirplanesLive, "airplaneslive", None, None).is_none());
    }

    #[test]
    fn test_alt_baro_ground_string() {
        let ac = json!({
            "hex": "ae1234",
            "flight": "TEST01",
            "lat": 35.0,
            "lon": 51.0,
            "alt_baro": "ground",
        });
        let event = aircraft_to_event(&ac, SourceType::AirplanesLive, "airplaneslive", None, None);
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.payload["alt_baro"], 0.0);
    }

    #[test]
    fn test_db_flags_military_bit() {
        // dbFlags bit 0 = military
        let ac = json!({
            "hex": "ae1234",
            "flight": "RANDOM01",
            "lat": 35.0,
            "lon": 51.0,
            "dbFlags": 1,
        });
        let event = aircraft_to_event(&ac, SourceType::AirplanesLive, "airplaneslive", None, None).unwrap();
        assert_eq!(event.payload["military"], true);
        assert!(event.tags.contains(&"military".to_string()));

        // No military flag
        let ac2 = json!({
            "hex": "ae5678",
            "flight": "UAL123",
            "lat": 35.0,
            "lon": 51.0,
            "dbFlags": 0,
        });
        let event2 = aircraft_to_event(&ac2, SourceType::AirplanesLive, "airplaneslive", None, None).unwrap();
        assert_eq!(event2.payload["military"], false);
        assert!(!event2.tags.contains(&"military".to_string()));
    }

    #[test]
    fn test_region_rotation() {
        let source = airplaneslive(None);
        // Verify cycling through all regions and wrapping
        for expected in 0..POINT_QUERIES.len() {
            let idx = source.point_index.fetch_add(1, Ordering::Relaxed) % POINT_QUERIES.len();
            assert_eq!(idx, expected);
        }
        // Wraps around
        let idx = source.point_index.fetch_add(1, Ordering::Relaxed) % POINT_QUERIES.len();
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_parse_aircraft_response() {
        let body = json!({
            "ac": [
                {
                    "hex": "ae1234",
                    "flight": "FORTE11",
                    "lat": 35.5,
                    "lon": 51.3,
                    "alt_baro": 55000,
                    "t": "RQ4",
                    "dbFlags": 1,
                },
                {
                    "hex": "ae5678",
                    "flight": "UAL123",
                    "lat": 36.0,
                    "lon": 52.0,
                    "alt_baro": 35000,
                    "dbFlags": 0,
                },
                {
                    // No position -- should be skipped
                    "hex": "ae9999",
                    "flight": "NOPOS",
                }
            ],
            "now": 1234567890,
            "total": 3,
            "ctime": 1234567890,
            "ptime": 50,
        });

        let events = parse_aircraft_response(&body, SourceType::AirplanesLive, "airplaneslive", Some("iran"), None);
        assert_eq!(events.len(), 2);

        // Verify region is set
        assert_eq!(events[0].region_code.as_deref(), Some("iran"));
    }

    #[test]
    fn test_point_url_normal() {
        let agg = airplaneslive(None);
        let url = agg.point_url(32.5, 53.0, 250);
        assert_eq!(url, "https://api.airplanes.live/v2/point/32.5/53/250");
    }

    #[test]
    fn test_point_url_adsb_fi_uses_v3() {
        let agg = adsb_fi(None);
        let url = agg.point_url(32.5, 53.0, 250);
        assert_eq!(url, "https://opendata.adsb.fi/api/v3/lat/32.5/lon/53/dist/250");
    }

    #[test]
    fn test_squawk_url() {
        let agg = airplaneslive(None);
        let url = agg.squawk_url("7700");
        assert_eq!(url, "https://api.airplanes.live/v2/sqk/7700");
    }

    #[test]
    fn test_military_url() {
        let agg = adsb_lol(None);
        let url = agg.military_url();
        assert_eq!(url, "https://api.adsb.lol/v2/mil");
    }

    #[test]
    fn test_adsb_lol_point_index_offset() {
        let agg = adsb_lol(None);
        // Should start at index 3
        let idx = agg.point_index.load(Ordering::Relaxed);
        assert_eq!(idx, 3);
    }

    #[test]
    fn test_adsb_fi_point_index_offset() {
        let agg = adsb_fi(None);
        // Should start at index 6
        let idx = agg.point_index.load(Ordering::Relaxed);
        assert_eq!(idx, 6);
    }

    #[test]
    fn test_is_flight_source() {
        assert!(SourceType::AirplanesLive.is_flight_source());
        assert!(SourceType::AdsbLol.is_flight_source());
        assert!(SourceType::AdsbFi.is_flight_source());
        assert!(SourceType::Opensky.is_flight_source());
        assert!(!SourceType::Gdelt.is_flight_source());
        assert!(!SourceType::Shodan.is_flight_source());
    }
}
