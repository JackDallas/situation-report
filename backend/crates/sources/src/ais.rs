use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, warn};

use chrono::Utc;

/// Interval between keepalive Ping frames sent to the server.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Browser-like User-Agent to avoid Cloudflare bot detection on WebSocket upgrade.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: Duration = Duration::from_secs(30);

/// Maximum backoff delay between reconnection attempts.
/// Set high (30 min) because aisstream.io is under sustained load pressure
/// since early 2026 — be a good citizen and don't hammer them.
const MAX_BACKOFF: Duration = Duration::from_secs(1800);

use sr_types::{EventType, Severity, SourceType};

use crate::common::mmsi_country;
use crate::rate_limit::AuthError;
use crate::{DataSource, InsertableEvent, SourceContext};

/// WebSocket URL for aisstream.io.
const AISSTREAM_URL: &str = "wss://stream.aisstream.io/v0/stream";

/// Bounding boxes for monitored maritime regions.
/// Each entry is (name, [[lat_min, lon_min], [lat_max, lon_max]]).
const BOUNDING_BOXES: &[(&str, [[f64; 2]; 2])] = &[
    ("Strait of Hormuz", [[25.5, 54.0], [27.0, 57.0]]),
    ("Bab-el-Mandeb", [[12.0, 43.0], [13.5, 44.0]]),
    ("Red Sea", [[14.0, 38.0], [22.0, 44.0]]),
    ("Suez Canal", [[29.5, 32.0], [31.5, 33.0]]),
    ("Persian Gulf", [[24.0, 48.0], [30.0, 56.0]]),
    ("Black Sea", [[41.0, 27.0], [46.0, 42.0]]),
];

/// MMSI prefixes known to belong to military/naval vessels.
const MILITARY_MMSI_PREFIXES: &[(&str, &str)] = &[
    ("338", "US Navy"),
    ("369", "US Navy"),
    ("422", "Iran Navy"),
];

/// AIS vessel tracking via aisstream.io WebSocket API.
pub struct AisSource;

impl AisSource {
    pub fn new() -> Self {
        Self
    }

    /// Build the JSON subscribe message with API key and bounding boxes.
    fn build_subscribe_message(api_key: &str) -> String {
        let boxes: Vec<[[f64; 2]; 2]> = BOUNDING_BOXES.iter().map(|(_, bb)| *bb).collect();
        let msg = serde_json::json!({
            "APIKey": api_key,
            "BoundingBoxes": boxes,
            "FilterMessageTypes": ["PositionReport", "ShipStaticData"],
        });
        msg.to_string()
    }

    /// Determine which monitored region a lat/lon falls in.
    /// Returns the first matching region name, or "Unknown" if none match.
    fn determine_region(lat: f64, lon: f64) -> &'static str {
        for (name, [[lat_min, lon_min], [lat_max, lon_max]]) in BOUNDING_BOXES {
            if lat >= *lat_min && lat <= *lat_max && lon >= *lon_min && lon <= *lon_max {
                return name;
            }
        }
        "Unknown"
    }

    /// Check if a vessel MMSI indicates a military/naval vessel.
    fn is_military_mmsi(mmsi: u64) -> bool {
        let mmsi_str = mmsi.to_string();
        MILITARY_MMSI_PREFIXES
            .iter()
            .any(|(prefix, _)| mmsi_str.starts_with(prefix))
    }

    /// Get the military designation for a given MMSI, if any.
    fn military_designation(mmsi: u64) -> Option<&'static str> {
        let mmsi_str = mmsi.to_string();
        MILITARY_MMSI_PREFIXES
            .iter()
            .find(|(prefix, _)| mmsi_str.starts_with(prefix))
            .map(|(_, designation)| *designation)
    }

    /// Convert AIS navigational status code to a human-readable string.
    fn nav_status_string(status: u64) -> &'static str {
        match status {
            0 => "Under way using engine",
            1 => "At anchor",
            2 => "Not under command",
            3 => "Restricted manoeuvrability",
            4 => "Constrained by her draught",
            5 => "Moored",
            6 => "Aground",
            7 => "Engaged in Fishing",
            8 => "Under way sailing",
            9 => "Reserved for HSC",
            10 => "Reserved for WIG",
            11 => "Power-driven vessel towing astern",
            12 => "Power-driven vessel pushing ahead or towing alongside",
            13 => "Reserved",
            14 => "AIS-SART, MOB-AIS, EPIRB-AIS",
            15 => "Undefined / default",
            _ => "Unknown",
        }
    }

    /// Process a PositionReport message and return an InsertableEvent.
    fn process_position_report(envelope: &serde_json::Value) -> Option<InsertableEvent> {
        let metadata = envelope.get("MetaData")?;
        let message = envelope.get("Message")?;
        let position_report = message.get("PositionReport")?;

        let mmsi = metadata
            .get("MMSI")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let ship_name = metadata
            .get("ShipName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let lat = metadata
            .get("latitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let lon = metadata
            .get("longitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let time_utc = metadata
            .get("time_utc")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sog = position_report
            .get("Sog")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let cog = position_report
            .get("Cog")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let true_heading = position_report
            .get("TrueHeading")
            .and_then(|v| v.as_u64())
            .unwrap_or(511); // 511 = not available
        let nav_status_code = position_report
            .get("NavigationalStatus")
            .and_then(|v| v.as_u64())
            .unwrap_or(15);
        let rate_of_turn = position_report
            .get("RateOfTurn")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let region = Self::determine_region(lat, lon);
        let military = Self::is_military_mmsi(mmsi);
        let military_designation = Self::military_designation(mmsi);
        let affiliation = mmsi_country(&mmsi.to_string());
        let nav_status = Self::nav_status_string(nav_status_code);

        let severity = if military { Severity::Medium } else { Severity::Low };
        let mut tags = Vec::new();
        if military {
            tags.push("military".to_string());
            tags.push("naval".to_string());
        }
        if let Some(country) = affiliation {
            tags.push(format!("affiliation:{}", country));
        }

        let data = serde_json::json!({
            "message_type": "PositionReport",
            "mmsi": mmsi,
            "name": ship_name,
            "lat": lat,
            "lon": lon,
            "speed": sog,
            "course": cog,
            "heading": true_heading,
            "nav_status": nav_status,
            "nav_status_code": nav_status_code,
            "rate_of_turn": rate_of_turn,
            "region": region,
            "military": military,
            "military_designation": military_designation,
            "affiliation": affiliation,
            "timestamp": time_utc,
        });

        // heading 511 = not available per AIS spec
        let heading_val = if true_heading == 511 { None } else { Some(true_heading as f32) };

        Some(InsertableEvent {
            event_time: Utc::now(),
            source_type: SourceType::Ais,
            source_id: None,
            longitude: Some(lon),
            latitude: Some(lat),
            region_code: Some(region.to_string()),
            entity_id: Some(mmsi.to_string()),
            entity_name: if ship_name.is_empty() { None } else { Some(ship_name) },
            event_type: EventType::VesselPosition,
            severity,
            confidence: None,
            tags,
            title: None,
            description: None,
            payload: data,
            heading: heading_val,
            speed: Some(sog as f32),
            altitude: None,
        })
    }

    /// Process a ShipStaticData message and return an InsertableEvent.
    fn process_ship_static_data(envelope: &serde_json::Value) -> Option<InsertableEvent> {
        let metadata = envelope.get("MetaData")?;
        let message = envelope.get("Message")?;
        let static_data = message.get("ShipStaticData")?;

        let mmsi = metadata
            .get("MMSI")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let ship_name = metadata
            .get("ShipName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let lat = metadata
            .get("latitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let lon = metadata
            .get("longitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let time_utc = metadata
            .get("time_utc")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let imo = static_data
            .get("ImoNumber")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let call_sign = static_data
            .get("CallSign")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let destination = static_data
            .get("Destination")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let ship_type = static_data
            .get("Type")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let eta = static_data.get("Eta").cloned();
        let draught = static_data
            .get("MaximumStaticDraught")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Dimension info may be nested.
        let dimension = static_data.get("Dimension");
        let length = dimension
            .map(|d| {
                let a = d.get("A").and_then(|v| v.as_u64()).unwrap_or(0);
                let b = d.get("B").and_then(|v| v.as_u64()).unwrap_or(0);
                a + b
            })
            .unwrap_or(0);
        let width = dimension
            .map(|d| {
                let c = d.get("C").and_then(|v| v.as_u64()).unwrap_or(0);
                let dd = d.get("D").and_then(|v| v.as_u64()).unwrap_or(0);
                c + dd
            })
            .unwrap_or(0);

        let region = Self::determine_region(lat, lon);
        let military = Self::is_military_mmsi(mmsi);
        let military_designation = Self::military_designation(mmsi);
        let affiliation = mmsi_country(&mmsi.to_string());

        let severity = if military { Severity::Medium } else { Severity::Low };
        let mut tags = Vec::new();
        if military {
            tags.push("military".to_string());
            tags.push("naval".to_string());
        }
        if let Some(country) = affiliation {
            tags.push(format!("affiliation:{}", country));
        }

        let data = serde_json::json!({
            "message_type": "ShipStaticData",
            "mmsi": mmsi,
            "name": ship_name,
            "lat": lat,
            "lon": lon,
            "imo": imo,
            "call_sign": call_sign,
            "destination": destination,
            "ship_type": ship_type,
            "eta": eta,
            "draught": draught,
            "length": length,
            "width": width,
            "region": region,
            "military": military,
            "military_designation": military_designation,
            "affiliation": affiliation,
            "timestamp": time_utc,
        });

        Some(InsertableEvent {
            event_time: Utc::now(),
            source_type: SourceType::Ais,
            source_id: None,
            longitude: Some(lon),
            latitude: Some(lat),
            region_code: Some(region.to_string()),
            entity_id: Some(mmsi.to_string()),
            entity_name: if ship_name.is_empty() { None } else { Some(ship_name) },
            event_type: EventType::VesselPosition,
            severity,
            confidence: None,
            tags,
            title: None,
            description: None,
            payload: data,
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

impl Default for AisSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for AisSource {
    fn id(&self) -> &str {
        "ais"
    }

    fn name(&self) -> &str {
        "AIS Vessel Tracking"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(0) // streaming
    }

    fn is_streaming(&self) -> bool {
        true
    }

    async fn poll(&self, _ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        // Streaming source; poll is unused.
        Ok(vec![])
    }

    async fn start_stream(
        &self,
        _ctx: &SourceContext,
        tx: broadcast::Sender<InsertableEvent>,
    ) -> anyhow::Result<()> {
        let api_key = std::env::var("AISSTREAM_API_KEY").map_err(|_| {
            warn!("AISSTREAM_API_KEY not set — AIS source will not start");
            anyhow::anyhow!("AISSTREAM_API_KEY environment variable not set")
        })?;

        let subscribe_msg = Self::build_subscribe_message(&api_key);

        let mut message_count: u64 = 0;
        let mut raw_message_count: u64 = 0;
        let mut backoff = INITIAL_BACKOFF;

        // Internal reconnection loop — keeps retrying with exponential backoff.
        // Auth errors propagate immediately; transient failures trigger reconnect.
        loop {
            info!(
                total_processed = message_count,
                "Connecting to aisstream.io WebSocket"
            );

            // Build a custom HTTP request with browser-like headers to avoid
            // Cloudflare bot detection on the WebSocket upgrade handshake.
            let request = match http::Request::builder()
                .uri(AISSTREAM_URL)
                .header("User-Agent", BROWSER_USER_AGENT)
                .header("Origin", "https://aisstream.io")
                .header("Host", "stream.aisstream.io")
                .header("Connection", "Upgrade")
                .header("Upgrade", "websocket")
                .header("Sec-WebSocket-Version", "13")
                .header(
                    "Sec-WebSocket-Key",
                    tokio_tungstenite::tungstenite::handshake::client::generate_key(),
                )
                .body(())
            {
                Ok(r) => r,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to build aisstream.io request: {}",
                        e
                    ));
                }
            };

            let ws_stream = match connect_async(request).await {
                Ok((stream, resp)) => {
                    info!(
                        status = %resp.status(),
                        "aisstream.io WebSocket connected"
                    );
                    stream
                }
                Err(e) => {
                    let jitter_ms = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64 / 4);
                    let total = backoff + Duration::from_millis(jitter_ms);
                    error!(
                        error = %e,
                        backoff_ms = total.as_millis() as u64,
                        "Failed to connect to aisstream.io, retrying after backoff"
                    );
                    tokio::time::sleep(total).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            };

            let (mut write, mut read) = ws_stream.split();

            // Send subscribe message with API key and bounding boxes.
            if let Err(e) = write
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    subscribe_msg.clone().into(),
                ))
                .await
            {
                let jitter_ms = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64 / 4);
                let total = backoff + Duration::from_millis(jitter_ms);
                error!(
                    error = %e,
                    backoff_ms = total.as_millis() as u64,
                    "Failed to send AIS subscribe message, reconnecting"
                );
                tokio::time::sleep(total).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }

            info!(
                regions = BOUNDING_BOXES.len(),
                "Subscribed to aisstream.io AIS stream"
            );

            let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
            keepalive.tick().await; // consume the immediate first tick
            let mut stats_timer = tokio::time::interval(Duration::from_secs(60));
            stats_timer.tick().await; // consume first tick
            let mut first_message_this_session = true;

            // Inner read loop — runs until the connection drops.
            let disconnect_reason: Option<anyhow::Error> = loop {
                tokio::select! {
                    _ = stats_timer.tick() => {
                        info!(
                            raw_messages = raw_message_count,
                            processed_events = message_count,
                            "AIS stream periodic stats"
                        );
                    }

                    msg_opt = read.next() => {
                        let msg_result = match msg_opt {
                            Some(r) => r,
                            None => {
                                warn!("aisstream.io stream ended (None)");
                                break None;
                            }
                        };

                        let msg = match msg_result {
                            Ok(m) => m,
                            Err(e) => {
                                error!(error = %e, "WebSocket read error on aisstream.io");
                                break None;
                            }
                        };

                        let text = match msg {
                            tokio_tungstenite::tungstenite::Message::Text(t) => t,
                            tokio_tungstenite::tungstenite::Message::Ping(data) => {
                                debug!("AIS: received Ping from server, sending Pong");
                                if let Err(e) = write.send(
                                    tokio_tungstenite::tungstenite::Message::Pong(data)
                                ).await {
                                    error!(error = %e, "AIS: failed to send Pong");
                                    break None;
                                }
                                continue;
                            }
                            tokio_tungstenite::tungstenite::Message::Pong(_) => {
                                debug!("AIS: received Pong from server");
                                continue;
                            }
                            tokio_tungstenite::tungstenite::Message::Close(frame) => {
                                warn!(?frame, "aisstream.io WebSocket closed by server");
                                break None;
                            }
                            _ => continue,
                        };

                        let envelope: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(e) => {
                                debug!(error = %e, "Failed to parse aisstream.io message");
                                continue;
                            }
                        };

                        // Check for error responses from the server (e.g. invalid API key).
                        if let Some(err_msg) = envelope.get("error").and_then(|v| v.as_str()) {
                            let lower = err_msg.to_lowercase();
                            let is_auth = lower.contains("key") || lower.contains("invalid")
                                || lower.contains("unauthorized") || lower.contains("authentication");
                            if is_auth {
                                // Auth errors are fatal — propagate immediately so the
                                // registry disables this source instead of retrying.
                                break Some(AuthError {
                                    source: "aisstream.io".into(),
                                    message: err_msg.to_string(),
                                }.into());
                            }
                            error!(error = err_msg, "aisstream.io returned error");
                            break None;
                        }

                        raw_message_count += 1;

                        // Reset backoff on first successful data message — connection is healthy.
                        if first_message_this_session {
                            first_message_this_session = false;
                            backoff = INITIAL_BACKOFF;
                            info!("AIS stream: connection healthy, backoff reset");
                        }

                        let message_type = envelope
                            .get("MessageType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let event = match message_type {
                            "PositionReport" => Self::process_position_report(&envelope),
                            "ShipStaticData" => Self::process_ship_static_data(&envelope),
                            _ => {
                                debug!(message_type, "Ignoring unhandled AIS message type");
                                None
                            }
                        };

                        if let Some(evt) = event {
                            let _ = tx.send(evt);
                            message_count += 1;
                            if message_count == 1 {
                                info!("AIS stream: first vessel message received and broadcast");
                            }
                            if message_count % 1000 == 0 {
                                info!(total = message_count, "AIS stream: messages processed");
                            }
                        }
                    }

                    _ = keepalive.tick() => {
                        debug!("AIS: sending keepalive Ping");
                        if let Err(e) = write.send(
                            tokio_tungstenite::tungstenite::Message::Ping(bytes::Bytes::from_static(b"keepalive"))
                        ).await {
                            error!(error = %e, "AIS: failed to send keepalive Ping");
                            break None;
                        }
                    }
                }
            };

            // If the inner loop returned a fatal error (auth), propagate it.
            if let Some(fatal) = disconnect_reason {
                return Err(fatal);
            }

            // Transient disconnect — reconnect after backoff.
            let jitter_ms = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64 / 4);
            let total = backoff + Duration::from_millis(jitter_ms);
            warn!(
                backoff_ms = total.as_millis() as u64,
                total_processed = message_count,
                "AIS stream disconnected, reconnecting after backoff"
            );
            tokio::time::sleep(total).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_message_format() {
        let msg = AisSource::build_subscribe_message("test-key-123");
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["APIKey"], "test-key-123");

        let boxes = parsed["BoundingBoxes"].as_array().unwrap();
        assert_eq!(boxes.len(), BOUNDING_BOXES.len());

        // Verify first bounding box (Strait of Hormuz)
        let first = &boxes[0];
        assert_eq!(first[0][0].as_f64().unwrap(), 25.5); // lat_min
        assert_eq!(first[0][1].as_f64().unwrap(), 54.0); // lon_min
        assert_eq!(first[1][0].as_f64().unwrap(), 27.0); // lat_max
        assert_eq!(first[1][1].as_f64().unwrap(), 57.0); // lon_max

        // Verify FilterMessageTypes included
        let filter = parsed["FilterMessageTypes"].as_array().unwrap();
        assert!(filter.contains(&serde_json::json!("PositionReport")));
        assert!(filter.contains(&serde_json::json!("ShipStaticData")));
    }

    #[test]
    fn test_determine_region() {
        // Strait of Hormuz
        assert_eq!(AisSource::determine_region(26.0, 55.5), "Strait of Hormuz");
        // Red Sea
        assert_eq!(AisSource::determine_region(18.0, 40.0), "Red Sea");
        // Persian Gulf
        assert_eq!(AisSource::determine_region(27.0, 50.0), "Persian Gulf");
        // Black Sea
        assert_eq!(AisSource::determine_region(43.0, 35.0), "Black Sea");
        // Unknown
        assert_eq!(AisSource::determine_region(0.0, 0.0), "Unknown");
    }

    #[test]
    fn test_military_mmsi() {
        assert!(AisSource::is_military_mmsi(338123456)); // US Navy
        assert!(AisSource::is_military_mmsi(369000001)); // US Navy
        assert!(AisSource::is_military_mmsi(422000001)); // Iran Navy
        assert!(!AisSource::is_military_mmsi(211000001)); // Not military
    }

    #[test]
    fn test_military_designation() {
        assert_eq!(AisSource::military_designation(338123456), Some("US Navy"));
        assert_eq!(AisSource::military_designation(422000001), Some("Iran Navy"));
        assert_eq!(AisSource::military_designation(211000001), None);
    }

    #[test]
    fn test_process_position_report() {
        let envelope = serde_json::json!({
            "MessageType": "PositionReport",
            "MetaData": {
                "MMSI": 338123456_u64,
                "ShipName": "USS TEST",
                "latitude": 26.5,
                "longitude": 55.0,
                "time_utc": "2026-03-01T12:00:00Z"
            },
            "Message": {
                "PositionReport": {
                    "Sog": 12.5,
                    "Cog": 180.0,
                    "TrueHeading": 179,
                    "NavigationalStatus": 0,
                    "RateOfTurn": 0
                }
            }
        });

        let event = AisSource::process_position_report(&envelope).unwrap();
        assert_eq!(event.source_type, SourceType::Ais);
        assert_eq!(event.event_type, EventType::VesselPosition);
        assert_eq!(event.entity_id, Some("338123456".to_string()));
        assert_eq!(event.entity_name, Some("USS TEST".to_string()));
        assert_eq!(event.latitude, Some(26.5));
        assert_eq!(event.longitude, Some(55.0));
        assert_eq!(event.heading, Some(179.0));
        assert_eq!(event.speed, Some(12.5));
        assert_eq!(event.severity, Severity::Medium); // military
        assert!(event.tags.contains(&"military".to_string()));
    }

    #[test]
    fn test_process_position_report_heading_unavailable() {
        let envelope = serde_json::json!({
            "MessageType": "PositionReport",
            "MetaData": {
                "MMSI": 211000001_u64,
                "ShipName": "CIVILIAN VESSEL",
                "latitude": 18.0,
                "longitude": 40.0,
                "time_utc": "2026-03-01T12:00:00Z"
            },
            "Message": {
                "PositionReport": {
                    "Sog": 5.0,
                    "Cog": 90.0,
                    "TrueHeading": 511,
                    "NavigationalStatus": 0,
                    "RateOfTurn": 0
                }
            }
        });

        let event = AisSource::process_position_report(&envelope).unwrap();
        assert_eq!(event.heading, None); // 511 = not available
        assert_eq!(event.severity, Severity::Low); // civilian
    }

    #[test]
    fn test_nav_status_string() {
        assert_eq!(AisSource::nav_status_string(0), "Under way using engine");
        assert_eq!(AisSource::nav_status_string(5), "Moored");
        assert_eq!(AisSource::nav_status_string(7), "Engaged in Fishing");
        assert_eq!(AisSource::nav_status_string(15), "Undefined / default");
        assert_eq!(AisSource::nav_status_string(99), "Unknown");
    }

    /// Integration test that connects to the real aisstream.io WebSocket and
    /// verifies keepalive/pong behavior over a 3-minute session.
    ///
    /// Run manually:
    ///   AISSTREAM_API_KEY=<key> cargo test -p sr-sources ais_websocket_keepalive -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn ais_websocket_keepalive_debug() {
        use std::time::Instant;
        use tokio::time;

        let api_key = match std::env::var("AISSTREAM_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                eprintln!("AISSTREAM_API_KEY not set — skipping integration test");
                return;
            }
        };

        let test_duration = Duration::from_secs(180); // 3 minutes
        let keepalive_interval = Duration::from_secs(30);

        // Use only Persian Gulf for a smaller subscription
        let subscribe_msg = serde_json::json!({
            "APIKey": api_key,
            "BoundingBoxes": [[[24.0, 48.0], [30.0, 56.0]]],
            "FilterMessageTypes": ["PositionReport"],
        })
        .to_string();

        eprintln!("=== AIS WebSocket Keepalive Debug Test ===");
        eprintln!("Duration: {}s", test_duration.as_secs());
        eprintln!("Keepalive interval: {}s", keepalive_interval.as_secs());
        eprintln!("Connecting to {}...", AISSTREAM_URL);

        let connect_start = Instant::now();

        let (ws_stream, response) = connect_async(AISSTREAM_URL)
            .await
            .expect("Failed to connect to aisstream.io");

        eprintln!(
            "Connected in {:?} (HTTP {})",
            connect_start.elapsed(),
            response.status()
        );

        let (mut write, mut read) = ws_stream.split();

        // Send subscription
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                subscribe_msg.into(),
            ))
            .await
            .expect("Failed to send subscribe message");
        eprintln!("Subscription sent (Persian Gulf region)");

        let session_start = Instant::now();
        let mut keepalive_timer = time::interval(keepalive_interval);
        keepalive_timer.tick().await; // consume immediate first tick

        let mut pings_received: u64 = 0;
        let mut pongs_received: u64 = 0;
        let mut pongs_sent: u64 = 0;
        let mut pings_sent: u64 = 0;
        let mut text_messages: u64 = 0;
        let mut close_received = false;
        let mut last_activity = Instant::now();

        let deadline = time::sleep(test_duration);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => {
                    eprintln!("\n=== Test duration reached ({}s) ===", test_duration.as_secs());
                    break;
                }

                msg_opt = read.next() => {
                    last_activity = Instant::now();
                    let elapsed = session_start.elapsed();

                    match msg_opt {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(data))) => {
                            pings_received += 1;
                            eprintln!(
                                "[{:>6.1}s] PING received (#{}, {} bytes)",
                                elapsed.as_secs_f64(),
                                pings_received,
                                data.len()
                            );
                            // Send Pong response
                            if let Err(e) = write.send(
                                tokio_tungstenite::tungstenite::Message::Pong(data)
                            ).await {
                                eprintln!("[{:>6.1}s] ERROR sending Pong: {}", elapsed.as_secs_f64(), e);
                                break;
                            }
                            pongs_sent += 1;
                            eprintln!(
                                "[{:>6.1}s] PONG sent (#{}) in response",
                                elapsed.as_secs_f64(),
                                pongs_sent
                            );
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(data))) => {
                            pongs_received += 1;
                            eprintln!(
                                "[{:>6.1}s] PONG received (#{}, {} bytes) — response to our Ping",
                                elapsed.as_secs_f64(),
                                pongs_received,
                                data.len()
                            );
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_))) => {
                            text_messages += 1;
                            if text_messages <= 3 || text_messages % 100 == 0 {
                                eprintln!(
                                    "[{:>6.1}s] TEXT #{} received",
                                    elapsed.as_secs_f64(),
                                    text_messages
                                );
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                            close_received = true;
                            eprintln!(
                                "[{:>6.1}s] CLOSE received: {:?}",
                                elapsed.as_secs_f64(),
                                frame
                            );
                            break;
                        }
                        Some(Ok(other)) => {
                            eprintln!(
                                "[{:>6.1}s] OTHER message: {:?}",
                                elapsed.as_secs_f64(),
                                other
                            );
                        }
                        Some(Err(e)) => {
                            eprintln!(
                                "[{:>6.1}s] READ ERROR: {}",
                                elapsed.as_secs_f64(),
                                e
                            );
                            break;
                        }
                        None => {
                            eprintln!(
                                "[{:>6.1}s] STREAM ENDED (None)",
                                elapsed.as_secs_f64()
                            );
                            break;
                        }
                    }
                }

                _ = keepalive_timer.tick() => {
                    let elapsed = session_start.elapsed();
                    pings_sent += 1;
                    eprintln!(
                        "[{:>6.1}s] PING sent (keepalive #{})",
                        elapsed.as_secs_f64(),
                        pings_sent
                    );
                    if let Err(e) = write.send(
                        tokio_tungstenite::tungstenite::Message::Ping(
                            bytes::Bytes::from_static(b"keepalive")
                        )
                    ).await {
                        eprintln!(
                            "[{:>6.1}s] ERROR sending keepalive Ping: {}",
                            elapsed.as_secs_f64(),
                            e
                        );
                        break;
                    }
                }
            }
        }

        let total_duration = session_start.elapsed();

        eprintln!("\n=== Results ===");
        eprintln!("Connection duration: {:.1}s", total_duration.as_secs_f64());
        eprintln!("Text messages received: {}", text_messages);
        eprintln!("Server Pings received: {}", pings_received);
        eprintln!("Pongs sent (to server): {}", pongs_sent);
        eprintln!("Keepalive Pings sent: {}", pings_sent);
        eprintln!("Pongs received (from server): {}", pongs_received);
        eprintln!("Close frame received: {}", close_received);
        eprintln!(
            "Last activity: {:.1}s before end",
            (total_duration - last_activity.duration_since(session_start)).as_secs_f64()
        );

        // If we ran the full duration without disconnect, the keepalive is working.
        // If we got disconnected early, the test output shows when and why.
        if !close_received && total_duration >= test_duration - Duration::from_secs(5) {
            eprintln!("\nSUCCESS: Connection stayed alive for the full test duration.");
        } else if close_received || total_duration < test_duration - Duration::from_secs(5) {
            eprintln!(
                "\nFAILURE: Connection dropped after {:.1}s (expected {:.1}s).",
                total_duration.as_secs_f64(),
                test_duration.as_secs_f64()
            );
        }

        // Soft assertions — print diagnostics rather than panic, since this is a debug test
        assert!(
            !close_received,
            "Server sent Close frame — keepalive may not be working"
        );
        assert!(
            total_duration >= test_duration - Duration::from_secs(5),
            "Connection dropped after {:.1}s, expected to survive {:.1}s",
            total_duration.as_secs_f64(),
            test_duration.as_secs_f64()
        );
    }
}
