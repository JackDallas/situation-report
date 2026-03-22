use std::time::Duration;

use futures_util::StreamExt;
use rand::Rng;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

/// WebSocket URL for the CertStream service.
const CERTSTREAM_URL: &str = "wss://certstream.calidog.io";

/// Domain patterns to filter for. Only certificates matching one of these
/// suffixes will generate events (the stream is very high volume).
const DOMAIN_PATTERNS: &[&str] = &[
    ".gov.ir",
    ".mil.ir",
    ".gov.il",
    ".mil.il",
    ".gov.ua",
    ".mil.ua",
    ".mod.gov.",
    ".irgc.ir",
    ".gov.ru",
    ".mil.ru",
];

/// Certificate transparency live stream source.
pub struct CertstreamSource;

impl Default for CertstreamSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CertstreamSource {
    pub fn new() -> Self {
        Self
    }

    /// Check if any domain in the list matches one of our monitored patterns.
    fn matches_pattern(domains: &[String]) -> bool {
        for domain in domains {
            let lower = domain.to_lowercase();
            for pattern in DOMAIN_PATTERNS {
                if lower.ends_with(pattern) || lower.contains(pattern) {
                    return true;
                }
            }
        }
        false
    }

    /// Extract all domain strings from the JSON message.
    fn extract_domains(data: &serde_json::Value) -> Vec<String> {
        let mut domains = Vec::new();

        // Common name from subject.
        if let Some(cn) = data
            .pointer("/leaf_cert/subject/CN")
            .and_then(|v| v.as_str())
        {
            domains.push(cn.to_string());
        }

        // all_domains array.
        if let Some(all) = data
            .pointer("/leaf_cert/all_domains")
            .and_then(|v| v.as_array())
        {
            for d in all {
                if let Some(s) = d.as_str() {
                    domains.push(s.to_string());
                }
            }
        }

        domains
    }
}

impl DataSource for CertstreamSource {
    fn id(&self) -> &str {
        "certstream"
    }

    fn name(&self) -> &str {
        "Certificate Transparency"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(0) // streaming
    }

    fn is_streaming(&self) -> bool {
        true
    }

    fn poll<'a>(&'a self, _ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        // Streaming source; poll is unused.
        Ok(vec![])
        })
    }

    fn start_stream<'a>(
        &'a self,
        _ctx: &'a SourceContext,
        tx: broadcast::Sender<InsertableEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
        let mut backoff_secs = 1u64;

        loop {
            info!(backoff_secs, "Connecting to CertStream WebSocket");

            let ws_result = connect_async(CERTSTREAM_URL).await;
            let (ws_stream, _response) = match ws_result {
                Ok(ws) => {
                    backoff_secs = 1; // reset on successful connection
                    ws
                }
                Err(e) => {
                    let jitter_ms = rand::rng().random_range(0..=backoff_secs * 1000 / 4);
                    let total = Duration::from_millis(backoff_secs * 1000 + jitter_ms);
                    error!(error = %e, "Failed to connect to CertStream");
                    tokio::time::sleep(total).await;
                    backoff_secs = (backoff_secs * 2).min(60);
                    continue;
                }
            };

            let (_write, mut read) = ws_stream.split();
            info!("Connected to CertStream");

            loop {
                let msg = match read.next().await {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        warn!(error = %e, "CertStream read error, reconnecting");
                        break; // break inner loop to reconnect
                    }
                    None => {
                        warn!("CertStream stream ended, reconnecting");
                        break;
                    }
                };

                let text = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => t,
                    tokio_tungstenite::tungstenite::Message::Ping(_) => continue,
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        warn!("CertStream closed by server, reconnecting");
                        break;
                    }
                    _ => continue,
                };

                let envelope: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Only process certificate_update messages.
                let msg_type = envelope
                    .get("message_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if msg_type != "certificate_update" {
                    continue;
                }

                let data = match envelope.get("data") {
                    Some(d) => d,
                    None => continue,
                };

                let domains = Self::extract_domains(data);

                if !Self::matches_pattern(&domains) {
                    continue;
                }

                // Extract issuer information.
                let issuer = data
                    .pointer("/leaf_cert/issuer/O")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let seen = data
                    .get("seen")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                let cert_index = data
                    .get("cert_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let primary_domain = domains.first().cloned().unwrap_or_default();

                debug!(domain = %primary_domain, "CertStream match found");

                // Infer region from domain TLD
                let region =
                    if primary_domain.ends_with(".ir") || primary_domain.contains(".gov.ir")
                        || primary_domain.ends_with(".il") || primary_domain.contains(".gov.il")
                    {
                        Some("middle-east")
                    } else if primary_domain.ends_with(".ua")
                        || primary_domain.contains(".gov.ua")
                        || primary_domain.ends_with(".ru")
                        || primary_domain.contains(".gov.ru")
                    {
                        Some("eastern-europe")
                    } else {
                        None
                    };

                let title = format!("Certificate issued: {}", primary_domain);

                let event_data = serde_json::json!({
                    "domain": primary_domain,
                    "all_domains": domains,
                    "issuer": issuer,
                    "seen": seen,
                    "cert_index": cert_index,
                });

                let _ = tx.send(InsertableEvent {
                    event_time: Utc::now(),
                    source_type: SourceType::Certstream,
                    source_id: None,
                    longitude: None,
                    latitude: None,
                    region_code: region.map(String::from),
                    entity_id: Some(primary_domain.clone()),
                    entity_name: Some(issuer),
                    event_type: EventType::CertIssued,
                    severity: Severity::Low,
                    confidence: None,
                    tags: domains.iter().take(5).cloned().collect(),
                    title: Some(title),
                    description: None,
                    payload: event_data,
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }

            // Exponential backoff before reconnect
            let jitter_ms = rand::rng().random_range(0..=backoff_secs * 1000 / 4);
            let total = Duration::from_millis(backoff_secs * 1000 + jitter_ms);
            tokio::time::sleep(total).await;
            backoff_secs = (backoff_secs * 2).min(60);
        }
        })
    }
}
