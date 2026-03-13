use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};
use crate::common;

/// Browser-like User-Agent for RSS fetches. Some feeds (breakingdefense.com,
/// warontherocks.com) return 403 when they see a bot-style UA string.
const RSS_USER_AGENT: &str =
    "Mozilla/5.0 (compatible; SituationReport/1.0; +https://github.com)";

/// An RSS/Atom feed definition.
struct Feed {
    id: &'static str,
    url: &'static str,
    source_name: &'static str,
    language: &'static str,
    region: Option<&'static str>,
}

/// Curated news feeds for global security/intelligence monitoring.
/// Verified working as of March 2026.
const FEEDS: &[Feed] = &[
    // --- Wire services ---
    Feed { id: "bbc-world", url: "https://feeds.bbci.co.uk/news/world/rss.xml", source_name: "BBC", language: "en", region: None },
    Feed { id: "guardian-world", url: "https://www.theguardian.com/world/rss", source_name: "Guardian", language: "en", region: None },
    Feed { id: "france24", url: "https://www.france24.com/en/rss", source_name: "France 24", language: "en", region: None },
    Feed { id: "dw", url: "https://rss.dw.com/rdf/rss-en-all", source_name: "Deutsche Welle", language: "en", region: None },
    // --- Middle East ---
    Feed { id: "aljazeera-en", url: "https://www.aljazeera.com/xml/rss/all.xml", source_name: "Al Jazeera", language: "en", region: Some("ME") },
    Feed { id: "middleeasteye", url: "https://www.middleeasteye.net/rss", source_name: "Middle East Eye", language: "en", region: Some("ME") },
    Feed { id: "timesofisrael", url: "https://www.timesofisrael.com/feed/", source_name: "Times of Israel", language: "en", region: Some("ME") },
    // --- Russia / Eastern Europe ---
    Feed { id: "meduza", url: "https://meduza.io/rss/en/all", source_name: "Meduza", language: "en", region: Some("RU") },
    Feed { id: "moscowtimes", url: "https://www.themoscowtimes.com/rss/news", source_name: "Moscow Times", language: "en", region: Some("RU") },
    // --- Asia-Pacific ---
    Feed { id: "scmp", url: "https://www.scmp.com/rss/91/feed", source_name: "SCMP", language: "en", region: Some("AS") },
    Feed { id: "yonhap", url: "https://en.yna.co.kr/RSS/news.xml", source_name: "Yonhap", language: "en", region: Some("AS") },
    // --- Africa ---
    Feed { id: "allafrica", url: "https://allafrica.com/tools/headlines/rdf/latest/headlines.rdf", source_name: "AllAfrica", language: "en", region: Some("AF") },
    // --- Defense / Military ---
    Feed { id: "warontherocks", url: "https://warontherocks.com/feed/", source_name: "War on the Rocks", language: "en", region: None },
    Feed { id: "breakingdefense", url: "https://breakingdefense.com/feed/", source_name: "Breaking Defense", language: "en", region: None },
    Feed { id: "usni", url: "https://news.usni.org/feed", source_name: "USNI News", language: "en", region: None },
    // --- Conflict / OSINT ---
    Feed { id: "bellingcat", url: "https://www.bellingcat.com/feed/", source_name: "Bellingcat", language: "en", region: None },
    Feed { id: "crisisgroup", url: "https://www.crisisgroup.org/rss.xml", source_name: "Crisis Group", language: "en", region: None },
    // --- Cyber / Threat Intel ---
    Feed { id: "hackernews", url: "https://feeds.feedburner.com/TheHackersNews", source_name: "The Hacker News", language: "en", region: None },
    Feed { id: "bleepingcomputer", url: "https://www.bleepingcomputer.com/feed/", source_name: "Bleeping Computer", language: "en", region: None },
    Feed { id: "krebsonsecurity", url: "https://krebsonsecurity.com/feed/", source_name: "Krebs on Security", language: "en", region: None },
    Feed { id: "therecord", url: "https://therecord.media/feed/", source_name: "The Record", language: "en", region: None },
    // --- Humanitarian ---
    Feed { id: "reliefweb", url: "https://reliefweb.int/updates/rss.xml", source_name: "ReliefWeb", language: "en", region: None },
    Feed { id: "un-news", url: "https://news.un.org/feed/subscribe/en/news/all/rss.xml", source_name: "UN News", language: "en", region: None },
    // --- Nuclear / Arms ---
    Feed { id: "world-nuclear-news", url: "https://world-nuclear-news.org/rss", source_name: "World Nuclear News", language: "en", region: None },
    Feed { id: "armscontrol", url: "https://www.armscontrol.org/rss.xml", source_name: "Arms Control Assoc", language: "en", region: None },
    // --- Maritime ---
    Feed { id: "gcaptain", url: "https://gcaptain.com/feed/", source_name: "gCaptain", language: "en", region: None },
];

/// How many feeds to poll per cycle (rotate through all feeds).
const FEEDS_PER_POLL: usize = 3;

pub struct RssNewsSource {
    feed_index: Mutex<usize>,
    /// Two-buffer dedup: check both, insert into current, rotate when current exceeds 5K.
    /// This ensures we always remember the last 5K-10K GUIDs with smooth eviction
    /// instead of losing all history at once.
    seen_current: Mutex<std::collections::HashSet<String>>,
    seen_previous: Mutex<std::collections::HashSet<String>>,
}

impl Default for RssNewsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl RssNewsSource {
    pub fn new() -> Self {
        Self {
            feed_index: Mutex::new(0),
            seen_current: Mutex::new(std::collections::HashSet::new()),
            seen_previous: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Extract text between XML open/close tags (simple, no attributes on target).
    fn extract_tag(xml: &str, tag: &str) -> Option<String> {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let start = xml.find(&open)?;
        // Find the end of the opening tag (could have attributes)
        let content_start = xml[start..].find('>')? + start + 1;
        // Handle CDATA
        let content_end = xml[content_start..].find(&close)? + content_start;
        let content = &xml[content_start..content_end];

        // Strip CDATA wrapper if present
        let content = content.trim();
        let content = if content.starts_with("<![CDATA[") && content.ends_with("]]>") {
            &content[9..content.len() - 3]
        } else {
            content
        };

        // Decode basic HTML entities
        let decoded = content
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'");

        if decoded.is_empty() {
            None
        } else {
            Some(decoded)
        }
    }

    /// Parse RFC 2822 date (common in RSS) into DateTime<Utc>.
    fn parse_pub_date(raw: &str) -> Option<DateTime<Utc>> {
        let trimmed = raw.trim();
        // Try RFC 2822 first (normalize timezone names)
        let normalized = trimmed
            .replace(" GMT", " +0000")
            .replace(" UTC", " +0000")
            .replace(" EST", " -0500")
            .replace(" EDT", " -0400")
            .replace(" PST", " -0800")
            .replace(" PDT", " -0700");
        if let Ok(dt) = DateTime::parse_from_rfc2822(&normalized) {
            return Some(dt.with_timezone(&Utc));
        }
        // Try common RSS date format explicitly
        // "Sat, 01 Mar 2026 12:00:00 +0000"
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(
            &normalized.split(',').last().unwrap_or(&normalized).trim().replace(" +0000", ""),
            "%d %b %Y %H:%M:%S",
        ) {
            return Some(dt.and_utc());
        }
        // Try ISO 8601 / RFC 3339 (Atom feeds)
        if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
            return Some(dt.with_timezone(&Utc));
        }
        None
    }

    fn parse_feed(body: &str, feed: &Feed) -> Vec<RssItem> {
        let mut items = Vec::new();
        let mut search_from = 0;

        loop {
            let item_start = match body[search_from..].find("<item") {
                Some(pos) => search_from + pos,
                None => {
                    // Try Atom <entry> format
                    match body[search_from..].find("<entry") {
                        Some(pos) => search_from + pos,
                        None => break,
                    }
                }
            };

            let item_end_tag = if body[item_start..].contains("</item>") {
                "</item>"
            } else {
                "</entry>"
            };

            let item_end = match body[item_start..].find(item_end_tag) {
                Some(pos) => item_start + pos + item_end_tag.len(),
                None => break,
            };

            let item_xml = &body[item_start..item_end];

            let title = Self::extract_tag(item_xml, "title");
            let link = Self::extract_tag(item_xml, "link")
                .or_else(|| {
                    // Atom uses <link href="..."/>
                    let link_start = item_xml.find("<link")?;
                    let tag_end = item_xml[link_start..].find('>')?;
                    let tag = &item_xml[link_start..link_start + tag_end];
                    let href_start = tag.find("href=\"")? + 6;
                    let href_end = tag[href_start..].find('"')? + href_start;
                    Some(tag[href_start..href_end].to_string())
                });
            let description = Self::extract_tag(item_xml, "description")
                .or_else(|| Self::extract_tag(item_xml, "summary"))
                .or_else(|| Self::extract_tag(item_xml, "content"));
            let pub_date = Self::extract_tag(item_xml, "pubDate")
                .or_else(|| Self::extract_tag(item_xml, "published"))
                .or_else(|| Self::extract_tag(item_xml, "updated"));
            let guid = Self::extract_tag(item_xml, "guid")
                .or_else(|| Self::extract_tag(item_xml, "id"))
                .or_else(|| link.clone());

            if let Some(ref link_val) = link {
                items.push(RssItem {
                    title: title.unwrap_or_default(),
                    link: link_val.clone(),
                    description: description.unwrap_or_default(),
                    pub_date,
                    guid: guid.unwrap_or_else(|| link_val.clone()),
                    feed_id: feed.id,
                    source_name: feed.source_name,
                    language: feed.language,
                    region: feed.region,
                });
            }

            search_from = item_end;
        }

        items
    }
}

struct RssItem {
    title: String,
    link: String,
    description: String,
    pub_date: Option<String>,
    guid: String,
    feed_id: &'static str,
    source_name: &'static str,
    language: &'static str,
    region: Option<&'static str>,
}

#[async_trait]
impl DataSource for RssNewsSource {
    fn id(&self) -> &str {
        "rss-news"
    }

    fn name(&self) -> &str {
        "RSS News Feeds"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(5 * 60) // 5 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        // Pick the next FEEDS_PER_POLL feeds to poll
        let feeds_to_poll: Vec<usize> = {
            let mut idx = self.feed_index.lock().unwrap_or_else(|e| e.into_inner());
            let mut indices = Vec::new();
            for _ in 0..FEEDS_PER_POLL {
                indices.push(*idx % FEEDS.len());
                *idx = (*idx + 1) % FEEDS.len();
            }
            indices
        };

        let mut all_events = Vec::new();

        for feed_idx in feeds_to_poll {
            let feed = &FEEDS[feed_idx];
            debug!(feed_id = feed.id, "Polling RSS feed");

            let resp = match ctx.http.get(feed.url)
                .header(reqwest::header::USER_AGENT, RSS_USER_AGENT)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(feed_id = feed.id, error = %e, "RSS fetch failed");
                    continue;
                }
            };

            if !resp.status().is_success() {
                warn!(feed_id = feed.id, status = %resp.status(), "RSS feed returned error");
                continue;
            }

            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    warn!(feed_id = feed.id, error = %e, "Failed to read RSS body");
                    continue;
                }
            };

            let items = Self::parse_feed(&body, feed);
            let mut new_count = 0;

            for item in items {
                // Dedup by guid — check both current and previous buffers
                {
                    let mut current = self.seen_current.lock().unwrap_or_else(|e| e.into_inner());
                    let previous = self.seen_previous.lock().unwrap_or_else(|e| e.into_inner());
                    if current.contains(&item.guid) || previous.contains(&item.guid) {
                        continue;
                    }
                    drop(previous);
                    current.insert(item.guid.clone());
                    // Rotate: when current exceeds 5K, move it to previous and start fresh
                    if current.len() > 5_000 {
                        let full = std::mem::replace(&mut *current, std::collections::HashSet::new());
                        current.insert(item.guid.clone());
                        drop(current);
                        let mut prev = self.seen_previous.lock().unwrap_or_else(|e| e.into_inner());
                        *prev = full;
                    }
                }

                let event_time = item.pub_date
                    .as_deref()
                    .and_then(Self::parse_pub_date)
                    .unwrap_or_else(Utc::now);

                // Truncate long descriptions for payload.
                // Use char-boundary-safe truncation to avoid panic on multi-byte UTF-8.
                let desc_truncated = if item.description.len() > 2000 {
                    let mut end = 2000;
                    while !item.description.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    item.description[..end].to_string()
                } else {
                    item.description.clone()
                };

                let payload = serde_json::json!({
                    "title": item.title,
                    "url": item.link,
                    "description": desc_truncated,
                    "feed_id": item.feed_id,
                    "source_name": item.source_name,
                    "language": item.language,
                    "guid": item.guid,
                });

                let mut tags = vec![format!("source:{}", item.source_name)];
                if item.language != "en" {
                    tags.push(item.language.to_string());
                }

                // Approximate coordinates from feed region code
                let (latitude, longitude) = item.region
                    .and_then(|r| {
                        // Try long-form region name first
                        common::region_center(r)
                            // Then try normalizing abbreviated codes
                            .or_else(|| {
                                let normalized = match r {
                                    "ME" => "middle-east",
                                    "EE" => "eastern-europe",
                                    "AF" => "africa",
                                    "SEA" => "southeast-asia",
                                    "EA" => "east-asia",
                                    "AS" => "east-asia",
                                    _ => r,
                                };
                                common::region_center(normalized)
                            })
                            // Then try as country code
                            .or_else(|| common::country_center(r))
                    })
                    .map(|(lat, lon)| (Some(lat), Some(lon)))
                    .unwrap_or((None, None));

                all_events.push(InsertableEvent {
                    event_time,
                    source_type: SourceType::RssNews,
                    source_id: Some(item.link.clone()),
                    longitude,
                    latitude,
                    region_code: item.region.map(String::from),
                    entity_id: None,
                    entity_name: None,
                    event_type: EventType::NewsArticle,
                    severity: Severity::Low,
                    confidence: None,
                    tags,
                    title: if item.title.is_empty() { None } else { Some(item.title) },
                    description: if item.link.is_empty() { None } else { Some(item.link) },
                    payload,
                    heading: None,
                    speed: None,
                    altitude: None,
                });

                new_count += 1;
            }

            debug!(feed_id = feed.id, new_count, "RSS feed processed");
        }

        Ok(all_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rss_item() {
        let xml = r#"<rss><channel><item>
            <title>Test Article Title</title>
            <link>https://example.com/article/1</link>
            <description>This is a test article description.</description>
            <pubDate>Sat, 01 Mar 2026 12:00:00 GMT</pubDate>
            <guid>https://example.com/article/1</guid>
        </item></channel></rss>"#;

        let feed = Feed {
            id: "test",
            url: "https://example.com/rss",
            source_name: "Test",
            language: "en",
            region: None,
        };

        let items = RssNewsSource::parse_feed(xml, &feed);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Test Article Title");
        assert_eq!(items[0].link, "https://example.com/article/1");
    }

    #[test]
    fn test_parse_cdata() {
        let xml = r#"<item>
            <title><![CDATA[Breaking: Major Event]]></title>
            <link>https://example.com/2</link>
            <description><![CDATA[<p>Details here</p>]]></description>
            <pubDate>Sat, 01 Mar 2026 14:30:00 +0000</pubDate>
        </item>"#;

        let feed = Feed {
            id: "test",
            url: "https://example.com/rss",
            source_name: "Test",
            language: "en",
            region: None,
        };

        let items = RssNewsSource::parse_feed(xml, &feed);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Breaking: Major Event");
    }

    #[test]
    fn test_parse_pub_date() {
        assert!(RssNewsSource::parse_pub_date("Sat, 01 Mar 2026 12:00:00 GMT").is_some());
        assert!(RssNewsSource::parse_pub_date("Sat, 01 Mar 2026 12:00:00 +0000").is_some());
        assert!(RssNewsSource::parse_pub_date("garbage").is_none());
    }

    #[test]
    fn test_description_truncation_multibyte_utf8() {
        // Build a string where byte 2000 falls in the middle of a multi-byte char.
        // U+00E9 (e-acute) is 2 bytes in UTF-8: 0xC3 0xA9
        // 1999 ASCII chars + one 2-byte char = 2001 bytes, byte 2000 is mid-char.
        let desc = "a".repeat(1999) + "\u{00e9}" + " trailing";
        assert!(desc.len() > 2000);
        assert!(!desc.is_char_boundary(2000)); // byte 2000 is continuation byte

        // The truncation code must not panic
        let truncated = if desc.len() > 2000 {
            let mut end = 2000;
            while !desc.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            desc[..end].to_string()
        } else {
            desc.clone()
        };

        assert_eq!(truncated.len(), 1999); // truncated just before the 2-byte char
        assert!(truncated.is_char_boundary(truncated.len()));
    }
}
