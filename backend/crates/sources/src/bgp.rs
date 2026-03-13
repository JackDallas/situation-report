use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};
use crate::common::region_for_country;

/// Window for deduplicating (origin_asn, prefix) pairs. Withdrawals for the same
/// prefix from the same ASN within this window are suppressed to avoid flooding
/// the pipeline with routine Rostelecom-style route flapping (~160 events/min → ~5/min).
const DEDUP_WINDOW: Duration = Duration::from_secs(300); // 5 minutes

/// Monitored ASNs by country/region:
/// Iran: 12880, 48159, 6736, 58224, 197207, 44244
/// Israel: 378, 8551, 9116
/// Ukraine: 6849, 15895
/// Russia: 12389, 8402
const MONITORED_ASNS: &[(u32, &str)] = &[
    // Iran
    (12880, "IR"),
    (48159, "IR"),
    (6736, "IR"),
    (58224, "IR"),
    (197207, "IR"),
    (44244, "IR"),
    // Israel
    (378, "IL"),
    (8551, "IL"),
    (9116, "IL"),
    // Ukraine
    (6849, "UA"),
    (15895, "UA"),
    // Russia
    (12389, "RU"),
    (8402, "RU"),
];

/// WebSocket URL for RIPE RIS Live.
const RIS_LIVE_URL: &str = "wss://ris-live.ripe.net/v1/ws/";

/// Subscribe message to send after connecting.
const SUBSCRIBE_MSG: &str = r#"{"type":"ris_subscribe","data":{"type":"UPDATE","socketOptions":{"includeRaw":false}}}"#;

/// RIPE RIS Live BGP stream source.
pub struct BgpSource;

impl Default for BgpSource {
    fn default() -> Self {
        Self::new()
    }
}

impl BgpSource {
    pub fn new() -> Self {
        Self
    }

    /// Check if an ASN is in our monitored set. Returns the country code if found.
    fn lookup_asn(asn: u32) -> Option<&'static str> {
        MONITORED_ASNS
            .iter()
            .find(|(a, _)| *a == asn)
            .map(|(_, cc)| *cc)
    }

    /// Check if any ASN in a path is monitored.
    fn find_monitored_asn_in_path(path: &[serde_json::Value]) -> Option<(u32, &'static str)> {
        for element in path {
            // Path elements can be numbers or arrays (AS sets).
            if let Some(asn) = element.as_u64() {
                if let Some(cc) = Self::lookup_asn(asn as u32) {
                    return Some((asn as u32, cc));
                }
            } else if let Some(as_set) = element.as_array() {
                for asn_val in as_set {
                    if let Some(asn) = asn_val.as_u64()
                        && let Some(cc) = Self::lookup_asn(asn as u32) {
                            return Some((asn as u32, cc));
                        }
                }
            }
        }
        None
    }
}

#[async_trait]
impl DataSource for BgpSource {
    fn id(&self) -> &str {
        "bgp"
    }

    fn name(&self) -> &str {
        "BGP RIS Live"
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
        info!("Connecting to RIPE RIS Live WebSocket");

        let (ws_stream, _response) = connect_async(RIS_LIVE_URL)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to RIS Live: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // Send subscribe message.
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                SUBSCRIBE_MSG.into(),
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send subscribe message: {}", e))?;

        info!("Subscribed to RIS Live BGP UPDATE stream");

        // Counters to log how many routine events we persist-only vs broadcast
        let persisted_only = AtomicU64::new(0);
        let broadcast_count = AtomicU64::new(0);
        let dedup_count = AtomicU64::new(0);
        let mut last_stats_log = tokio::time::Instant::now();

        // Dedup: track (origin_asn, prefix) → last_seen_time to suppress
        // repeated withdrawals for the same prefix within DEDUP_WINDOW.
        let mut seen_withdrawals: HashMap<(u32, String), tokio::time::Instant> = HashMap::new();
        let mut last_dedup_cleanup = tokio::time::Instant::now();

        while let Some(msg_result) = read.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    error!(error = %e, "WebSocket read error on RIS Live");
                    return Err(e.into());
                }
            };

            let text = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => t,
                tokio_tungstenite::tungstenite::Message::Ping(_) => continue,
                tokio_tungstenite::tungstenite::Message::Close(_) => {
                    warn!("RIS Live WebSocket closed by server");
                    return Err(anyhow::anyhow!("RIS Live WebSocket closed by server"));
                }
                _ => continue,
            };

            let envelope: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    debug!(error = %e, "Failed to parse RIS Live message");
                    continue;
                }
            };

            // Expect: {"type":"ris_message","data":{...}}
            let msg_type = envelope.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if msg_type != "ris_message" {
                continue;
            }

            let data = match envelope.get("data") {
                Some(d) => d,
                None => continue,
            };

            let timestamp = data.get("timestamp").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let peer = data
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let peer_asn = data
                .get("peer_asn")
                .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_u64().map(|n| n.to_string())))
                .unwrap_or_default();
            let host = data
                .get("host")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let path = data
                .get("path")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            // Process announcements — these are routine and EXTREMELY HIGH VOLUME
            // (~6000/min = 100/sec). Just count them for observability.
            // We do NOT persist or broadcast individual announcements — only
            // withdrawals (route disappearances) are operationally significant.
            if let Some(announcements) = data.get("announcements").and_then(|v| v.as_array()) {
                for announcement in announcements {
                    let prefixes = announcement
                        .get("prefixes")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len() as u64)
                        .unwrap_or(0);

                    if Self::find_monitored_asn_in_path(&path).is_some() {
                        persisted_only.fetch_add(prefixes, Ordering::Relaxed);
                    }
                }
            }

            // Process withdrawals — these are significant (route disappearing)
            // and much lower volume. Broadcast to pipeline for correlation.
            if let Some(withdrawals) = data.get("withdrawals").and_then(|v| v.as_array()) {
                // For withdrawals, check the peer_asn or look for monitored ASNs in path.
                let peer_asn_num: Option<u32> = peer_asn.parse().ok();
                let monitored_via_peer = peer_asn_num.and_then(Self::lookup_asn);
                let monitored_via_path = Self::find_monitored_asn_in_path(&path);

                let (origin_asn, country) = if let Some(cc) = monitored_via_peer {
                    (peer_asn_num.unwrap_or(0), cc)
                } else if let Some((asn, cc)) = monitored_via_path {
                    (asn, cc)
                } else {
                    continue;
                };

                let region = region_for_country(country);

                // Periodic cleanup of expired dedup entries (every 5 min)
                if last_dedup_cleanup.elapsed() >= DEDUP_WINDOW {
                    let now = tokio::time::Instant::now();
                    seen_withdrawals.retain(|_, ts| now.duration_since(*ts) < DEDUP_WINDOW);
                    last_dedup_cleanup = now;
                }

                for prefix in withdrawals {
                    let prefix_str = prefix.as_str().unwrap_or("");

                    // Dedup: skip if we already broadcast this (ASN, prefix) recently
                    let dedup_key = (origin_asn, prefix_str.to_string());
                    let now = tokio::time::Instant::now();
                    if let Some(last_seen) = seen_withdrawals.get(&dedup_key) {
                        if now.duration_since(*last_seen) < DEDUP_WINDOW {
                            dedup_count.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    }
                    seen_withdrawals.insert(dedup_key, now);

                    let event_data = serde_json::json!({
                        "event_type": "withdrawal",
                        "prefix": prefix_str,
                        "peer": peer,
                        "peer_asn": peer_asn,
                        "origin_asn": origin_asn,
                        "country": country,
                        "path": path,
                        "host": host,
                        "timestamp": timestamp,
                    });

                    let title = format!("BGP withdrawal: AS{} prefix {}", origin_asn, prefix_str);

                    let _ = tx.send(InsertableEvent {
                        event_time: Utc::now(),
                        source_type: SourceType::Bgp,
                        source_id: None,
                        longitude: None,
                        latitude: None,
                        region_code: region.map(String::from),
                        entity_id: Some(format!("AS{}", origin_asn)),
                        entity_name: None,
                        event_type: EventType::BgpAnomaly,
                        severity: Severity::High,
                        confidence: None,
                        tags: vec![],
                        title: Some(title),
                        description: None,
                        payload: event_data,
                        heading: None,
                        speed: None,
                        altitude: None,
                    });
                    broadcast_count.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Log stats every 5 minutes
            if last_stats_log.elapsed() >= Duration::from_secs(300) {
                let p = persisted_only.swap(0, Ordering::Relaxed);
                let b = broadcast_count.swap(0, Ordering::Relaxed);
                let d = dedup_count.swap(0, Ordering::Relaxed);
                info!(
                    persisted_only = p,
                    broadcast = b,
                    deduped = d,
                    dedup_cache_size = seen_withdrawals.len(),
                    "BGP stream stats (last 5min)"
                );
                last_stats_log = tokio::time::Instant::now();
            }
        }

        Err(anyhow::anyhow!("RIS Live stream ended unexpectedly"))
    }
}
