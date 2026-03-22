use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

/// Bounding box definition: (name, lamin, lomin, lamax, lomax)
/// Expanded to ~10 regions for global coverage while respecting credit limits.
/// Each region is kept moderately sized to avoid exceeding per-query data caps.
const BOUNDING_BOXES: &[(&str, f64, f64, f64, f64)] = &[
    // Original conflict zones
    ("middle_east", 25.0, 34.0, 42.0, 63.0),
    ("ukraine", 44.0, 22.0, 53.0, 40.0),
    ("persian_gulf", 22.0, 46.0, 31.0, 57.0),
    ("red_sea_yemen", 10.0, 38.0, 20.0, 48.0),
    // Europe
    ("western_europe", 43.0, -10.0, 55.0, 15.0),
    // Asia
    ("east_asia", 25.0, 115.0, 45.0, 145.0),
    ("southeast_asia", -10.0, 95.0, 20.0, 125.0),
    // Africa
    ("north_africa", 20.0, -5.0, 37.0, 35.0),
    // Americas
    ("south_america", -35.0, -75.0, 5.0, -35.0),
    ("north_america", 25.0, -125.0, 50.0, -65.0),
];

/// Known military callsign prefixes.
const MILITARY_PREFIXES: &[&str] = &[
    "RCH",    // US Air Force (Air Mobility Command)
    "REACH",  // US Air Force (Air Mobility Command, alternate)
    "DUKE",   // NATO ISR
    "FORTE",  // RQ-4 Global Hawk
    "JAKE",   // P-8 Poseidon
    "VIPER",  // Various military
    "EVAC",   // Military evacuation
    "TOPPS",  // US military
    "NCHO",   // NATO
    "NATO",   // NATO
    "RRR",    // US Air Force refueling
    "HOMER",  // US military
    "LAGR",   // US military C-17
    "ETHYL",  // US military (KC-135 tanker)
    "SENTRY", // AWACS / E-3 Sentry
    "GORDO",  // US military transport
    "IAF",    // Israeli Air Force
];

/// OAuth2 token endpoint for OpenSky Network (Keycloak).
const TOKEN_URL: &str = "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token";

/// Safety margin (in seconds) to refresh the token before it actually expires.
const TOKEN_EXPIRY_MARGIN_SECS: u64 = 30;

/// Response from the OAuth2 token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// Cached OAuth2 bearer token with expiry tracking.
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

pub struct OpenSkySource {
    bbox_index: AtomicUsize,
    /// Cached OAuth2 token, behind an async mutex for interior mutability.
    token_cache: Mutex<Option<CachedToken>>,
}

impl OpenSkySource {
    pub fn new() -> Self {
        Self {
            bbox_index: AtomicUsize::new(0),
            token_cache: Mutex::new(None),
        }
    }

    /// Check if a callsign indicates a military aircraft.
    fn is_military_callsign(callsign: &str) -> bool {
        let cs = callsign.trim().to_uppercase();
        MILITARY_PREFIXES
            .iter()
            .any(|prefix| cs.starts_with(prefix))
    }

    /// Obtain a valid OAuth2 bearer token, refreshing if expired or absent.
    /// Returns `None` if client credentials are not configured.
    async fn get_bearer_token(
        &self,
        http: &reqwest::Client,
    ) -> anyhow::Result<Option<String>> {
        let client_id = match std::env::var("OPENSKY_CLIENT_ID") {
            Ok(v) if !v.is_empty() => v,
            _ => return Ok(None),
        };
        let client_secret = match std::env::var("OPENSKY_CLIENT_SECRET") {
            Ok(v) if !v.is_empty() => v,
            _ => return Ok(None),
        };

        let mut cache = self.token_cache.lock().await;

        // Return cached token if still valid
        if let Some(ref cached) = *cache {
            if Instant::now() < cached.expires_at {
                return Ok(Some(cached.access_token.clone()));
            }
            debug!("OpenSky OAuth2 token expired, refreshing");
        }

        // Request a new token via client credentials grant
        let resp = http
            .post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=client_credentials&client_id={}&client_secret={}",
                client_id, client_secret
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "OpenSky OAuth2 token request failed {}: {}",
                status,
                body.chars().take(300).collect::<String>()
            );
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_at = Instant::now()
            + Duration::from_secs(
                token_resp
                    .expires_in
                    .saturating_sub(TOKEN_EXPIRY_MARGIN_SECS),
            );

        debug!(
            expires_in = token_resp.expires_in,
            "Obtained new OpenSky OAuth2 token"
        );

        let access_token = token_resp.access_token.clone();
        *cache = Some(CachedToken {
            access_token: token_resp.access_token,
            expires_at,
        });

        Ok(Some(access_token))
    }
}

impl Default for OpenSkySource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for OpenSkySource {
    fn id(&self) -> &str {
        "opensky"
    }

    fn name(&self) -> &str {
        "OpenSky Aircraft"
    }

    fn default_interval(&self) -> Duration {
        // 10 regions at 220s interval: 10 x (86400/220) ≈ 3927 polls/day, within 4000 credit limit.
        // OpenSky serves as secondary/cross-reference source.
        Duration::from_secs(220)
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        // Rotate through bounding boxes
        let idx = self.bbox_index.fetch_add(1, Ordering::Relaxed) % BOUNDING_BOXES.len();
        let (region, lamin, lomin, lamax, lomax) = BOUNDING_BOXES[idx];

        debug!(region, "Polling OpenSky for bounding box");

        let url = format!(
            "https://opensky-network.org/api/states/all?lamin={}&lomin={}&lamax={}&lomax={}",
            lamin, lomin, lamax, lomax
        );

        // Build request with OAuth2 bearer token (if credentials configured)
        let mut req = ctx.http.get(&url);

        match self.get_bearer_token(&ctx.http).await {
            Ok(Some(token)) => {
                req = req.bearer_auth(&token);
            }
            Ok(None) => {
                warn!("OpenSky: no OAuth2 credentials configured, using unauthenticated access");
            }
            Err(e) => {
                warn!("OpenSky: failed to obtain OAuth2 token, falling back to unauthenticated: {e}");
            }
        }

        let resp = req.send().await?;
        let resp = crate::rate_limit::check_rate_limit(resp, "opensky")?;

        let body: serde_json::Value = resp.json().await?;

        let states = match body.get("states").and_then(|s| s.as_array()) {
            Some(states) => states,
            None => {
                debug!(region, "No states in OpenSky response");
                return Ok(Vec::new());
            }
        };

        let mut events: Vec<InsertableEvent> = Vec::with_capacity(states.len());

        for state in states {
            let arr = match state.as_array() {
                Some(a) if a.len() > 14 => a,
                _ => continue,
            };

            // Extract fields by index per OpenSky state vector spec
            let icao24 = arr[0].as_str().unwrap_or("").to_string();
            let callsign = arr[1].as_str().unwrap_or("").trim().to_string();
            let origin_country = arr[2].as_str().unwrap_or("").to_string();
            let longitude = match arr[5].as_f64() {
                Some(v) => v,
                None => continue, // Skip if no position
            };
            let latitude = match arr[6].as_f64() {
                Some(v) => v,
                None => continue,
            };
            let baro_altitude = arr[7].as_f64();
            let on_ground = arr[8].as_bool().unwrap_or(false);
            let velocity = arr[9].as_f64();
            let true_track = arr[10].as_f64();
            let vertical_rate = arr[11].as_f64();
            let geo_altitude = arr[13].as_f64();
            let squawk = arr[14].as_str().map(|s| s.to_string());

            let altitude = geo_altitude.or(baro_altitude);
            let military = Self::is_military_callsign(&callsign);
            let severity = if military { Severity::Medium } else { Severity::Low };

            let mut tags = Vec::new();
            if military {
                tags.push("military".to_string());
            }

            let data = json!({
                "icao24": icao24,
                "callsign": callsign,
                "origin_country": origin_country,
                "lat": latitude,
                "lon": longitude,
                "altitude": altitude,
                "velocity": velocity,
                "heading": true_track,
                "vertical_rate": vertical_rate,
                "on_ground": on_ground,
                "squawk": squawk,
                "military": military,
                "region": region,
            });

            let title = if !callsign.is_empty() {
                Some(format!("Flight {} ({})", callsign, icao24))
            } else {
                None
            };

            events.push(InsertableEvent {
                event_time: Utc::now(),
                source_type: SourceType::Opensky,
                source_id: Some(self.id().to_string()),
                longitude: Some(longitude),
                latitude: Some(latitude),
                region_code: None,
                entity_id: if icao24.is_empty() { None } else { Some(icao24.clone()) },
                entity_name: if callsign.is_empty() { None } else { Some(callsign.clone()) },
                event_type: EventType::FlightPosition,
                severity,
                confidence: None,
                tags,
                title,
                description: None,
                payload: data,
                heading: true_track.map(|v| v as f32),
                speed: velocity.map(|v| v as f32),
                altitude: altitude.map(|v| v as f32),
            });
        }

        if !events.is_empty() {
            debug!(region, count = events.len(), "OpenSky aircraft tracked");
        }

        Ok(events)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_military_detection() {
        assert!(OpenSkySource::is_military_callsign("RCH123"));
        assert!(OpenSkySource::is_military_callsign("FORTE11"));
        assert!(OpenSkySource::is_military_callsign("DUKE01"));
        assert!(OpenSkySource::is_military_callsign("JAKE21"));
        assert!(OpenSkySource::is_military_callsign("VIPER01"));
        assert!(OpenSkySource::is_military_callsign("REACH44"));
        assert!(OpenSkySource::is_military_callsign("ETHYL62"));
        assert!(OpenSkySource::is_military_callsign("SENTRY30"));
        assert!(OpenSkySource::is_military_callsign("GORDO01"));
        assert!(OpenSkySource::is_military_callsign("IAF902"));
        assert!(!OpenSkySource::is_military_callsign("UAL123"));
        assert!(!OpenSkySource::is_military_callsign("BAW456"));
        assert!(!OpenSkySource::is_military_callsign(""));
    }

    #[test]
    fn test_bbox_rotation() {
        let source = OpenSkySource::new();
        for expected in 0..BOUNDING_BOXES.len() {
            let idx = source.bbox_index.fetch_add(1, Ordering::Relaxed) % BOUNDING_BOXES.len();
            assert_eq!(idx, expected);
        }
        // Wraps around
        let idx = source.bbox_index.fetch_add(1, Ordering::Relaxed) % BOUNDING_BOXES.len();
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_default_interval_is_220s() {
        let source = OpenSkySource::new();
        assert_eq!(
            <OpenSkySource as DataSource>::default_interval(&source),
            Duration::from_secs(220)
        );
    }

    #[test]
    fn test_military_flag_in_event() {
        // Just test that is_military_callsign works for military detection
        assert!(OpenSkySource::is_military_callsign("IAF902"));
        assert!(!OpenSkySource::is_military_callsign("UAL123"));
    }
}
