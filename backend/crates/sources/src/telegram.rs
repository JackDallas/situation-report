use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;
use grammers_client::{Client, SenderPool};
use grammers_session::storages::SqliteSession;
use grammers_session::types::PeerRef;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};

// ---------------------------------------------------------------------------
// Channel configuration
// ---------------------------------------------------------------------------

struct ChannelConfig {
    username: &'static str,
    display_name: &'static str,
    region: Option<&'static str>,
    tier: u8,
}

const CHANNELS: &[ChannelConfig] = &[
    // Tier 1 - Structured alerts (via Telegram since direct API is geo-blocked)
    ChannelConfig {
        username: "CumtaAlertsEnglishChannel",
        display_name: "Cumta Red Alerts",
        region: Some("middle-east"),
        tier: 1,
    },
    // Tier 2 - English OSINT aggregators
    ChannelConfig {
        username: "noel_reports",
        display_name: "NOELREPORTS",
        region: Some("eastern-europe"),
        tier: 2,
    },
    ChannelConfig {
        username: "warmonitors",
        display_name: "War Monitor",
        region: None,
        tier: 2,
    },
    ChannelConfig {
        username: "intelslava",
        display_name: "Intel Slava Z",
        region: Some("eastern-europe"),
        tier: 2,
    },
    ChannelConfig {
        username: "sitreports",
        display_name: "SITREP",
        region: None,
        tier: 2,
    },
    ChannelConfig {
        username: "ClashReport",
        display_name: "Clash Report",
        region: None,
        tier: 2,
    },
    // Tier 3 - Analytical / verified
    ChannelConfig {
        username: "DeepStateEN",
        display_name: "DeepState English",
        region: Some("eastern-europe"),
        tier: 3,
    },
    ChannelConfig {
        username: "rybar_in_english",
        display_name: "Rybar English",
        region: Some("eastern-europe"),
        tier: 3,
    },
    ChannelConfig {
        username: "CIT_en",
        display_name: "Conflict Intelligence Team",
        region: Some("eastern-europe"),
        tier: 3,
    },
    ChannelConfig {
        username: "GeoConfirmed",
        display_name: "GeoConfirmed",
        region: None,
        tier: 3,
    },
    ChannelConfig {
        username: "Intelsky",
        display_name: "IntelSky",
        region: None,
        tier: 3,
    },
    // Tier 4 - Regional
    ChannelConfig {
        username: "DIUkraine",
        display_name: "Ukraine Military Intel",
        region: Some("eastern-europe"),
        tier: 4,
    },
    ChannelConfig {
        username: "Ansarallah_MC",
        display_name: "Houthi Military Media",
        region: Some("middle-east"),
        tier: 4,
    },
    ChannelConfig {
        username: "englishabuali",
        display_name: "Abu Ali Express",
        region: Some("middle-east"),
        tier: 4,
    },
];

// ---------------------------------------------------------------------------
// Keyword severity classification
// ---------------------------------------------------------------------------

/// Keywords that trigger critical severity
const CRITICAL_KEYWORDS: &[&str] = &[
    "strike", "missile", "nuclear", "IRGC", "IDF", "drone", "explosion",
    "attack", "Natanz", "Fordow", "Isfahan", "Hormuz", "Bandar Abbas",
    "BREAKING", "URGENT",
];

/// Keywords that trigger high severity
const HIGH_KEYWORDS: &[&str] = &[
    "military", "airstrike", "bombing", "casualties", "intercept",
    "mobilization", "escalation", "retaliation", "sanctions",
    "radar", "submarine", "warship", "convoy", "airspace",
];

// ---------------------------------------------------------------------------
// Source struct
// ---------------------------------------------------------------------------

pub struct TelegramSource;

impl Default for TelegramSource {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramSource {
    pub fn new() -> Self {
        Self
    }

    fn classify_severity(text: &str) -> Severity {
        let upper = text.to_uppercase();
        for kw in CRITICAL_KEYWORDS {
            if upper.contains(&kw.to_uppercase()) {
                return Severity::Critical;
            }
        }
        for kw in HIGH_KEYWORDS {
            if upper.contains(&kw.to_uppercase()) {
                return Severity::High;
            }
        }
        Severity::Medium
    }

    fn extract_tags(text: &str) -> Vec<String> {
        let mut tags = vec!["source:telegram".to_string()];
        let upper = text.to_uppercase();

        for kw in CRITICAL_KEYWORDS.iter().chain(HIGH_KEYWORDS.iter()) {
            if upper.contains(&kw.to_uppercase()) {
                tags.push(kw.to_lowercase());
            }
        }

        tags.sort();
        tags.dedup();
        tags
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a channel message into an InsertableEvent.
fn message_to_event(
    text: &str,
    msg_id: i32,
    msg_date: DateTime<Utc>,
    config: &ChannelConfig,
) -> InsertableEvent {
    let severity = TelegramSource::classify_severity(text);
    let tags = TelegramSource::extract_tags(text);

    let truncated: String = text.chars().take(100).collect();
    let title = format!("[{}] {}", config.display_name, truncated);
    let description: String = text.chars().take(500).collect();

    InsertableEvent {
        event_time: msg_date,
        source_type: SourceType::Telegram,
        source_id: Some(format!("tg:{}:{}", config.username, msg_id)),
        longitude: None,
        latitude: None,
        region_code: config.region.map(String::from),
        entity_id: Some(config.username.to_string()),
        entity_name: Some(config.display_name.to_string()),
        event_type: EventType::TelegramMessage,
        severity,
        confidence: None,
        tags,
        title: Some(title),
        description: Some(description),
        payload: serde_json::json!({
            "channel": config.username,
            "channel_name": config.display_name,
            "message_id": msg_id,
            "text": text,
            "tier": config.tier,
            "region": config.region,
        }),
        heading: None,
        speed: None,
        altitude: None,
    }
}

// ---------------------------------------------------------------------------
// DataSource implementation
// ---------------------------------------------------------------------------

impl DataSource for TelegramSource {
    fn id(&self) -> &str {
        "telegram"
    }

    fn name(&self) -> &str {
        "Telegram OSINT Channels"
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
        ctx: &'a SourceContext,
        tx: broadcast::Sender<InsertableEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
        // ---- 1. Read auth from env ----
        let api_id: i32 = match std::env::var("TELEGRAM_API_ID") {
            Ok(v) if !v.is_empty() => match v.parse() {
                Ok(id) => id,
                Err(e) => {
                    warn!("TELEGRAM_API_ID is not a valid integer: {e}");
                    return Ok(());
                }
            },
            _ => {
                // Silent skip -- Telegram MTProto is optional
                debug!("TELEGRAM_API_ID not set -- Telegram source disabled");
                return Ok(());
            }
        };

        let api_hash = match std::env::var("TELEGRAM_API_HASH") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                warn!("TELEGRAM_API_HASH not set -- Telegram source disabled");
                return Ok(());
            }
        };

        // ---- 2. Session management ----
        let session_path = std::env::var("TELEGRAM_SESSION_PATH")
            .unwrap_or_else(|_| "data/telegram.session".to_string());

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(&session_path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        info!(session_path = %session_path, "Opening Telegram session");
        let session = Arc::new(SqliteSession::open(&session_path).await.map_err(|e| {
            anyhow::anyhow!("Failed to open Telegram session at {session_path}: {e}")
        })?);

        // ---- 3. Connect ----
        let mut backoff_secs = 1u64;

        loop {
            info!(backoff_secs, "Connecting to Telegram MTProto");

            let pool = SenderPool::new(Arc::clone(&session), api_id);
            let SenderPool {
                runner,
                handle,
                updates,
            } = pool;

            let client = Client::new(handle.clone());

            // Spawn the network runner
            let runner_task = tokio::spawn(runner.run());

            // ---- 4. Check auth ----
            match client.is_authorized().await {
                Ok(true) => {
                    info!("Telegram client authorized");
                }
                Ok(false) => {
                    warn!(
                        "Telegram not authorized. To perform initial login:\n\
                         1. Set TELEGRAM_PHONE env var with your phone (international format)\n\
                         2. Set TELEGRAM_CODE env var with the code you receive\n\
                         Attempting interactive-less auth..."
                    );

                    let phone = match std::env::var("TELEGRAM_PHONE") {
                        Ok(p) if !p.is_empty() => p,
                        _ => {
                            warn!("TELEGRAM_PHONE not set -- cannot authorize. Telegram source disabled.");
                            handle.quit();
                            let _ = runner_task.await;
                            return Ok(());
                        }
                    };

                    // Request login code
                    info!(phone = %phone, "Requesting Telegram login code");
                    let token = match client.request_login_code(&phone, &api_hash).await {
                        Ok(t) => t,
                        Err(e) => {
                            error!("Failed to request login code: {e}");
                            handle.quit();
                            let _ = runner_task.await;
                            return Err(anyhow::anyhow!("Telegram login code request failed: {e}"));
                        }
                    };

                    // Check for code in env (for non-interactive auth)
                    let code = match std::env::var("TELEGRAM_CODE") {
                        Ok(c) if !c.is_empty() => c,
                        _ => {
                            warn!(
                                "Login code sent to your Telegram app. \
                                 Set TELEGRAM_CODE env var and restart to complete login."
                            );
                            handle.quit();
                            let _ = runner_task.await;
                            return Ok(());
                        }
                    };

                    match client.sign_in(&token, &code).await {
                        Ok(user) => {
                            info!(
                                name = user.first_name().unwrap_or("unknown"),
                                "Telegram sign-in successful"
                            );
                        }
                        Err(e) => {
                            error!("Telegram sign-in failed: {e}");
                            handle.quit();
                            let _ = runner_task.await;
                            return Err(anyhow::anyhow!("Telegram sign-in failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to check Telegram auth: {e}, will retry");
                    handle.quit();
                    let _ = runner_task.await;
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(120);
                    continue;
                }
            }

            backoff_secs = 1; // reset on successful connection

            // ---- 5. Resolve channels ----
            let mut resolved: Vec<(PeerRef, &'static ChannelConfig)> = Vec::new();
            let mut peer_id_to_config: std::collections::HashMap<
                grammers_session::types::PeerId,
                &'static ChannelConfig,
            > = std::collections::HashMap::new();

            for config in CHANNELS {
                match client.resolve_username(config.username).await {
                    Ok(Some(peer)) => {
                        let peer_id = peer.id();
                        match peer.to_ref().await {
                            Some(peer_ref) => {
                                info!(
                                    channel = config.username,
                                    name = config.display_name,
                                    "Resolved Telegram channel"
                                );
                                peer_id_to_config.insert(peer_id, config);
                                resolved.push((peer_ref, config));
                            }
                            None => {
                                warn!(
                                    channel = config.username,
                                    "Could not get PeerRef for channel (missing access hash)"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        warn!(
                            channel = config.username,
                            "Channel not found"
                        );
                    }
                    Err(e) => {
                        warn!(
                            channel = config.username,
                            error = %e,
                            "Failed to resolve channel"
                        );
                    }
                }
                // Small delay between resolves to avoid flood waits
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            if resolved.is_empty() {
                warn!("No Telegram channels resolved -- nothing to monitor");
                handle.quit();
                let _ = runner_task.await;
                return Ok(());
            }

            info!(count = resolved.len(), "Resolved Telegram channels");

            // ---- 6. Smart backfill (12 hours) ----
            let recent_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM events \
                 WHERE source_type = 'telegram' \
                 AND event_time > NOW() - INTERVAL '12 hours'",
            )
            .fetch_one(&ctx.pool)
            .await
            .unwrap_or(0);

            if recent_count > 0 {
                info!(
                    recent_count,
                    "Telegram events found in last 12h, skipping backfill"
                );
            } else {
                info!("No recent Telegram events -- performing 12h backfill");
                let twelve_hours_ago = Utc::now() - chrono::Duration::hours(12);

                for (peer_ref, config) in &resolved {
                    let mut messages = client.iter_messages(*peer_ref).limit(200);
                    let mut backfill_count = 0u32;

                    loop {
                        match messages.next().await {
                            Ok(Some(msg)) => {
                                let msg_time = msg.date();
                                if msg_time < twelve_hours_ago {
                                    break;
                                }

                                let text = msg.text();
                                if text.is_empty() {
                                    continue;
                                }

                                let event = message_to_event(
                                    text,
                                    msg.id(),
                                    msg_time,
                                    config,
                                );
                                let _ = tx.send(event);
                                backfill_count += 1;
                            }
                            Ok(None) => break,
                            Err(e) => {
                                warn!(
                                    channel = config.username,
                                    error = %e,
                                    "Error during backfill"
                                );
                                break;
                            }
                        }
                    }

                    if backfill_count > 0 {
                        info!(
                            channel = config.username,
                            count = backfill_count,
                            "Backfilled messages"
                        );
                    }

                    // Avoid flood wait between channels
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }

            // ---- 7. Real-time streaming (main loop) ----
            info!("Starting Telegram real-time update stream");

            let mut update_stream = client
                .stream_updates(
                    updates,
                    UpdatesConfiguration {
                        catch_up: false,
                        update_queue_limit: Some(500),
                    },
                )
                .await;

            let mut message_count: u64 = 0;
            let mut stats_timer = tokio::time::interval(Duration::from_secs(300));
            stats_timer.tick().await; // consume first tick

            let stream_error = loop {
                tokio::select! {
                    _ = stats_timer.tick() => {
                        info!(
                            total_messages = message_count,
                            channels = resolved.len(),
                            "Telegram stream periodic stats"
                        );
                    }

                    result = update_stream.next() => {
                        match result {
                            Ok(Update::NewMessage(message)) if !message.outgoing() => {
                                // Find matching channel config by peer_id
                                let peer_id = message.peer_id();
                                let config = match peer_id_to_config.get(&peer_id) {
                                    Some(c) => c,
                                    None => {
                                        // Message from a peer we're not monitoring
                                        // (could be a private message or unrelated group)
                                        continue;
                                    }
                                };

                                let text = message.text();
                                if text.is_empty() {
                                    continue;
                                }

                                let event = message_to_event(
                                    text,
                                    message.id(),
                                    message.date(),
                                    config,
                                );

                                let _ = tx.send(event);
                                message_count += 1;

                                if message_count == 1 {
                                    info!("Telegram: first real-time message received");
                                }
                                if message_count % 100 == 0 {
                                    info!(
                                        total = message_count,
                                        "Telegram stream messages processed"
                                    );
                                }
                            }
                            Ok(_) => {
                                // Ignore other update types (edits, deletions, etc.)
                            }
                            Err(e) => {
                                break e;
                            }
                        }
                    }
                }
            };

            // Connection dropped -- save state and reconnect
            warn!(error = %stream_error, "Telegram connection lost, will reconnect");
            update_stream.sync_update_state().await;
            handle.quit();
            let _ = runner_task.await;

            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(120);
        }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_classification() {
        assert_eq!(TelegramSource::classify_severity("missile strike on target"), Severity::Critical);
        assert_eq!(TelegramSource::classify_severity("BREAKING: explosion reported"), Severity::Critical);
        assert_eq!(TelegramSource::classify_severity("IRGC forces mobilize"), Severity::Critical);
        assert_eq!(TelegramSource::classify_severity("military convoy spotted"), Severity::High);
        assert_eq!(TelegramSource::classify_severity("airstrike confirmed"), Severity::Critical); // contains "strike"
        assert_eq!(TelegramSource::classify_severity("radar contact established"), Severity::High);
        assert_eq!(TelegramSource::classify_severity("regular news update"), Severity::Medium);
    }

    #[test]
    fn test_tag_extraction() {
        let tags = TelegramSource::extract_tags("missile strike near Natanz");
        assert!(tags.contains(&"source:telegram".to_string()));
        assert!(tags.contains(&"missile".to_string()));
        assert!(tags.contains(&"strike".to_string()));
        assert!(tags.contains(&"natanz".to_string()));
    }

    #[test]
    fn test_tag_dedup() {
        let tags = TelegramSource::extract_tags("attack attack attack");
        let attack_count = tags.iter().filter(|t| *t == "attack").count();
        assert_eq!(attack_count, 1);
    }

    #[test]
    fn test_message_to_event() {
        let event = message_to_event(
            "BREAKING: missile strike reported near Natanz facility",
            12345,
            Utc::now(),
            &ChannelConfig {
                username: "test_channel",
                display_name: "Test Channel",
                region: Some("middle-east"),
                tier: 1,
            },
        );
        assert_eq!(event.source_type, SourceType::Telegram);
        assert_eq!(event.event_type, EventType::TelegramMessage);
        assert_eq!(event.severity, Severity::Critical);
        assert_eq!(event.entity_id, Some("test_channel".to_string()));
        assert_eq!(event.entity_name, Some("Test Channel".to_string()));
        assert_eq!(event.region_code, Some("middle-east".to_string()));
        assert_eq!(event.source_id, Some("tg:test_channel:12345".to_string()));
        assert!(event.title.unwrap().starts_with("[Test Channel]"));
        assert!(event.tags.contains(&"source:telegram".to_string()));
        assert!(event.tags.contains(&"missile".to_string()));
        assert!(event.tags.contains(&"strike".to_string()));
    }

    #[test]
    fn test_message_to_event_truncation() {
        let long_text = "A".repeat(600);
        let event = message_to_event(
            &long_text,
            1,
            Utc::now(),
            &ChannelConfig {
                username: "ch",
                display_name: "Ch",
                region: None,
                tier: 2,
            },
        );
        // Title has "[Ch] " prefix (5 chars) + 100 chars = 105 max
        assert!(event.title.as_ref().unwrap().len() <= 105);
        // Description truncated to 500 chars
        assert!(event.description.as_ref().unwrap().len() <= 500);
    }
}
