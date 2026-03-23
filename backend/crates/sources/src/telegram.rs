use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
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
                updates: _updates,
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

            for config in CHANNELS {
                match client.resolve_username(config.username).await {
                    Ok(Some(peer)) => {
                        match peer.to_ref().await {
                            Some(peer_ref) => {
                                info!(
                                    channel = config.username,
                                    name = config.display_name,
                                    "Resolved Telegram channel"
                                );
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

            // ---- 7. Poll-based streaming (main loop) ----
            // grammers stream_updates is unreliable for channel updates —
            // the MessageBoxes state machine frequently fails to receive
            // pushed channel updates even after iter_dialogs(). Instead,
            // we poll each channel with iter_messages on a timer, relying
            // on source_id dedup to avoid duplicates.
            info!("Starting Telegram poll-based message loop");

            // Track the newest message ID we've seen per channel to avoid
            // re-sending old messages after the first poll.
            let mut last_seen_id: std::collections::HashMap<&str, i32> =
                std::collections::HashMap::new();

            // On first poll, seed last_seen_id from what we already sent
            // (backfill or DB) so we don't duplicate. We'll just fetch the
            // latest message ID per channel without sending it.
            for (peer_ref, config) in &resolved {
                let mut messages = client.iter_messages(*peer_ref).limit(1);
                match messages.next().await {
                    Ok(Some(msg)) => {
                        last_seen_id.insert(config.username, msg.id());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(channel = config.username, error = %e, "Failed to seed last_seen_id");
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            info!(channels = last_seen_id.len(), "Seeded last-seen message IDs");

            let mut message_count: u64 = 0;
            let poll_interval = Duration::from_secs(120); // poll every 2 minutes
            let mut poll_timer = tokio::time::interval(poll_interval);
            poll_timer.tick().await; // consume first tick

            let mut stats_timer = tokio::time::interval(Duration::from_secs(300));
            stats_timer.tick().await; // consume first tick

            let mut consecutive_errors = 0u32;

            loop {
                tokio::select! {
                    _ = stats_timer.tick() => {
                        info!(
                            total_messages = message_count,
                            channels = resolved.len(),
                            "Telegram poll periodic stats"
                        );
                    }

                    _ = poll_timer.tick() => {
                        let mut poll_total = 0u32;
                        let mut poll_errors = 0u32;

                        for (peer_ref, config) in &resolved {
                            let last_id = last_seen_id.get(config.username).copied().unwrap_or(0);
                            // Fetch up to 50 recent messages
                            let mut messages = client.iter_messages(*peer_ref).limit(50);
                            let mut new_msgs: Vec<(i32, DateTime<Utc>, String)> = Vec::new();
                            let mut highest_id = last_id;

                            loop {
                                match messages.next().await {
                                    Ok(Some(msg)) => {
                                        let msg_id = msg.id();
                                        if msg_id <= last_id {
                                            // We've reached messages we already processed
                                            break;
                                        }
                                        if msg_id > highest_id {
                                            highest_id = msg_id;
                                        }
                                        let text = msg.text().to_string();
                                        if !text.is_empty() {
                                            new_msgs.push((msg_id, msg.date(), text));
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(e) => {
                                        warn!(
                                            channel = config.username,
                                            error = %e,
                                            "Error polling channel messages"
                                        );
                                        poll_errors += 1;
                                        break;
                                    }
                                }
                            }

                            if highest_id > last_id {
                                last_seen_id.insert(config.username, highest_id);
                            }

                            // Send in chronological order (oldest first)
                            new_msgs.reverse();
                            for (msg_id, msg_date, text) in &new_msgs {
                                let event = message_to_event(text, *msg_id, *msg_date, config);
                                let _ = tx.send(event);
                                message_count += 1;
                                poll_total += 1;
                            }

                            // Small delay between channels to avoid flood waits
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }

                        if poll_total > 0 {
                            info!(new_messages = poll_total, "Telegram poll cycle complete");
                        }

                        // Track connection health
                        if poll_errors >= resolved.len() as u32 {
                            // Every channel errored — connection is likely dead
                            consecutive_errors += 1;
                            if consecutive_errors >= 3 {
                                warn!(consecutive_errors, "Telegram connection appears dead, reconnecting");
                                break;
                            }
                        } else {
                            consecutive_errors = 0;
                        }
                    }
                }
            }

            // Reconnect
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
