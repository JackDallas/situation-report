use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default Bluesky list URI (OSINT accounts curated list).
const DEFAULT_LIST_URI: &str =
    "at://did:plc:36rwne5ohbpfmtz2n2c255q2/app.bsky.graph.list/3mfx7qknu6c2b";

/// Bluesky public API base for list resolution.
const BSKY_PUBLIC_API: &str = "https://public.api.bsky.app/xrpc";

/// Jetstream WebSocket base URL.
const JETSTREAM_URL: &str = "wss://jetstream2.us-east.bsky.network/subscribe";

/// Interval between keepalive Ping frames.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum number of DIDs to include in the WebSocket URL query string.
/// Jetstream supports large query strings but we cap to be safe.
const MAX_DIDS: usize = 500;

// ---------------------------------------------------------------------------
// Keyword severity classification (shared with Telegram pattern)
// ---------------------------------------------------------------------------

const CRITICAL_KEYWORDS: &[&str] = &[
    "strike", "missile", "nuclear", "IRGC", "IDF", "drone", "explosion",
    "attack", "Natanz", "Fordow", "Isfahan", "Hormuz", "Bandar Abbas",
    "BREAKING", "URGENT",
];

const HIGH_KEYWORDS: &[&str] = &[
    "military", "airstrike", "bombing", "casualties", "intercept",
    "mobilization", "escalation", "retaliation", "sanctions",
    "radar", "submarine", "warship", "convoy", "airspace",
];

// ---------------------------------------------------------------------------
// Hardcoded fallback OSINT accounts (used if list API fails)
// ---------------------------------------------------------------------------

/// Fallback DID + display name pairs for key OSINT accounts.
/// These are well-known public accounts that post conflict/security content.
const FALLBACK_ACCOUNTS: &[(&str, &str)] = &[
    ("did:plc:36rwne5ohbpfmtz2n2c255q2", "Situation Room"),
];

// ---------------------------------------------------------------------------
// Source struct
// ---------------------------------------------------------------------------

pub struct BlueskySource;

impl BlueskySource {
    pub fn new() -> Self {
        Self
    }

    /// Classify severity based on post text using keyword matching.
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

    /// Extract tags from post text based on keyword matching.
    fn extract_keyword_tags(text: &str) -> Vec<String> {
        let mut tags = vec!["source:bluesky".to_string()];
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

    /// Fetch all DIDs from a Bluesky list via the public API, paginating with cursor.
    async fn fetch_list_dids(
        http: &reqwest::Client,
        list_uri: &str,
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut did_to_name: HashMap<String, String> = HashMap::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/app.bsky.graph.getList?list={}&limit=100",
                BSKY_PUBLIC_API, list_uri
            );
            if let Some(ref c) = cursor {
                url.push_str(&format!("&cursor={}", c));
            }

            let resp = http.get(&url).send().await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "Bluesky list API returned {}: {}",
                    status,
                    body
                ));
            }

            let body: serde_json::Value = resp.json().await?;

            if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(subject) = item.get("subject") {
                        let did = subject
                            .get("did")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let display_name = subject
                            .get("displayName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let handle = subject
                            .get("handle")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        if !did.is_empty() {
                            let name = if display_name.is_empty() {
                                handle
                            } else {
                                display_name
                            };
                            did_to_name.insert(did, name);
                        }
                    }
                }
            }

            // Check for next page
            cursor = body
                .get("cursor")
                .and_then(|v| v.as_str())
                .map(String::from);
            if cursor.is_none() {
                break;
            }
        }

        Ok(did_to_name)
    }

    /// Build the Jetstream WebSocket URL with wantedDids and wantedCollections.
    fn build_ws_url(dids: &[String], cursor: Option<u64>) -> String {
        let mut url = format!(
            "{}?wantedCollections=app.bsky.feed.post",
            JETSTREAM_URL
        );

        for did in dids.iter().take(MAX_DIDS) {
            url.push_str(&format!("&wantedDids={}", did));
        }

        if let Some(c) = cursor {
            url.push_str(&format!("&cursor={}", c));
        }

        url
    }

    /// Extract facets (links, mentions, hashtags) from a post record.
    fn extract_facets(record: &serde_json::Value) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut links = Vec::new();
        let mut mentions = Vec::new();
        let mut hashtags = Vec::new();

        if let Some(facets) = record.get("facets").and_then(|v| v.as_array()) {
            for facet in facets {
                if let Some(features) = facet.get("features").and_then(|v| v.as_array()) {
                    for feature in features {
                        let ftype = feature.get("$type").and_then(|v| v.as_str()).unwrap_or("");
                        match ftype {
                            "app.bsky.richtext.facet#link" => {
                                if let Some(uri) = feature.get("uri").and_then(|v| v.as_str()) {
                                    links.push(uri.to_string());
                                }
                            }
                            "app.bsky.richtext.facet#mention" => {
                                if let Some(did) = feature.get("did").and_then(|v| v.as_str()) {
                                    mentions.push(did.to_string());
                                }
                            }
                            "app.bsky.richtext.facet#tag" => {
                                if let Some(tag) = feature.get("tag").and_then(|v| v.as_str()) {
                                    hashtags.push(tag.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        (links, mentions, hashtags)
    }

    /// Extract embed information from a post record.
    fn extract_embed(record: &serde_json::Value) -> (serde_json::Value, Vec<String>) {
        let mut embed_data = serde_json::json!({});
        let mut content_tags = Vec::new();

        if let Some(embed) = record.get("embed") {
            let embed_type = embed.get("$type").and_then(|v| v.as_str()).unwrap_or("");

            match embed_type {
                "app.bsky.embed.images" => {
                    content_tags.push("has_images".to_string());
                    if let Some(images) = embed.get("images").and_then(|v| v.as_array()) {
                        let alt_texts: Vec<&str> = images
                            .iter()
                            .filter_map(|img| img.get("alt").and_then(|v| v.as_str()))
                            .filter(|alt| !alt.is_empty())
                            .collect();
                        let image_count = images.len();
                        embed_data = serde_json::json!({
                            "type": "images",
                            "count": image_count,
                            "alt_texts": alt_texts,
                        });
                    }
                }
                "app.bsky.embed.external" => {
                    if let Some(external) = embed.get("external") {
                        let url = external.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                        let ext_title = external
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let ext_desc = external
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        content_tags.push("has_link".to_string());
                        embed_data = serde_json::json!({
                            "type": "external",
                            "url": url,
                            "title": ext_title,
                            "description": ext_desc,
                        });
                    }
                }
                "app.bsky.embed.record" => {
                    content_tags.push("is_quote".to_string());
                    if let Some(rec) = embed.get("record") {
                        let uri = rec.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                        embed_data = serde_json::json!({
                            "type": "quote",
                            "quoted_uri": uri,
                        });
                    }
                }
                "app.bsky.embed.recordWithMedia" => {
                    content_tags.push("is_quote".to_string());
                    // Handle the record part
                    if let Some(rec) = embed.get("record").and_then(|r| r.get("record")) {
                        let uri = rec.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                        embed_data["quoted_uri"] =
                            serde_json::Value::String(uri.to_string());
                    }
                    // Handle the media part
                    if let Some(media) = embed.get("media") {
                        let media_type =
                            media.get("$type").and_then(|v| v.as_str()).unwrap_or("");
                        if media_type == "app.bsky.embed.images" {
                            content_tags.push("has_images".to_string());
                        } else if media_type == "app.bsky.embed.video" {
                            content_tags.push("has_video".to_string());
                        }
                    }
                    embed_data["type"] =
                        serde_json::Value::String("record_with_media".to_string());
                }
                "app.bsky.embed.video" => {
                    content_tags.push("has_video".to_string());
                    embed_data = serde_json::json!({
                        "type": "video",
                    });
                }
                _ => {}
            }
        }

        (embed_data, content_tags)
    }

    /// Process a Jetstream commit event into an InsertableEvent.
    fn process_commit(
        msg: &serde_json::Value,
        did_to_name: &HashMap<String, String>,
    ) -> Option<InsertableEvent> {
        // Jetstream commit message structure:
        // { "kind": "commit", "did": "...", "commit": { "collection": "...", "rkey": "...", "record": {...} }, "time_us": ... }
        let kind = msg.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "commit" {
            return None;
        }

        let did = msg.get("did").and_then(|v| v.as_str())?;
        let commit = msg.get("commit")?;

        let operation = commit.get("operation").and_then(|v| v.as_str()).unwrap_or("");
        if operation != "create" {
            // We only care about new posts, not updates or deletes
            return None;
        }

        let collection = commit
            .get("collection")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if collection != "app.bsky.feed.post" {
            return None;
        }

        let rkey = commit.get("rkey").and_then(|v| v.as_str()).unwrap_or("");
        let record = commit.get("record")?;

        // Extract post text
        let text = record.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if text.is_empty() {
            return None;
        }

        // Event time from record.createdAt, falling back to Jetstream time_us
        let event_time = record
            .get("createdAt")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| {
                msg.get("time_us")
                    .and_then(|v| v.as_i64())
                    .and_then(|us| {
                        DateTime::from_timestamp(us / 1_000_000, ((us % 1_000_000) * 1000) as u32)
                    })
                    .unwrap_or_else(Utc::now)
            });

        // Account info
        let display_name = did_to_name
            .get(did)
            .cloned()
            .unwrap_or_else(|| did.to_string());

        // source_id for dedup
        let source_id = format!("bsky:{}:{}", did, rkey);

        // Title: [AccountName] first 100 chars
        let truncated_title: String = text.chars().take(100).collect();
        let title = format!("[{}] {}", display_name, truncated_title);

        // Description: up to 500 chars
        let description: String = text.chars().take(500).collect();

        // Severity from keywords
        let severity = Self::classify_severity(text);

        // Tags
        let mut tags = Self::extract_keyword_tags(text);

        // Facets (links, mentions, hashtags)
        let (links, _mentions, hashtags) = Self::extract_facets(record);
        for ht in &hashtags {
            let tag = format!("#{}", ht.to_lowercase());
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }

        // Embed info
        let (embed_data, content_tags) = Self::extract_embed(record);
        for ct in &content_tags {
            if !tags.contains(ct) {
                tags.push(ct.clone());
            }
        }

        // Reply detection
        let is_reply = record.get("reply").is_some();
        if is_reply {
            if !tags.contains(&"is_reply".to_string()) {
                tags.push("is_reply".to_string());
            }
        }

        // Languages
        let langs: Vec<String> = record
            .get("langs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for lang in &langs {
            let lang_tag = format!("lang:{}", lang);
            if !tags.contains(&lang_tag) {
                tags.push(lang_tag);
            }
        }

        // Build payload
        let mut payload = serde_json::json!({
            "did": did,
            "rkey": rkey,
            "text": text,
            "account_name": display_name,
        });

        if !links.is_empty() {
            payload["links"] = serde_json::json!(links);
        }
        if !hashtags.is_empty() {
            payload["hashtags"] = serde_json::json!(hashtags);
        }
        if !langs.is_empty() {
            payload["langs"] = serde_json::json!(langs);
        }
        if embed_data != serde_json::json!({}) {
            payload["embed"] = embed_data;
        }
        if is_reply {
            if let Some(reply) = record.get("reply") {
                let root_uri = reply
                    .get("root")
                    .and_then(|r| r.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let parent_uri = reply
                    .get("parent")
                    .and_then(|p| p.get("uri"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                payload["reply"] = serde_json::json!({
                    "root_uri": root_uri,
                    "parent_uri": parent_uri,
                });
            }
        }

        // Construct post URI for payload
        let post_uri = format!("at://{}/app.bsky.feed.post/{}", did, rkey);
        payload["uri"] = serde_json::Value::String(post_uri);

        Some(InsertableEvent {
            event_time,
            source_type: SourceType::Bluesky,
            source_id: Some(source_id),
            longitude: None,
            latitude: None,
            region_code: None,
            entity_id: Some(did.to_string()),
            entity_name: Some(display_name),
            event_type: EventType::BlueskyPost,
            severity,
            confidence: None,
            tags,
            title: Some(title),
            description: Some(description),
            payload,
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

impl Default for BlueskySource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for BlueskySource {
    fn id(&self) -> &str {
        "bluesky"
    }

    fn name(&self) -> &str {
        "Bluesky OSINT"
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
        ctx: &SourceContext,
        tx: broadcast::Sender<InsertableEvent>,
    ) -> anyhow::Result<()> {
        let list_uri = std::env::var("BLUESKY_LIST_URI")
            .unwrap_or_else(|_| DEFAULT_LIST_URI.to_string());

        // ---- 1. Resolve list members to get DIDs ----
        info!(list_uri = %list_uri, "Fetching Bluesky list members");

        let did_to_name = match Self::fetch_list_dids(&ctx.http, &list_uri).await {
            Ok(map) if !map.is_empty() => {
                info!(count = map.len(), "Resolved Bluesky list members");
                map
            }
            Ok(_) => {
                warn!("Bluesky list returned 0 members, using fallback accounts");
                FALLBACK_ACCOUNTS
                    .iter()
                    .map(|(did, name)| (did.to_string(), name.to_string()))
                    .collect()
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to fetch Bluesky list, using fallback accounts"
                );
                FALLBACK_ACCOUNTS
                    .iter()
                    .map(|(did, name)| (did.to_string(), name.to_string()))
                    .collect()
            }
        };

        if did_to_name.is_empty() {
            warn!("No Bluesky accounts to monitor -- source disabled");
            return Ok(());
        }

        let dids: Vec<String> = did_to_name.keys().cloned().collect();
        info!(
            accounts = dids.len(),
            "Starting Bluesky Jetstream connection"
        );

        // ---- 2. Connect to Jetstream with reconnection loop ----
        let mut backoff_secs = 1u64;
        let mut last_cursor: Option<u64> = None;

        loop {
            let ws_url = Self::build_ws_url(&dids, last_cursor);
            debug!(url = %ws_url, "Connecting to Jetstream");

            let (ws_stream, _response) = match connect_async(&ws_url).await {
                Ok(conn) => {
                    backoff_secs = 1; // reset on successful connect
                    conn
                }
                Err(e) => {
                    error!(error = %e, backoff_secs, "Failed to connect to Jetstream");
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(120);
                    continue;
                }
            };

            info!("Connected to Bluesky Jetstream");

            let (_write, mut read) = ws_stream.split();

            let mut message_count: u64 = 0;
            let mut raw_count: u64 = 0;
            let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
            keepalive.tick().await; // consume immediate first tick
            let mut stats_timer = tokio::time::interval(Duration::from_secs(300));
            stats_timer.tick().await;

            let disconnect_reason: String = loop {
                tokio::select! {
                    _ = stats_timer.tick() => {
                        info!(
                            raw_messages = raw_count,
                            processed_posts = message_count,
                            accounts = dids.len(),
                            "Bluesky stream periodic stats"
                        );
                    }

                    msg_opt = read.next() => {
                        let msg_result = match msg_opt {
                            Some(r) => r,
                            None => {
                                break "Jetstream ended unexpectedly".to_string();
                            }
                        };

                        let msg = match msg_result {
                            Ok(m) => m,
                            Err(e) => {
                                break format!("WebSocket read error: {}", e);
                            }
                        };

                        let text = match msg {
                            tokio_tungstenite::tungstenite::Message::Text(t) => t,
                            tokio_tungstenite::tungstenite::Message::Ping(_) => {
                                debug!("Bluesky: received Ping from server");
                                // tokio-tungstenite auto-responds to Pings
                                continue;
                            }
                            tokio_tungstenite::tungstenite::Message::Pong(_) => {
                                continue;
                            }
                            tokio_tungstenite::tungstenite::Message::Close(_) => {
                                break "Jetstream WebSocket closed by server".to_string();
                            }
                            _ => continue,
                        };

                        raw_count += 1;

                        let parsed: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(e) => {
                                debug!(error = %e, "Failed to parse Jetstream message");
                                continue;
                            }
                        };

                        // Track cursor for reconnection (unix microseconds)
                        if let Some(time_us) = parsed.get("time_us").and_then(|v| v.as_u64()) {
                            last_cursor = Some(time_us);
                        }

                        if let Some(event) = Self::process_commit(&parsed, &did_to_name) {
                            let _ = tx.send(event);
                            message_count += 1;

                            if message_count == 1 {
                                info!("Bluesky stream: first post received and broadcast");
                            }
                            if message_count % 100 == 0 {
                                info!(
                                    total = message_count,
                                    "Bluesky stream: posts processed"
                                );
                            }
                        }
                    }

                    _ = keepalive.tick() => {
                        // Jetstream is read-only; no keepalive ping needed from client.
                        // The server sends periodic heartbeat messages.
                        // We just use this as an activity check.
                    }
                }
            };

            warn!(
                reason = %disconnect_reason,
                backoff_secs,
                "Bluesky Jetstream disconnected, reconnecting"
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(120);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_classification() {
        assert_eq!(
            BlueskySource::classify_severity("missile strike on target"),
            Severity::Critical
        );
        assert_eq!(
            BlueskySource::classify_severity("BREAKING: explosion reported"),
            Severity::Critical
        );
        assert_eq!(
            BlueskySource::classify_severity("military convoy spotted"),
            Severity::High
        );
        assert_eq!(
            BlueskySource::classify_severity("regular update about weather"),
            Severity::Medium
        );
    }

    #[test]
    fn test_keyword_tags() {
        let tags = BlueskySource::extract_keyword_tags("missile strike near Natanz");
        assert!(tags.contains(&"source:bluesky".to_string()));
        assert!(tags.contains(&"missile".to_string()));
        assert!(tags.contains(&"strike".to_string()));
        assert!(tags.contains(&"natanz".to_string()));
    }

    #[test]
    fn test_tag_dedup() {
        let tags = BlueskySource::extract_keyword_tags("attack attack attack");
        let attack_count = tags.iter().filter(|t| *t == "attack").count();
        assert_eq!(attack_count, 1);
    }

    #[test]
    fn test_extract_facets() {
        let record = serde_json::json!({
            "text": "Check this out @user",
            "facets": [
                {
                    "index": {"byteStart": 0, "byteEnd": 14},
                    "features": [
                        {"$type": "app.bsky.richtext.facet#link", "uri": "https://example.com"}
                    ]
                },
                {
                    "index": {"byteStart": 15, "byteEnd": 20},
                    "features": [
                        {"$type": "app.bsky.richtext.facet#mention", "did": "did:plc:abc123"}
                    ]
                },
                {
                    "index": {"byteStart": 21, "byteEnd": 30},
                    "features": [
                        {"$type": "app.bsky.richtext.facet#tag", "tag": "OSINT"}
                    ]
                }
            ]
        });

        let (links, mentions, hashtags) = BlueskySource::extract_facets(&record);
        assert_eq!(links, vec!["https://example.com"]);
        assert_eq!(mentions, vec!["did:plc:abc123"]);
        assert_eq!(hashtags, vec!["OSINT"]);
    }

    #[test]
    fn test_extract_embed_images() {
        let record = serde_json::json!({
            "text": "Photo",
            "embed": {
                "$type": "app.bsky.embed.images",
                "images": [
                    {"alt": "Satellite image", "image": {}},
                    {"alt": "", "image": {}}
                ]
            }
        });

        let (data, tags) = BlueskySource::extract_embed(&record);
        assert!(tags.contains(&"has_images".to_string()));
        assert_eq!(data["count"], 2);
        assert_eq!(data["alt_texts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_extract_embed_external() {
        let record = serde_json::json!({
            "text": "Link post",
            "embed": {
                "$type": "app.bsky.embed.external",
                "external": {
                    "uri": "https://example.com/article",
                    "title": "Article Title",
                    "description": "Article desc"
                }
            }
        });

        let (data, tags) = BlueskySource::extract_embed(&record);
        assert!(tags.contains(&"has_link".to_string()));
        assert_eq!(data["url"], "https://example.com/article");
        assert_eq!(data["title"], "Article Title");
    }

    #[test]
    fn test_extract_embed_quote() {
        let record = serde_json::json!({
            "text": "Quote post",
            "embed": {
                "$type": "app.bsky.embed.record",
                "record": {
                    "uri": "at://did:plc:abc/app.bsky.feed.post/xyz"
                }
            }
        });

        let (data, tags) = BlueskySource::extract_embed(&record);
        assert!(tags.contains(&"is_quote".to_string()));
        assert_eq!(data["quoted_uri"], "at://did:plc:abc/app.bsky.feed.post/xyz");
    }

    #[test]
    fn test_extract_embed_video() {
        let record = serde_json::json!({
            "text": "Video post",
            "embed": {
                "$type": "app.bsky.embed.video"
            }
        });

        let (_, tags) = BlueskySource::extract_embed(&record);
        assert!(tags.contains(&"has_video".to_string()));
    }

    #[test]
    fn test_process_commit_create() {
        let mut did_map = HashMap::new();
        did_map.insert(
            "did:plc:testuser".to_string(),
            "Test OSINT".to_string(),
        );

        let msg = serde_json::json!({
            "kind": "commit",
            "did": "did:plc:testuser",
            "time_us": 1710000000000000_u64,
            "commit": {
                "operation": "create",
                "collection": "app.bsky.feed.post",
                "rkey": "abc123",
                "record": {
                    "text": "BREAKING: missile strike reported in the region",
                    "createdAt": "2026-03-10T12:00:00Z",
                    "langs": ["en"],
                    "facets": [
                        {
                            "features": [
                                {"$type": "app.bsky.richtext.facet#tag", "tag": "OSINT"}
                            ]
                        }
                    ]
                }
            }
        });

        let event = BlueskySource::process_commit(&msg, &did_map).unwrap();
        assert_eq!(event.source_type, SourceType::Bluesky);
        assert_eq!(event.event_type, EventType::BlueskyPost);
        assert_eq!(event.severity, Severity::Critical); // "missile" + "strike" + "BREAKING"
        assert_eq!(
            event.source_id,
            Some("bsky:did:plc:testuser:abc123".to_string())
        );
        assert_eq!(event.entity_id, Some("did:plc:testuser".to_string()));
        assert_eq!(event.entity_name, Some("Test OSINT".to_string()));
        assert!(event.title.unwrap().starts_with("[Test OSINT]"));
        assert!(event.tags.contains(&"source:bluesky".to_string()));
        assert!(event.tags.contains(&"missile".to_string()));
        assert!(event.tags.contains(&"#osint".to_string()));
        assert!(event.tags.contains(&"lang:en".to_string()));
    }

    #[test]
    fn test_process_commit_ignores_delete() {
        let did_map = HashMap::new();
        let msg = serde_json::json!({
            "kind": "commit",
            "did": "did:plc:testuser",
            "commit": {
                "operation": "delete",
                "collection": "app.bsky.feed.post",
                "rkey": "abc123"
            }
        });

        assert!(BlueskySource::process_commit(&msg, &did_map).is_none());
    }

    #[test]
    fn test_process_commit_ignores_non_post() {
        let did_map = HashMap::new();
        let msg = serde_json::json!({
            "kind": "commit",
            "did": "did:plc:testuser",
            "commit": {
                "operation": "create",
                "collection": "app.bsky.graph.follow",
                "rkey": "abc123",
                "record": {}
            }
        });

        assert!(BlueskySource::process_commit(&msg, &did_map).is_none());
    }

    #[test]
    fn test_process_commit_reply_detection() {
        let mut did_map = HashMap::new();
        did_map.insert("did:plc:testuser".to_string(), "Test".to_string());

        let msg = serde_json::json!({
            "kind": "commit",
            "did": "did:plc:testuser",
            "time_us": 1710000000000000_u64,
            "commit": {
                "operation": "create",
                "collection": "app.bsky.feed.post",
                "rkey": "reply123",
                "record": {
                    "text": "I agree with this assessment",
                    "createdAt": "2026-03-10T12:00:00Z",
                    "reply": {
                        "root": {"uri": "at://did:plc:other/app.bsky.feed.post/root1"},
                        "parent": {"uri": "at://did:plc:other/app.bsky.feed.post/parent1"}
                    }
                }
            }
        });

        let event = BlueskySource::process_commit(&msg, &did_map).unwrap();
        assert!(event.tags.contains(&"is_reply".to_string()));
        let reply = event.payload.get("reply").unwrap();
        assert_eq!(
            reply["root_uri"],
            "at://did:plc:other/app.bsky.feed.post/root1"
        );
        assert_eq!(
            reply["parent_uri"],
            "at://did:plc:other/app.bsky.feed.post/parent1"
        );
    }

    #[test]
    fn test_build_ws_url_basic() {
        let dids = vec!["did:plc:abc".to_string(), "did:plc:def".to_string()];
        let url = BlueskySource::build_ws_url(&dids, None);
        assert!(url.starts_with("wss://jetstream2.us-east.bsky.network/subscribe"));
        assert!(url.contains("wantedCollections=app.bsky.feed.post"));
        assert!(url.contains("wantedDids=did:plc:abc"));
        assert!(url.contains("wantedDids=did:plc:def"));
        assert!(!url.contains("cursor="));
    }

    #[test]
    fn test_build_ws_url_with_cursor() {
        let dids = vec!["did:plc:abc".to_string()];
        let url = BlueskySource::build_ws_url(&dids, Some(1710000000000000));
        assert!(url.contains("cursor=1710000000000000"));
    }

    #[test]
    fn test_description_truncation() {
        let mut did_map = HashMap::new();
        did_map.insert("did:plc:test".to_string(), "T".to_string());

        let long_text = "A".repeat(600);
        let msg = serde_json::json!({
            "kind": "commit",
            "did": "did:plc:test",
            "time_us": 1710000000000000_u64,
            "commit": {
                "operation": "create",
                "collection": "app.bsky.feed.post",
                "rkey": "trunc1",
                "record": {
                    "text": long_text,
                    "createdAt": "2026-03-10T12:00:00Z"
                }
            }
        });

        let event = BlueskySource::process_commit(&msg, &did_map).unwrap();
        // Title: "[T] " (4 chars) + 100 chars = 104 max
        assert!(event.title.as_ref().unwrap().len() <= 104);
        // Description truncated to 500
        assert!(event.description.as_ref().unwrap().len() <= 500);
    }
}
