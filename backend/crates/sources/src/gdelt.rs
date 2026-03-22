use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::{DataSource, InsertableEvent, SourceContext};
use crate::common::{country_center, region_for_country};

/// Search queries rotated through on successive polls.
const QUERIES: &[&str] = &[
    "iran israel war",
    "ukraine russia war",
    "yemen houthi",
    "sudan conflict",
    "strait hormuz",
    "cyber attack iran",
    "missile strike",
];

pub struct GdeltSource {
    /// Index into QUERIES to rotate through on each poll.
    query_index: Mutex<usize>,
    /// Tracks the newest seendate we have already emitted so we can skip duplicates.
    last_seen: Mutex<Option<String>>,
}

/// GDELT Doc API response envelope.
#[derive(Debug, Deserialize)]
struct GdeltDocResponse {
    #[serde(default)]
    articles: Vec<GdeltArticle>,
}

#[derive(Debug, Deserialize)]
struct GdeltArticle {
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    seendate: String,
    #[serde(default)]
    socialimage: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    sourcecountry: String,
    #[serde(default)]
    tone: String,
}

impl Default for GdeltSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GdeltSource {
    pub fn new() -> Self {
        Self {
            query_index: Mutex::new(0),
            last_seen: Mutex::new(None),
        }
    }

    /// Advance the query index and return the current query string.
    fn next_query(&self) -> &'static str {
        let mut idx = self.query_index.lock().unwrap_or_else(|e| e.into_inner());
        let query = QUERIES[*idx % QUERIES.len()];
        *idx = (*idx + 1) % QUERIES.len();
        query
    }

    /// Parse GDELT's seendate format (e.g. "20240615T143000Z") into a comparable
    /// string. Returns the raw string on parse failure so ordering still roughly works.
    fn normalize_seendate(raw: &str) -> String {
        // GDELT seendates are already lexicographically sortable in their raw form.
        raw.to_string()
    }

    /// Parse GDELT seendate format into a UTC DateTime.
    fn parse_seendate(raw: &str) -> Option<chrono::DateTime<Utc>> {
        chrono::NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%SZ")
            .ok()
            .map(|ndt| ndt.and_utc())
    }
}

impl DataSource for GdeltSource {
    fn id(&self) -> &str {
        "gdelt"
    }

    fn name(&self) -> &str {
        "GDELT News"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(15 * 60) // 15 minutes
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let query = self.next_query();
        let encoded_query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("query", query)
            .finish();
        // form_urlencoded prepends "query=", but we only need the value portion.
        let encoded_value = &encoded_query["query=".len()..];

        let url = format!(
            "https://api.gdeltproject.org/api/v2/doc/doc?query={q}&mode=ArtList&maxrecords=250&format=json",
            q = encoded_value,
        );

        debug!(query, "Polling GDELT Doc API");

        let resp = match ctx.http.get(&url).timeout(Duration::from_secs(15)).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, query, "GDELT request failed, retrying once");
                tokio::time::sleep(Duration::from_secs(2)).await;
                match ctx.http.get(&url).timeout(Duration::from_secs(15)).send().await {
                    Ok(r) => r,
                    Err(e2) => {
                        warn!(error = %e2, query, "GDELT retry also failed");
                        return Err(anyhow::anyhow!("GDELT request failed after retry: {e2}"));
                    }
                }
            }
        };

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "gdelt")?;

        let body_text = resp.text().await?;

        // GDELT occasionally returns empty or non-JSON responses for rare queries.
        let doc_resp: GdeltDocResponse = match serde_json::from_str(&body_text) {
            Ok(r) => r,
            Err(e) => {
                if body_text.trim().is_empty() || body_text.trim().starts_with('<') {
                    // Empty body or HTML error page — service is down/broken
                    return Err(anyhow::anyhow!("GDELT returned non-JSON response ({} bytes): {e}", body_text.len()));
                }
                // Non-empty JSON-ish body that failed to parse — rare query, no results
                warn!(error = %e, body_len = body_text.len(), "Failed to parse GDELT response; returning empty");
                return Ok(Vec::new());
            }
        };

        // Determine cutoff: only emit articles whose seendate is strictly newer
        // than the last_seen watermark.
        let cutoff = {
            let guard = self.last_seen.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };

        let mut events: Vec<InsertableEvent> = Vec::new();
        let mut newest_seen: Option<String> = cutoff.clone();

        for article in &doc_resp.articles {
            let normalized = Self::normalize_seendate(&article.seendate);

            // Skip articles we've already seen.
            if let Some(ref cutoff_val) = cutoff
                && normalized <= *cutoff_val {
                    continue;
                }

            // Track the newest date in this batch.
            match &newest_seen {
                Some(ns) if normalized > *ns => {
                    newest_seen = Some(normalized.clone());
                }
                None => {
                    newest_seen = Some(normalized.clone());
                }
                _ => {}
            }

            let data = serde_json::json!({
                "title": article.title,
                "url": article.url,
                "tone": article.tone,
                "sourcecountry": article.sourcecountry,
                "seendate": article.seendate,
                "domain": article.domain,
                "language": article.language,
                "socialimage": article.socialimage,
                "query": query,
            });

            // Parse tone for severity
            let tone = article.tone.split(',').next().and_then(|t| t.parse::<f64>().ok());
            let severity = match tone {
                Some(t) if t < -5.0 => Severity::High,
                Some(t) if t < -2.0 => Severity::Medium,
                _ => Severity::Low,
            };

            let region_code = if article.sourcecountry.is_empty() {
                None
            } else {
                region_for_country(&article.sourcecountry).map(String::from)
            };

            let mut tags = Vec::new();
            tags.push(format!("query:{}", query));
            if !article.language.is_empty() {
                tags.push(article.language.clone());
            }

            let event_time = Self::parse_seendate(&article.seendate)
                .unwrap_or_else(Utc::now);

            // Approximate coordinates from source country code
            let (latitude, longitude) = country_center(&article.sourcecountry)
                .map(|(lat, lon)| (Some(lat), Some(lon)))
                .unwrap_or((None, None));

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Gdelt,
                source_id: if article.url.is_empty() { None } else { Some(article.url.clone()) },
                longitude,
                latitude,
                region_code,
                entity_id: None,
                entity_name: None,
                event_type: EventType::NewsArticle,
                severity,
                confidence: None,
                tags,
                title: if article.title.is_empty() { None } else { Some(article.title.clone()) },
                description: if article.url.is_empty() { None } else { Some(article.url.clone()) },
                payload: data,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Update the watermark.
        if let Some(ns) = newest_seen {
            let mut guard = self.last_seen.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(ns);
        }

        debug!(count = events.len(), query, "GDELT poll complete");
        Ok(events)
        })
    }
}
