//! Supplementary web search via Exa API for situation enrichment.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Circuit breaker for Exa API — trips on 402 (credits exhausted) to avoid
/// spamming a dead API. Resets on process restart.
static EXA_CIRCUIT_OPEN: AtomicBool = AtomicBool::new(false);

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sr_types::Severity;
use tracing::{debug, info, warn};
use ts_rs::TS;

/// Maximum number of articles stored per cluster.
const MAX_ARTICLES_PER_CLUSTER: usize = 10;

/// Supplementary data from web search for a situation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct SupplementaryData {
    pub articles: Vec<SearchArticle>,
    pub context: String,
}

impl SupplementaryData {
    /// Merge new articles into existing supplementary data, deduplicating by URL.
    /// Caps total articles at `MAX_ARTICLES_PER_CLUSTER`. Newer articles are appended
    /// and the context string is rebuilt from the latest top-3 snippets.
    pub fn merge(&mut self, new: SupplementaryData) {
        let existing_urls: HashSet<String> = self.articles.iter().map(|a| a.url.clone()).collect();
        for article in new.articles {
            if self.articles.len() >= MAX_ARTICLES_PER_CLUSTER {
                break;
            }
            if !existing_urls.contains(&article.url) {
                self.articles.push(article);
            }
        }
        // Rebuild context from the latest 3 articles (prefer tail = newest)
        let start = self.articles.len().saturating_sub(3);
        self.context = self.articles[start..]
            .iter()
            .map(|a| a.snippet.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct SearchArticle {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_date: Option<String>,
    #[serde(default)]
    pub highlights: Vec<String>,
}

/// Rate limiter: max 1 search per minute, max 10 per hour.
/// Daily cap: ~$10/day at $0.007/request ≈ 1,400 requests.
const EXA_DAILY_CAP: u32 = 1_400;

pub struct SearchRateLimiter {
    last_search: std::sync::Mutex<tokio::time::Instant>,
    hourly_count: AtomicU64,
    hourly_reset: std::sync::Mutex<tokio::time::Instant>,
    /// Total Exa requests made today (resets at midnight UTC via `maybe_reset_daily`).
    exa_requests_today: AtomicU32,
    /// Instant of the last daily counter reset.
    daily_reset: std::sync::Mutex<tokio::time::Instant>,
}

impl SearchRateLimiter {
    pub fn new() -> Self {
        Self {
            last_search: std::sync::Mutex::new(tokio::time::Instant::now() - Duration::from_secs(30)),
            hourly_count: AtomicU64::new(0),
            hourly_reset: std::sync::Mutex::new(tokio::time::Instant::now()),
            exa_requests_today: AtomicU32::new(0),
            daily_reset: std::sync::Mutex::new(tokio::time::Instant::now()),
        }
    }

    pub fn can_search(&self) -> bool {
        let now = tokio::time::Instant::now();

        // Check daily cap ($10/day budget)
        self.maybe_reset_daily();
        if self.exa_requests_today.load(Ordering::Relaxed) >= EXA_DAILY_CAP {
            return false;
        }

        // Check hourly reset
        if let Ok(mut reset) = self.hourly_reset.lock() {
            if now.duration_since(*reset) >= Duration::from_secs(3600) {
                self.hourly_count.store(0, Ordering::Relaxed);
                *reset = now;
            }
        }

        // Check hourly cap (60/hr = ~1,440/day max, consistent with daily cap)
        if self.hourly_count.load(Ordering::Relaxed) >= 60 {
            return false;
        }

        // Check per-request cooldown (30 seconds between searches)
        if let Ok(last) = self.last_search.lock() {
            now.duration_since(*last) >= Duration::from_secs(30)
        } else {
            false
        }
    }

    /// Atomically check rate limits AND reserve a search slot in one operation.
    /// Prevents the TOCTOU race between `can_search()` + `record_search()` when
    /// multiple async tasks are spawned from a loop.
    pub fn try_acquire(&self) -> bool {
        let now = tokio::time::Instant::now();

        self.maybe_reset_daily();
        if self.exa_requests_today.load(Ordering::Relaxed) >= EXA_DAILY_CAP {
            return false;
        }

        if let Ok(mut reset) = self.hourly_reset.lock() {
            if now.duration_since(*reset) >= Duration::from_secs(3600) {
                self.hourly_count.store(0, Ordering::Relaxed);
                *reset = now;
            }
        }

        if self.hourly_count.load(Ordering::Relaxed) >= 60 {
            return false;
        }

        // Atomically check cooldown AND update last_search under the same lock
        if let Ok(mut last) = self.last_search.lock() {
            if now.duration_since(*last) >= Duration::from_secs(30) {
                *last = now;
                self.hourly_count.fetch_add(1, Ordering::Relaxed);
                self.exa_requests_today.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }

        false
    }

    pub fn record_search(&self) {
        if let Ok(mut last) = self.last_search.lock() {
            *last = tokio::time::Instant::now();
        }
        self.hourly_count.fetch_add(1, Ordering::Relaxed);
        self.maybe_reset_daily();
        self.exa_requests_today.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset daily counter if 24 hours have elapsed since last reset.
    fn maybe_reset_daily(&self) {
        let now = tokio::time::Instant::now();
        if let Ok(mut reset) = self.daily_reset.lock() {
            if now.duration_since(*reset) >= Duration::from_secs(86_400) {
                self.exa_requests_today.store(0, Ordering::Relaxed);
                *reset = now;
            }
        }
    }

    /// Number of Exa API requests made today.
    pub fn requests_today(&self) -> u32 {
        self.maybe_reset_daily();
        self.exa_requests_today.load(Ordering::Relaxed)
    }

    /// Estimated cost in USD for today's Exa usage (~$7 per 1K requests = $0.007/request).
    pub fn daily_cost_estimate(&self) -> f64 {
        self.requests_today() as f64 * 0.007
    }
}

impl Default for SearchRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a search query from cluster metadata: entities, topics, and region names.
/// Prefers entities + region over the AI title, which can be too narrative for search.
pub fn build_search_query(
    title: &str,
    entities: &[String],
    topics: &[String],
    regions: &[String],
) -> String {
    const GENERIC_REGIONS: &[&str] = &[
        "africa", "europe", "asia", "middle-east", "ME", "americas",
        "south-america", "north-america", "oceania", "global",
    ];

    let mut parts: Vec<String> = Vec::new();

    // Lead with top entities (most specific signal), preferring multi-word proper nouns
    let mut sorted_entities: Vec<&String> = entities.iter().collect();
    sorted_entities.sort_by(|a, b| {
        let a_words = a.split_whitespace().count();
        let b_words = b.split_whitespace().count();
        b_words.cmp(&a_words)
    });
    for entity in sorted_entities.iter().take(3) {
        let e = entity.trim();
        if !e.is_empty() {
            parts.push(e.to_string());
        }
    }

    // Add region names, skipping generic regions when we already have entities
    for region in regions.iter().take(2) {
        let r = region.trim();
        if !r.is_empty()
            && !parts.iter().any(|p| p.eq_ignore_ascii_case(r))
            && !(parts.len() >= 2 && GENERIC_REGIONS.iter().any(|g| r.eq_ignore_ascii_case(g)))
        {
            parts.push(r.to_string());
        }
    }

    // Add top topics (filtering out generic prefixed ones)
    for topic in topics.iter().take(2) {
        let t = topic.trim().replace('-', " ");
        if !t.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(&t)) {
            parts.push(t);
        }
    }

    // If we still have nothing useful, fall back to the AI title
    if parts.is_empty() {
        return title.to_string();
    }

    // Quality gate: reject queries that are just generic regions
    let is_all_generic = parts.iter().all(|p| {
        GENERIC_REGIONS.iter().any(|g| p.eq_ignore_ascii_case(g))
            || p.len() < 4
    });
    if is_all_generic && entities.is_empty() {
        return String::new(); // signal: don't search
    }

    let query = parts.join(" ");

    // If query is very short (< 10 chars), supplement with title keywords
    if query.len() < 10 {
        let title_words: Vec<&str> = title.split_whitespace().take(5).collect();
        let suffix = title_words.join(" ");
        if !suffix.is_empty() {
            return format!("{query} {suffix}");
        }
    }

    query
}

// ---------------------------------------------------------------------------
// Gap analysis types
// ---------------------------------------------------------------------------

/// The kind of information gap detected in a situation cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GapType {
    /// Has conflict/sensor events but no news coverage.
    NewsCoverage,
    /// Has news but no sensor/conflict confirmation.
    GroundVerification,
    /// Entities with no enrichment context.
    EntityBackground,
    /// Supplementary data is stale while situation is still active.
    StaleContext,
    /// All events come from a single source type.
    Corroboration,
    /// Has geographic centroid but no region-specific articles.
    RegionalContext,
}

impl fmt::Display for GapType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            GapType::NewsCoverage => "news_coverage",
            GapType::GroundVerification => "ground_verification",
            GapType::EntityBackground => "entity_background",
            GapType::StaleContext => "stale_context",
            GapType::Corroboration => "corroboration",
            GapType::RegionalContext => "regional_context",
        };
        write!(f, "{s}")
    }
}

impl FromStr for GapType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "news_coverage" => Ok(GapType::NewsCoverage),
            "ground_verification" => Ok(GapType::GroundVerification),
            "entity_background" => Ok(GapType::EntityBackground),
            "stale_context" => Ok(GapType::StaleContext),
            "corroboration" => Ok(GapType::Corroboration),
            "regional_context" => Ok(GapType::RegionalContext),
            _ => Err(format!("unknown GapType: {s}")),
        }
    }
}

impl GapType {
    /// Minimum cooldown before the same gap type triggers another search.
    fn cooldown(&self) -> chrono::Duration {
        match self {
            GapType::NewsCoverage => chrono::Duration::minutes(20),
            GapType::GroundVerification => chrono::Duration::minutes(30),
            GapType::EntityBackground => chrono::Duration::hours(2),
            GapType::StaleContext => chrono::Duration::minutes(15),
            GapType::Corroboration => chrono::Duration::minutes(45),
            GapType::RegionalContext => chrono::Duration::hours(1),
        }
    }

    /// How far back the search should look for content.
    /// News/event searches need very recent content; entity background can be older.
    pub fn search_lookback(&self) -> Option<chrono::Duration> {
        match self {
            // Breaking news — only last 48 hours
            GapType::NewsCoverage => Some(chrono::Duration::hours(48)),
            // Recent context refresh
            GapType::StaleContext => Some(chrono::Duration::hours(48)),
            // Regional news — last 72 hours
            GapType::RegionalContext => Some(chrono::Duration::hours(72)),
            // Verification — last 7 days
            GapType::GroundVerification => Some(chrono::Duration::days(7)),
            // Corroboration — last 7 days
            GapType::Corroboration => Some(chrono::Duration::days(7)),
            // Entity background — no time filter, older articles are fine
            GapType::EntityBackground => None,
        }
    }
}

/// A detected information gap in a cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationGap {
    pub gap_type: GapType,
    /// Weight from 0.0 to 1.0 indicating how important this gap is.
    pub weight: f32,
    pub reason: String,
}

/// Tracks search history per gap type for a cluster.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchHistory {
    /// Last time each gap type was searched.
    pub last_searched_by_type: HashMap<GapType, DateTime<Utc>>,
    /// Total number of searches performed for this cluster.
    pub total_searches: u32,
    /// Number of searches that returned no results.
    pub empty_searches: u32,
}

impl SearchHistory {
    /// Restore a single gap-type entry from the database. Uses `max()` for
    /// counters so that in-memory state is never downgraded by stale DB rows.
    pub fn set_from_db(&mut self, gap_type: GapType, last_searched: DateTime<Utc>, total: u32, empty: u32) {
        self.last_searched_by_type.insert(gap_type, last_searched);
        self.total_searches = self.total_searches.max(total);
        self.empty_searches = self.empty_searches.max(empty);
    }
}

/// Input data for gap analysis — a snapshot of cluster state.
#[derive(Debug, Clone)]
pub struct GapAnalysisInput {
    pub source_types: HashSet<String>,
    pub entities: HashSet<String>,
    pub topics: HashSet<String>,
    pub region_codes: HashSet<String>,
    pub severity: Severity,
    pub event_count: usize,
    pub centroid: Option<(f64, f64)>,
    pub has_supplementary: bool,
    pub supplementary_age_secs: Option<i64>,
    pub search_history: SearchHistory,
    pub has_enrichment: bool,
    pub last_updated: DateTime<Utc>,
    pub first_seen: DateTime<Utc>,
}

// Source type classification helpers
const NEWS_SOURCES: &[&str] = &["gdelt", "gdelt-geo", "rss"];
const CONFLICT_SOURCES: &[&str] = &["acled", "geoconfirmed"];
const SENSOR_SOURCES: &[&str] = &["firms", "usgs", "opensky", "airplaneslive-mil", "ais"];

fn has_source_type(source_types: &HashSet<String>, patterns: &[&str]) -> bool {
    source_types.iter().any(|st| patterns.iter().any(|p| st.starts_with(p)))
}

/// Analyze a cluster's state to detect information gaps that could be filled
/// by a targeted web search.
pub fn analyze_gaps(input: &GapAnalysisInput) -> Vec<InformationGap> {
    let mut gaps = Vec::new();
    let now = Utc::now();

    let has_news = has_source_type(&input.source_types, NEWS_SOURCES);
    let has_conflict = has_source_type(&input.source_types, CONFLICT_SOURCES);
    let has_sensor = has_source_type(&input.source_types, SENSOR_SOURCES);

    // Helper: check if a gap type is on cooldown
    let on_cooldown = |gap_type: &GapType| -> bool {
        if let Some(last) = input.search_history.last_searched_by_type.get(gap_type) {
            now.signed_duration_since(*last) < gap_type.cooldown()
        } else {
            false
        }
    };

    // 1. NewsCoverage: has conflict/sensor events but no news
    if (has_conflict || has_sensor) && !has_news && !on_cooldown(&GapType::NewsCoverage) {
        gaps.push(InformationGap {
            gap_type: GapType::NewsCoverage,
            weight: 0.9,
            reason: "Conflict/sensor data detected without news coverage".to_string(),
        });
    }

    // 2. GroundVerification: has news but no ground truth from sensors/conflict data
    if has_news && !has_conflict && !has_sensor && !on_cooldown(&GapType::GroundVerification) {
        gaps.push(InformationGap {
            gap_type: GapType::GroundVerification,
            weight: 0.6,
            reason: "News reports lack sensor or conflict data confirmation".to_string(),
        });
    }

    // 3. Corroboration: only 1 source type and >= 3 events
    if input.source_types.len() == 1 && input.event_count >= 3 && !on_cooldown(&GapType::Corroboration) {
        gaps.push(InformationGap {
            gap_type: GapType::Corroboration,
            weight: 0.65,
            reason: format!(
                "All {} events from single source type: {}",
                input.event_count,
                input.source_types.iter().next().unwrap_or(&"unknown".to_string())
            ),
        });
    }

    // 4. StaleContext: supplementary data exists but is old
    if input.has_supplementary
        && !on_cooldown(&GapType::StaleContext)
        && let Some(age_secs) = input.supplementary_age_secs
    {
        let stale_threshold = match input.severity {
            Severity::Critical => 30 * 60,    // 30 minutes
            Severity::High => 60 * 60,        // 1 hour
            _ => 3 * 60 * 60,                 // 3 hours
        };
        if age_secs >= stale_threshold {
            let weight = match input.severity {
                Severity::Critical => 0.85,
                Severity::High => 0.8,
                _ => 0.7,
            };
            gaps.push(InformationGap {
                gap_type: GapType::StaleContext,
                weight,
                reason: format!(
                    "Supplementary data is {}min old (threshold: {}min for {} severity)",
                    age_secs / 60,
                    stale_threshold / 60,
                    input.severity,
                ),
            });
        }
    }

    // 5. EntityBackground: has entities but no enrichment context
    if !input.entities.is_empty() && !input.has_enrichment && !on_cooldown(&GapType::EntityBackground) {
        let weight = if input.entities.len() >= 3 { 0.5 } else { 0.35 };
        gaps.push(InformationGap {
            gap_type: GapType::EntityBackground,
            weight,
            reason: format!(
                "{} entities with no enrichment background",
                input.entities.len()
            ),
        });
    }

    // 6. RegionalContext: has centroid but no region-specific articles
    if input.centroid.is_some()
        && !input.region_codes.is_empty()
        && !input.has_supplementary
        && !on_cooldown(&GapType::RegionalContext)
    {
        gaps.push(InformationGap {
            gap_type: GapType::RegionalContext,
            weight: 0.4,
            reason: "Geographic cluster without regional news context".to_string(),
        });
    }

    // Sort by weight descending for convenience
    gaps.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
    gaps
}

/// Compute a search priority score (0-100) for a cluster.
/// Higher scores mean the cluster should be searched sooner.
pub fn compute_search_priority(input: &GapAnalysisInput, gaps: &[InformationGap]) -> u32 {
    if gaps.is_empty() {
        return 0;
    }

    let mut score: f32 = 0.0;

    // Severity: up to 40 points
    score += match input.severity {
        Severity::Critical => 40.0,
        Severity::High => 30.0,
        Severity::Medium => 15.0,
        Severity::Low => 5.0,
        Severity::Info => 0.0,
    };

    // Source diversity: up to 20 points (more sources = more interesting)
    let diversity = (input.source_types.len() as f32).min(5.0) * 4.0;
    score += diversity;

    // Gap count: up to 20 points
    let gap_score = (gaps.len() as f32).min(4.0) * 5.0;
    score += gap_score;

    // Event velocity: up to 10 points (based on events / age in hours)
    let age_hours = Utc::now()
        .signed_duration_since(input.first_seen)
        .num_minutes()
        .max(1) as f32
        / 60.0;
    let velocity = (input.event_count as f32 / age_hours).min(10.0);
    score += velocity;

    // Penalty: recency of last search (searched recently = lower priority)
    let min_since_any_search = input.search_history.last_searched_by_type.values()
        .map(|t| Utc::now().signed_duration_since(*t).num_minutes())
        .min();
    if let Some(minutes) = min_since_any_search {
        if minutes < 15 {
            score -= 20.0;
        } else if minutes < 30 {
            score -= 10.0;
        }
    }

    // Penalty: high empty search ratio (cluster is a dead end for web search)
    if input.search_history.total_searches > 2 {
        let empty_ratio = input.search_history.empty_searches as f32
            / input.search_history.total_searches as f32;
        if empty_ratio > 0.7 {
            score -= 15.0;
        } else if empty_ratio > 0.5 {
            score -= 8.0;
        }
    }

    score.clamp(0.0, 100.0) as u32
}

/// Build a search query tailored to a specific gap type.
pub fn build_gap_query(
    gap_type: GapType,
    title: &str,
    entities: &[String],
    topics: &[String],
    regions: &[String],
) -> String {
    match gap_type {
        GapType::NewsCoverage => {
            let entity_part = entities.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
            let region_part = regions.first().cloned().unwrap_or_default();
            let q = format!("{entity_part} {region_part} latest news").trim().to_string();
            if q.len() < 5 { build_search_query(title, entities, topics, regions) } else { q }
        }
        GapType::GroundVerification => {
            let region_part = regions.first().cloned().unwrap_or_default();
            let topic_part = topics.first().cloned().unwrap_or_default();
            let q = format!("{region_part} {topic_part} confirmed reports").trim().to_string();
            if q.len() < 10 { build_search_query(title, entities, topics, regions) } else { q }
        }
        GapType::EntityBackground => {
            let entity = entities.first().cloned().unwrap_or_default();
            let q = format!("{entity} background profile").trim().to_string();
            if q.len() < 10 { build_search_query(title, entities, topics, regions) } else { q }
        }
        GapType::StaleContext => {
            let base = build_search_query(title, entities, topics, regions);
            format!("{base} latest update")
        }
        GapType::Corroboration => {
            // Standard entity+topic query — looking for independent corroboration
            build_search_query(title, entities, topics, regions)
        }
        GapType::RegionalContext => {
            let region_part = regions.first().cloned().unwrap_or_default();
            let topic_part = topics.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
            let q = format!("{region_part} {topic_part} local reports").trim().to_string();
            if q.len() < 10 { build_search_query(title, entities, topics, regions) } else { q }
        }
    }
}

/// Search for supplementary context about a situation using Exa API.
/// Returns None if API key not set, rate limited, or search fails.
///
/// - `gap_type`: When set to `"NewsCoverage"`, `"StaleContext"`, or `"RegionalContext"`,
///   adds `"category": "news"` to the Exa request for more targeted results.
/// - `since`: ISO 8601 timestamp — when provided, adds `"startPublishedDate"` to
///   restrict results to articles published after this date.
pub async fn search_situation_context(
    http: &reqwest::Client,
    title: &str,
    entities: &[String],
    topics: &[String],
    regions: &[String],
    rate_limiter: &SearchRateLimiter,
    gap_type: Option<&str>,
    since: Option<&str>,
) -> Option<SupplementaryData> {
    // Circuit breaker: once 402 is hit, stop all searches this session
    if EXA_CIRCUIT_OPEN.load(Ordering::Relaxed) {
        return None;
    }

    // Rate limiting: callers should use try_acquire() before calling this function.
    // This is a fallback safety net — if called directly without pre-acquisition,
    // try_acquire() atomically checks + reserves to prevent burst.
    if !rate_limiter.try_acquire() {
        debug!("Exa search rate limited — skipping");
        return None;
    }

    let api_key = std::env::var("EXA_API_KEY").ok()?;
    if api_key.is_empty() {
        return None;
    }

    let query = build_search_query(title, entities, topics, regions);
    if query.is_empty() {
        debug!("Search query rejected by quality gate — skipping");
        return None;
    }

    debug!(query = %query, gap_type = ?gap_type, since = ?since, "Searching Exa for situation context");

    let mut body = serde_json::json!({
        "query": query,
        "num_results": 5,
        "use_autoprompt": true,
        "type": "auto",
        "highlights": {
            "highlights_per_url": 2,
            "num_sentences": 3,
            "query": query
        }
    });

    // Add news category filter for news-related gap types
    if let Some(gt) = gap_type {
        if matches!(gt, "NewsCoverage" | "StaleContext" | "RegionalContext") {
            body.as_object_mut()
                .unwrap()
                .insert("category".to_string(), serde_json::json!("news"));
        }
    }

    // Add recency filter when a start date is provided
    if let Some(start_date) = since {
        body.as_object_mut()
            .unwrap()
            .insert("startPublishedDate".to_string(), serde_json::json!(start_date));
    }

    let resp = match http
        .post("https://api.exa.ai/search")
        .header("x-api-key", &api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("Exa search failed: {e}");
            return None;
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if status.as_u16() == 402 {
            warn!("Exa API credits exhausted (402) — circuit breaker tripped, disabling searches");
            EXA_CIRCUIT_OPEN.store(true, Ordering::Relaxed);
        } else {
            warn!(status = %status, body = %body_text, "Exa API error");
        }
        return None;
    }

    let data: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse Exa response: {e}");
            return None;
        }
    };

    let results = data.get("results").and_then(|v| v.as_array())?;

    let articles: Vec<SearchArticle> = results
        .iter()
        .filter_map(|r| {
            let highlights: Vec<String> = r
                .get("highlights")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Build snippet from highlights (joined with " ... "), falling back
            // to text field for backward compatibility with older API responses.
            let snippet = if highlights.is_empty() {
                r.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(300)
                    .collect::<String>()
            } else {
                highlights.join(" ... ")
            };

            Some(SearchArticle {
                title: r.get("title").and_then(|v| v.as_str())?.to_string(),
                url: r.get("url").and_then(|v| v.as_str())?.to_string(),
                snippet,
                published_date: r
                    .get("published_date")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                highlights,
            })
        })
        .collect();

    if articles.is_empty() {
        return None;
    }

    // Build a brief context summary from snippets
    let context = articles
        .iter()
        .take(3)
        .map(|a| a.snippet.as_str())
        .collect::<Vec<_>>()
        .join(" | ");

    info!(
        count = articles.len(),
        query = %title,
        "Exa search returned supplementary articles"
    );

    Some(SupplementaryData { articles, context })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_article(url: &str, title: &str) -> SearchArticle {
        SearchArticle {
            title: title.to_string(),
            url: url.to_string(),
            snippet: format!("Snippet for {title}"),
            published_date: None,
            highlights: Vec::new(),
        }
    }

    fn make_supplementary(articles: Vec<SearchArticle>) -> SupplementaryData {
        let context = articles
            .iter()
            .take(3)
            .map(|a| a.snippet.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        SupplementaryData { articles, context }
    }

    #[test]
    fn test_merge_deduplicates_by_url() {
        let mut existing = make_supplementary(vec![
            make_article("https://example.com/1", "Article 1"),
            make_article("https://example.com/2", "Article 2"),
        ]);
        let new = make_supplementary(vec![
            make_article("https://example.com/2", "Article 2 (duplicate)"),
            make_article("https://example.com/3", "Article 3"),
        ]);
        existing.merge(new);
        assert_eq!(existing.articles.len(), 3);
        // The duplicate should keep the original title
        assert_eq!(existing.articles[1].title, "Article 2");
        assert_eq!(existing.articles[2].url, "https://example.com/3");
    }

    #[test]
    fn test_merge_caps_at_max() {
        let mut existing = make_supplementary(
            (0..9)
                .map(|i| make_article(&format!("https://example.com/{i}"), &format!("Article {i}")))
                .collect(),
        );
        let new = make_supplementary(vec![
            make_article("https://example.com/new1", "New 1"),
            make_article("https://example.com/new2", "New 2"),
        ]);
        existing.merge(new);
        // 9 existing + 1 new = 10 (cap), second new article dropped
        assert_eq!(existing.articles.len(), MAX_ARTICLES_PER_CLUSTER);
        assert!(existing.articles.iter().any(|a| a.url == "https://example.com/new1"));
        assert!(!existing.articles.iter().any(|a| a.url == "https://example.com/new2"));
    }

    #[test]
    fn test_merge_rebuilds_context_from_tail() {
        let mut existing = make_supplementary(vec![
            make_article("https://example.com/1", "Old"),
        ]);
        let new = make_supplementary(vec![
            make_article("https://example.com/2", "New A"),
            make_article("https://example.com/3", "New B"),
        ]);
        existing.merge(new);
        // Context should be from the last 3 articles
        assert!(existing.context.contains("Snippet for New B"));
    }

    #[test]
    fn test_build_query_entities_and_region() {
        let q = build_search_query(
            "AI Title Here",
            &["Iran".into(), "IRGC".into()],
            &["missile".into()],
            &["IR".into()],
        );
        assert!(q.contains("Iran"));
        assert!(q.contains("IRGC"));
        assert!(q.contains("IR"));
        assert!(q.contains("missile"));
        // Should NOT contain the AI title since entities are present
        assert!(!q.contains("AI Title Here"));
    }

    #[test]
    fn test_build_query_fallback_to_title() {
        let q = build_search_query("Fallback Title", &[], &[], &[]);
        assert_eq!(q, "Fallback Title");
    }

    #[test]
    fn test_build_query_short_query_supplemented() {
        let q = build_search_query(
            "Something happened in region",
            &["Iran".into()],
            &[],
            &[],
        );
        // "Iran" is < 10 chars, so title keywords get appended
        assert!(q.contains("Iran"));
        assert!(q.contains("Something"));
    }

    #[test]
    fn test_build_query_deduplicates() {
        let q = build_search_query(
            "Title",
            &["Ukraine".into()],
            &["ukraine".into()],
            &["Ukraine".into()],
        );
        // "Ukraine" should appear only once (case-insensitive dedup)
        let count = q.matches("kraine").count(); // case insensitive via substring
        assert_eq!(count, 1);
    }

    #[test]
    fn test_build_query_topic_hyphens_replaced() {
        let q = build_search_query(
            "Title",
            &["Entity".into()],
            &["cyber-attack".into()],
            &["US".into()],
        );
        assert!(q.contains("cyber attack"));
        assert!(!q.contains("cyber-attack"));
    }

    // -----------------------------------------------------------------------
    // Gap analysis tests
    // -----------------------------------------------------------------------

    fn default_gap_input() -> GapAnalysisInput {
        GapAnalysisInput {
            source_types: HashSet::new(),
            entities: HashSet::new(),
            topics: HashSet::new(),
            region_codes: HashSet::new(),
            severity: Severity::Medium,
            event_count: 5,
            centroid: None,
            has_supplementary: false,
            supplementary_age_secs: None,
            search_history: SearchHistory::default(),
            has_enrichment: false,
            last_updated: Utc::now(),
            first_seen: Utc::now() - chrono::Duration::hours(1),
        }
    }

    #[test]
    fn test_gap_news_coverage() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        input.source_types.insert("firms".to_string());
        // No news sources

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::NewsCoverage));
        let news_gap = gaps.iter().find(|g| g.gap_type == GapType::NewsCoverage).unwrap();
        assert!((news_gap.weight - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_gap_no_news_coverage_when_news_present() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        input.source_types.insert("gdelt".to_string()); // news source present

        let gaps = analyze_gaps(&input);
        assert!(!gaps.iter().any(|g| g.gap_type == GapType::NewsCoverage));
    }

    #[test]
    fn test_gap_ground_verification() {
        let mut input = default_gap_input();
        input.source_types.insert("gdelt".to_string());
        // No conflict or sensor sources

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::GroundVerification));
        let gap = gaps.iter().find(|g| g.gap_type == GapType::GroundVerification).unwrap();
        assert!((gap.weight - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_gap_corroboration_single_source() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        input.event_count = 5;

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::Corroboration));
    }

    #[test]
    fn test_gap_no_corroboration_multi_source() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        input.source_types.insert("gdelt".to_string());
        input.event_count = 5;

        let gaps = analyze_gaps(&input);
        assert!(!gaps.iter().any(|g| g.gap_type == GapType::Corroboration));
    }

    #[test]
    fn test_gap_stale_context_critical() {
        let mut input = default_gap_input();
        input.has_supplementary = true;
        input.supplementary_age_secs = Some(35 * 60); // 35 minutes
        input.severity = Severity::Critical;

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::StaleContext));
        let gap = gaps.iter().find(|g| g.gap_type == GapType::StaleContext).unwrap();
        assert!((gap.weight - 0.85).abs() < 0.01);
    }

    #[test]
    fn test_gap_no_stale_for_fresh_data() {
        let mut input = default_gap_input();
        input.has_supplementary = true;
        input.supplementary_age_secs = Some(10 * 60); // 10 minutes
        input.severity = Severity::Medium;
        // Medium threshold is 3 hours, 10 min < 3h

        let gaps = analyze_gaps(&input);
        assert!(!gaps.iter().any(|g| g.gap_type == GapType::StaleContext));
    }

    #[test]
    fn test_gap_entity_background() {
        let mut input = default_gap_input();
        input.entities.insert("Hezbollah".to_string());
        input.entities.insert("IRGC".to_string());
        input.entities.insert("Hamas".to_string());
        input.has_enrichment = false;

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::EntityBackground));
        let gap = gaps.iter().find(|g| g.gap_type == GapType::EntityBackground).unwrap();
        assert!((gap.weight - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gap_regional_context() {
        let mut input = default_gap_input();
        input.centroid = Some((33.0, 44.0));
        input.region_codes.insert("IQ".to_string());
        input.has_supplementary = false;

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::RegionalContext));
    }

    #[test]
    fn test_gap_cooldown_prevents_detection() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        // No news → should detect NewsCoverage gap

        // But mark it as recently searched (within cooldown)
        input.search_history.last_searched_by_type.insert(
            GapType::NewsCoverage,
            Utc::now() - chrono::Duration::minutes(5), // 5 min ago, cooldown is 20 min
        );

        let gaps = analyze_gaps(&input);
        assert!(!gaps.iter().any(|g| g.gap_type == GapType::NewsCoverage));
    }

    #[test]
    fn test_gap_cooldown_expired_allows_detection() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());

        // Searched 25 minutes ago, cooldown is 20 min → should detect
        input.search_history.last_searched_by_type.insert(
            GapType::NewsCoverage,
            Utc::now() - chrono::Duration::minutes(25),
        );

        let gaps = analyze_gaps(&input);
        assert!(gaps.iter().any(|g| g.gap_type == GapType::NewsCoverage));
    }

    #[test]
    fn test_gap_sorted_by_weight_descending() {
        let mut input = default_gap_input();
        input.source_types.insert("acled".to_string());
        input.source_types.insert("firms".to_string());
        // No news → NewsCoverage (0.9)
        input.entities.insert("Entity1".to_string());
        input.has_enrichment = false;
        // EntityBackground (0.35 for <3 entities)
        input.centroid = Some((33.0, 44.0));
        input.region_codes.insert("IQ".to_string());
        // RegionalContext (0.4)

        let gaps = analyze_gaps(&input);
        assert!(gaps.len() >= 2);
        for i in 1..gaps.len() {
            assert!(gaps[i - 1].weight >= gaps[i].weight);
        }
    }

    // -----------------------------------------------------------------------
    // Priority scoring tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_priority_zero_without_gaps() {
        let input = default_gap_input();
        let score = compute_search_priority(&input, &[]);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_priority_critical_high() {
        let mut input = default_gap_input();
        input.severity = Severity::Critical;
        input.source_types.insert("acled".to_string());
        input.source_types.insert("firms".to_string());
        input.source_types.insert("gdelt".to_string());

        let gaps = vec![InformationGap {
            gap_type: GapType::NewsCoverage,
            weight: 0.9,
            reason: "test".to_string(),
        }];
        let score = compute_search_priority(&input, &gaps);
        assert!(score >= 50, "Critical severity with gaps should score >= 50, got {score}");
    }

    #[test]
    fn test_priority_penalized_by_recent_search() {
        let mut input = default_gap_input();
        input.severity = Severity::High;
        input.source_types.insert("acled".to_string());

        let gaps = vec![InformationGap {
            gap_type: GapType::NewsCoverage,
            weight: 0.9,
            reason: "test".to_string(),
        }];

        let score_fresh = compute_search_priority(&input, &gaps);

        // Now add a recent search
        input.search_history.last_searched_by_type.insert(
            GapType::NewsCoverage,
            Utc::now() - chrono::Duration::minutes(5),
        );
        let score_recent = compute_search_priority(&input, &gaps);

        assert!(score_fresh > score_recent, "Recent search should penalize priority");
    }

    #[test]
    fn test_priority_penalized_by_empty_searches() {
        let mut input = default_gap_input();
        input.severity = Severity::High;
        input.source_types.insert("acled".to_string());

        let gaps = vec![InformationGap {
            gap_type: GapType::NewsCoverage,
            weight: 0.9,
            reason: "test".to_string(),
        }];

        let score_clean = compute_search_priority(&input, &gaps);

        input.search_history.total_searches = 5;
        input.search_history.empty_searches = 4; // 80% empty
        let score_empty = compute_search_priority(&input, &gaps);

        assert!(score_clean > score_empty, "High empty ratio should penalize priority");
    }

    // -----------------------------------------------------------------------
    // Gap query tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gap_query_news_coverage() {
        let q = build_gap_query(
            GapType::NewsCoverage,
            "Situation Title",
            &["Iran".into(), "IRGC".into()],
            &["missile".into()],
            &["IR".into()],
        );
        assert!(q.contains("Iran"));
        assert!(q.contains("latest news"));
    }

    #[test]
    fn test_gap_query_ground_verification() {
        let q = build_gap_query(
            GapType::GroundVerification,
            "Situation Title",
            &["Iran".into()],
            &["missile-strike".into()],
            &["IR".into()],
        );
        assert!(q.contains("confirmed reports"));
    }

    #[test]
    fn test_gap_query_entity_background() {
        let q = build_gap_query(
            GapType::EntityBackground,
            "Title",
            &["IRGC".into()],
            &[],
            &[],
        );
        assert!(q.contains("IRGC"));
        assert!(q.contains("background profile"));
    }

    #[test]
    fn test_gap_query_stale_context() {
        let q = build_gap_query(
            GapType::StaleContext,
            "Title",
            &["Iran".into()],
            &["missile".into()],
            &["IR".into()],
        );
        assert!(q.contains("latest update"));
    }

    #[test]
    fn test_gap_query_regional_context() {
        let q = build_gap_query(
            GapType::RegionalContext,
            "Title",
            &[],
            &["conflict".into(), "strike".into()],
            &["UA".into()],
        );
        assert!(q.contains("UA"));
        assert!(q.contains("local reports"));
    }
}
