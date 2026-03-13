//! Scoring infrastructure for situation clustering.
//!
//! Contains `StreamingIdf` (inverse document frequency with exponential decay),
//! `BurstDetector` (dual EWMA burst detection), haversine distance, and various
//! helper functions used by `SituationGraph::score_candidate()`.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sr_types::{EventType, SourceType};

use sr_sources::InsertableEvent;

// ---------------------------------------------------------------------------
// Streaming IDF — inverse document frequency with exponential decay
// ---------------------------------------------------------------------------

/// Streaming IDF tracker for topics or entities. Tracks observed frequency
/// with exponential decay so that globally common terms get lower weight.
pub(crate) struct StreamingIdf {
    /// Decayed frequency counts per term.
    freq: HashMap<String, f64>,
    /// Running total of observations (also decayed).
    total: f64,
    /// Decay factor applied to all counts on each observation.
    decay_factor: f64,
    /// Counter for periodic cleanup.
    obs_count: u64,
    /// Cleanup every N observations.
    cleanup_interval: u64,
    /// Remove entries below this threshold during cleanup.
    cleanup_threshold: f64,
    /// Min total for IDF baseline at startup.
    min_total: f64,
}

impl StreamingIdf {
    pub(crate) fn new(idf_config: &sr_config::IdfConfig) -> Self {
        Self {
            freq: HashMap::new(),
            total: 0.0,
            decay_factor: idf_config.decay_factor,
            obs_count: 0,
            cleanup_interval: idf_config.cleanup_interval,
            cleanup_threshold: idf_config.cleanup_threshold,
            min_total: idf_config.min_total,
        }
    }

    /// Observe a set of terms from one event. Applies global decay then increments.
    pub(crate) fn observe(&mut self, terms: &HashSet<String>) {
        // Apply decay to all existing counts
        let d = self.decay_factor;
        self.total *= d;
        for v in self.freq.values_mut() {
            *v *= d;
        }

        // Increment observed terms
        for t in terms {
            *self.freq.entry(t.clone()).or_insert(0.0) += 1.0;
            self.total += 1.0;
        }

        self.obs_count += 1;

        // Periodic cleanup: remove negligible entries
        if self.cleanup_interval > 0 && self.obs_count.is_multiple_of(self.cleanup_interval) {
            let thresh = self.cleanup_threshold;
            self.freq.retain(|_, v| *v >= thresh);
        }
    }

    /// IDF score for a term: ln(total / (1 + freq)). Higher = rarer.
    pub(crate) fn score(&self, term: &str) -> f64 {
        let freq = self.freq.get(term).copied().unwrap_or(0.0);
        let total = self.total.max(self.min_total);
        (total / (1.0 + freq)).ln()
    }
}

// ---------------------------------------------------------------------------
// Burst detection — dual EWMA per topic
// ---------------------------------------------------------------------------

/// Tracks short-term vs long-term event rate per topic to detect bursts.
pub(crate) struct BurstDetector {
    /// Short-window EWMA
    short: HashMap<String, f64>,
    /// Long-window EWMA
    long: HashMap<String, f64>,
    /// Last update time for computing dt-based decay
    last_update: DateTime<Utc>,
    /// Counter for periodic cleanup
    obs_count: u64,
    /// Half-life parameters
    short_half_life_secs: f64,
    long_half_life_secs: f64,
    cleanup_threshold: f64,
    cleanup_interval: u64,
    /// Burst ratio -> anomaly score tiers: (ratio_threshold, anomaly_score)
    ratio_tiers: Vec<(f64, f64)>,
}

impl BurstDetector {
    pub(crate) fn new(burst_config: &sr_config::BurstConfig) -> Self {
        Self {
            short: HashMap::new(),
            long: HashMap::new(),
            last_update: Utc::now(),
            obs_count: 0,
            short_half_life_secs: burst_config.short_half_life_secs,
            long_half_life_secs: burst_config.long_half_life_secs,
            cleanup_threshold: burst_config.cleanup_threshold,
            cleanup_interval: burst_config.cleanup_interval,
            ratio_tiers: burst_config.ratio_tiers.clone(),
        }
    }

    /// Observe topics from one event and update both EWMA windows.
    pub(crate) fn observe(&mut self, topics: &HashSet<String>) {
        let now = Utc::now();
        let dt_secs = (now - self.last_update).num_milliseconds().max(1) as f64 / 1000.0;
        self.last_update = now;

        // Decay factors: alpha = 1 - exp(-ln(2) * dt / half_life)
        let alpha_short = 1.0 - (-0.693 * dt_secs / self.short_half_life_secs).exp();
        let alpha_long = 1.0 - (-0.693 * dt_secs / self.long_half_life_secs).exp();

        // Decay all existing values
        for v in self.short.values_mut() {
            *v *= 1.0 - alpha_short;
        }
        for v in self.long.values_mut() {
            *v *= 1.0 - alpha_long;
        }

        // Increment observed topics
        for t in topics {
            *self.short.entry(t.clone()).or_insert(0.0) += alpha_short;
            *self.long.entry(t.clone()).or_insert(0.0) += alpha_long;
        }

        self.obs_count += 1;
        if self.cleanup_interval > 0 && self.obs_count.is_multiple_of(self.cleanup_interval) {
            let thresh = self.cleanup_threshold;
            self.short.retain(|_, v| *v >= thresh);
            self.long.retain(|_, v| *v >= thresh);
        }
    }

    /// Burst ratio for a topic. > 2.0 means short-term rate is 2x the long-term baseline.
    pub(crate) fn burst_ratio(&self, topic: &str) -> f64 {
        let s = self.short.get(topic).copied().unwrap_or(0.0);
        let l = self.long.get(topic).copied().unwrap_or(0.0);
        if l < self.cleanup_threshold {
            if s > self.cleanup_threshold { 3.0 } else { 1.0 }
        } else {
            s / l
        }
    }

    /// Graduated anomaly score (0.0-1.0) based on burst ratio.
    pub(crate) fn anomaly_score(&self, topic: &str) -> f64 {
        let ratio = self.burst_ratio(topic);
        // ratio_tiers is sorted descending by ratio: [(3.0, 1.0), (2.0, 0.75), ...]
        // Use strict > to match original behavior: ratio <= 1.0 returns 0.0
        for &(threshold, score) in &self.ratio_tiers {
            if ratio > threshold {
                return score;
            }
        }
        0.0
    }

    /// Composite anomaly score across a set of topics (average of individual scores).
    pub(crate) fn composite_anomaly_score(&self, topics: &HashSet<String>) -> f64 {
        if topics.is_empty() { return 0.0; }
        let sum: f64 = topics.iter().map(|t| self.anomaly_score(t)).sum();
        sum / topics.len() as f64
    }
}

// ---------------------------------------------------------------------------
// Haversine distance
// ---------------------------------------------------------------------------

/// Haversine distance between two points in kilometres.
pub(crate) fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0; // Earth radius in km
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    R * c
}

// ---------------------------------------------------------------------------
// Entity normalization & filtering
// ---------------------------------------------------------------------------

/// Canonicalize common entity abbreviations to their full form
/// so that "DRC" matches "democratic republic of the congo", etc.
fn canonicalize_entity(name: &str) -> &str {
    match name {
        "drc" | "dr congo" => "democratic republic of the congo",
        "uae" => "united arab emirates",
        "us" | "usa" | "u.s." | "u.s.a." | "united states of america" => "united states",
        "uk" | "u.k." | "great britain" => "united kingdom",
        "dprk" | "north korea" => "democratic people's republic of korea",
        "rok" | "south korea" => "republic of korea",
        "prc" | "people's republic of china" => "china",
        "ksa" => "saudi arabia",
        "idf" => "israel defense forces",
        "irgc" => "islamic revolutionary guard corps",
        "nato" => "north atlantic treaty organization",
        "eu" | "european union" => "european union",
        "hamas" | "hamas movement" => "hamas",
        "hezbollah" | "hizbollah" | "hizballah" => "hezbollah",
        _ => name,
    }
}

pub(crate) fn normalize_entity(name: &str) -> String {
    let mut s = name.trim().to_lowercase();
    // Strip corporate/org suffixes
    for suffix in &[
        " ltd", " llc", " co.", " inc", " pjs", " pjsc", " corp",
        " gmbh", " s.a.", " sa", " ag", " plc", " pty", " bv",
        " nv", " srl", " spa", " ab", " oy", " as",
    ] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
        }
    }
    // Collapse separators
    s = s.replace(['_', '-'], " ");
    // Trim trailing punctuation
    s = s.trim_end_matches(['.', ',', ';', ':', '!', '?']).to_string();
    let s = s.trim().to_string();
    // Apply canonical aliases
    canonicalize_entity(&s).to_string()
}

pub(crate) fn is_entity_worthy(event: &InsertableEvent, name: &str) -> bool {
    let lower = name.to_lowercase();
    // Reject very short names
    if lower.len() < 3 {
        return false;
    }
    // Reject Shodan org names (ISP/hosting orgs from banner scanning)
    if event.source_type == SourceType::Shodan {
        // Only accept if it's a known actor/country, not an ISP org
        if event.payload.get("org").and_then(|v| v.as_str()).is_some_and(|org| {
            org.eq_ignore_ascii_case(name)
        }) {
            return false;
        }
    }
    // Reject flight callsigns (e.g., RCH1234, FORTE12)
    if event.source_type.is_flight_source() {
        let bytes = name.as_bytes();
        if bytes.len() >= 3 && bytes.len() <= 8
            && bytes.iter().take(4).all(|b| b.is_ascii_uppercase())
            && bytes.iter().skip(2).any(|b| b.is_ascii_digit())
        {
            return false;
        }
    }
    // Reject generic ISP/hosting names (unless tagged with disruption)
    let isp_keywords = ["telecom", "broadband", "hosting", "datacenter", "data center",
                        "colocation", "communications", "isp", "wireless", "mobile"];
    let has_disruption = event.tags.iter().any(|t|
        t.contains("disruption") || t.contains("outage") || t.contains("censorship")
    );
    if !has_disruption && isp_keywords.iter().any(|kw| lower.contains(kw)) {
        return false;
    }
    // Reject news organization names -- these appear in many articles but carry no signal
    let news_orgs = [
        "deutsche welle", "reuters", "associated press", "bbc", "al jazeera",
        "cnn", "fox news", "new york times", "washington post", "guardian",
        "france 24", "dw news", "sky news", "bloomberg", "afp",
    ];
    if news_orgs.iter().any(|org| lower == *org) {
        return false;
    }
    // Reject Telegram channel names -- these are the source, not intelligence entities
    let telegram_channels = [
        "war monitor", "noelreports", "intel slava z", "sitrep", "clash report",
        "deepstate english", "rybar english", "conflict intelligence team",
        "geoconfirmed", "intelsky", "ukraine military intel", "abu ali express",
        "cumta red alerts", "houthi military media",
    ];
    if telegram_channels.iter().any(|ch| lower == *ch) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Convert a normalized lowercase string to title case for display.
pub(crate) fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Map ISO 3166-1 alpha-2 region codes to readable names.
pub(crate) fn region_code_to_name(code: &str) -> String {
    match code.to_uppercase().as_str() {
        "AF" => "Afghanistan", "AL" => "Albania", "DZ" => "Algeria",
        "AO" => "Angola", "AR" => "Argentina", "AM" => "Armenia",
        "AU" => "Australia", "AT" => "Austria", "AZ" => "Azerbaijan",
        "BH" => "Bahrain", "BD" => "Bangladesh", "BY" => "Belarus",
        "BE" => "Belgium", "BR" => "Brazil", "BG" => "Bulgaria",
        "MM" => "Myanmar", "KH" => "Cambodia", "CM" => "Cameroon",
        "CA" => "Canada", "CF" => "Central African Republic",
        "TD" => "Chad", "CL" => "Chile", "CN" => "China",
        "CO" => "Colombia", "CD" => "DR Congo", "CG" => "Congo",
        "HR" => "Croatia", "CU" => "Cuba", "CY" => "Cyprus",
        "CZ" => "Czechia", "DK" => "Denmark", "DJ" => "Djibouti",
        "EC" => "Ecuador", "EG" => "Egypt", "SV" => "El Salvador",
        "ER" => "Eritrea", "EE" => "Estonia", "ET" => "Ethiopia",
        "FI" => "Finland", "FR" => "France", "GE" => "Georgia",
        "DE" => "Germany", "GH" => "Ghana", "GR" => "Greece",
        "GT" => "Guatemala", "GN" => "Guinea", "HT" => "Haiti",
        "HN" => "Honduras", "HK" => "Hong Kong", "HU" => "Hungary",
        "IS" => "Iceland", "IN" => "India", "ID" => "Indonesia",
        "IR" => "Iran", "IQ" => "Iraq", "IE" => "Ireland",
        "IL" => "Israel", "IT" => "Italy", "JP" => "Japan",
        "JO" => "Jordan", "KZ" => "Kazakhstan", "KE" => "Kenya",
        "KP" => "North Korea", "KR" => "South Korea", "KW" => "Kuwait",
        "KG" => "Kyrgyzstan", "LA" => "Laos", "LV" => "Latvia",
        "LB" => "Lebanon", "LY" => "Libya", "LT" => "Lithuania",
        "MG" => "Madagascar", "MY" => "Malaysia", "ML" => "Mali",
        "MX" => "Mexico", "MD" => "Moldova", "MN" => "Mongolia",
        "ME" => "Montenegro", "MA" => "Morocco", "MZ" => "Mozambique",
        "NA" => "Namibia", "NP" => "Nepal", "NL" => "Netherlands",
        "NZ" => "New Zealand", "NI" => "Nicaragua", "NE" => "Niger",
        "NG" => "Nigeria", "NO" => "Norway", "OM" => "Oman",
        "PK" => "Pakistan", "PS" => "Palestine", "PA" => "Panama",
        "PG" => "Papua New Guinea", "PY" => "Paraguay", "PE" => "Peru",
        "PH" => "Philippines", "PL" => "Poland", "PT" => "Portugal",
        "QA" => "Qatar", "RO" => "Romania", "RU" => "Russia",
        "RW" => "Rwanda", "SA" => "Saudi Arabia", "SN" => "Senegal",
        "RS" => "Serbia", "SG" => "Singapore", "SK" => "Slovakia",
        "SI" => "Slovenia", "SO" => "Somalia", "ZA" => "South Africa",
        "SS" => "South Sudan", "ES" => "Spain", "LK" => "Sri Lanka",
        "SD" => "Sudan", "SE" => "Sweden", "CH" => "Switzerland",
        "SY" => "Syria", "TW" => "Taiwan", "TJ" => "Tajikistan",
        "TZ" => "Tanzania", "TH" => "Thailand", "TN" => "Tunisia",
        "TR" => "Turkey", "TM" => "Turkmenistan", "UG" => "Uganda",
        "UA" => "Ukraine", "AE" => "UAE", "GB" => "UK",
        "US" => "United States", "UY" => "Uruguay", "UZ" => "Uzbekistan",
        "VE" => "Venezuela", "VN" => "Vietnam", "YE" => "Yemen",
        "ZM" => "Zambia", "ZW" => "Zimbabwe",
        other => return other.to_string(),
    }
    .to_string()
}

/// Returns true for event types that represent high-signal intelligence (conflict,
/// news, geo-confirmed). These are weighted more heavily in regen triggers.
/// Moderate-signal types (NOTAMs, thermal, GPS, BGP) return false.
pub(crate) fn is_high_signal_event(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::ConflictEvent
            | EventType::NewsArticle
            | EventType::GeoNews
            | EventType::GeoEvent
            | EventType::NuclearEvent
            | EventType::SeismicEvent
            | EventType::CensorshipEvent
            | EventType::TelegramMessage
            | EventType::BlueskyPost
            | EventType::ThreatIntel
    )
}

/// Normalize region code abbreviations to canonical long form.
/// Sources use inconsistent codes: RSS uses "ME", region_for_country returns "middle-east",
/// FIRMS uses underscores ("east_asia").
pub(crate) fn normalize_region(code: &str) -> &str {
    match code {
        "ME" => "middle-east",
        "EE" => "eastern-europe",
        "WE" => "western-europe",
        "AF" => "africa",       // continent, not Afghanistan (which is a country code)
        "SEA" => "southeast-asia",
        "EA" => "east-asia",
        "SA" if code.len() == 2 => "south-asia", // distinguish from Saudi Arabia
        "NA" => "north-america",
        "LA" => "south-america",
        // Common country codes → region normalization
        "RU" | "UA" | "BY" | "MD" | "PL" | "RO" | "BG" | "HU" | "CZ" | "SK" => "eastern-europe",
        "AS" | "JP" | "KR" | "CN" | "TW" | "MN" => "east-asia",
        "DE" | "FR" | "GB" | "IT" | "ES" | "NL" | "BE" | "AT" | "CH" | "SE" | "NO" | "DK" | "FI" | "IE" | "PT" => "western-europe",
        "US" | "CA" | "MX" => "north-america",
        "BR" | "AR" | "CO" | "VE" | "CL" | "PE" | "EC" | "BO" | "UY" | "PY" => "south-america",
        "IN" | "PK" | "BD" | "LK" | "NP" => "south-asia",
        "TH" | "VN" | "PH" | "MM" | "KH" | "MY" | "SG" | "ID" => "southeast-asia",
        "KZ" | "UZ" | "TM" | "KG" | "TJ" => "central-asia",
        "AU" | "NZ" | "FJ" | "PG" => "oceania",
        "IR" | "IQ" | "SY" | "JO" | "LB" | "IL" | "PS" | "YE" | "OM" | "AE" | "QA" | "BH" | "KW" | "TR" => "middle-east",
        // FIRMS underscore variants
        "east_asia" => "east-asia",
        "south_asia" => "south-asia",
        "middle_east" => "middle-east",
        "north_america" => "north-america",
        "south_america" => "south-america",
        "southeast_asia" => "southeast-asia",
        "eastern_europe" => "eastern-europe",
        "western_europe" => "western-europe",
        "central_asia" => "central-asia",
        "north_africa" => "north-africa",
        "central_america" => "central-america",
        _ => code,
    }
}

/// Check if two region code sets overlap, normalizing abbreviations.
pub(crate) fn regions_overlap(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    for ra in a {
        let na = normalize_region(ra);
        for rb in b {
            let nb = normalize_region(rb);
            if na == nb {
                return true;
            }
        }
    }
    false
}

/// Topics that indicate active armed conflict -- used for cluster severity escalation.
pub(crate) fn is_conflict_topic(topic: &str) -> bool {
    let lower = topic.to_lowercase();
    let patterns = [
        "war", "conflict", "strike", "attack", "bombing", "shelling",
        "missile", "airstrike", "frontline", "casualties", "killed",
        "combat", "offensive", "invasion", "battle", "siege",
        "armed-conflict", "military-strike", "drone-strike",
        "cross-border", "artillery", "rocket",
    ];
    patterns.iter().any(|p| lower.contains(p))
}

/// Returns true if the topic belongs to a natural-disaster phenomenon domain.
/// These are genuinely global and should merge at relaxed cross-region thresholds.
pub(crate) fn is_natural_disaster_topic(topic: &str) -> bool {
    let lower = topic.to_lowercase();
    // Exact matches for short/ambiguous terms
    if matches!(lower.as_str(), "fire" | "fires" | "blaze") {
        return true;
    }
    [
        "wildfire", "forest-fire", "bushfire", "drought", "flood", "cyclone",
        "hurricane", "typhoon", "tornado", "earthquake", "tsunami", "volcanic",
        "eruption", "thermal-anomaly", "fire-activity", "natural-disaster",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// Words that are generic category descriptors rather than specific situation identifiers.
/// Used by topical orphaning to avoid false title overlap (e.g., "Central Africa Wildfires"
/// keeping "Thailand Wildfires" as a child just because both contain "wildfires").
pub(crate) const GENERIC_TITLE_WORDS: &[&str] = &[
    "wildfires", "wildfire", "fires", "fire", "earthquake", "earthquakes",
    "conflict", "crisis", "activity", "sequence", "swarm", "surge",
    "operations", "military", "regional", "multi-region", "impact",
    "flood", "floods", "flooding", "cyclone", "hurricane", "typhoon",
    "tornado", "tsunami", "volcanic", "eruption", "drought",
    "forest", "bushfire",
];

/// Returns true if the title text contains natural-disaster keywords.
/// Used alongside topic-based detection to catch situations like "Nigeria Forest Fires".
pub(crate) fn is_natural_disaster_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    [
        "wildfire", "wildfires", "forest fire", "bushfire", "earthquake",
        "tsunami", "volcanic", "eruption", "cyclone", "hurricane",
        "typhoon", "tornado", "flood",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// Conflict-indicating source types.
pub(crate) fn is_conflict_source(st: SourceType) -> bool {
    matches!(
        st,
        SourceType::Acled | SourceType::Geoconfirmed | SourceType::Gdelt | SourceType::GdeltGeo
    )
}

/// Cyber-indicating source types.
pub(crate) fn is_cyber_source(st: SourceType) -> bool {
    matches!(
        st,
        SourceType::Cloudflare
            | SourceType::CloudflareBgp
            | SourceType::Ioda
            | SourceType::Bgp
            | SourceType::Otx
            | SourceType::Certstream
            | SourceType::Ooni
            | SourceType::Shodan
    )
}

/// Reject generic enrichment topics that connect nearly everything.
pub(crate) fn is_generic_topic(topic: &str) -> bool {
    let prefixes = [
        "regional-", "geopolitical-", "international-", "strategic-",
        "military-", "global-", "middle-east-", "political-",
    ];
    prefixes.iter().any(|p| topic.starts_with(p))
}

/// Reject language names/codes that come from enrichment `original_language`
/// and source metadata -- these cause unrelated events to merge.
pub(crate) fn is_language_tag(tag: &str) -> bool {
    matches!(
        tag,
        "albanian" | "arabic" | "armenian" | "azerbaijani" | "bengali"
        | "bulgarian" | "burmese" | "chinese" | "croatian" | "czech"
        | "danish" | "dutch" | "english" | "estonian" | "finnish"
        | "french" | "german" | "greek" | "hebrew" | "hindi"
        | "hungarian" | "indonesian" | "italian" | "japanese" | "korean"
        | "kurdish" | "latvian" | "lithuanian" | "malay" | "malayalam"
        | "norwegian" | "pashto" | "persian" | "polish" | "portuguese"
        | "romanian" | "russian" | "serbian" | "sinhalese" | "slovak"
        | "slovenian" | "somali" | "spanish" | "swahili" | "swedish"
        | "tamil" | "telugu" | "thai" | "turkish" | "ukrainian"
        | "urdu" | "uzbek" | "vietnamese"
    )
}

/// Effective source diversity: collapses related sources into single categories
/// so they don't inflate diversity scores:
///   - Flight sources (airplaneslive, adsb-fi, adsb-lol, opensky) → 1 "flight"
///   - News sources (gdelt, gdelt-geo, rss-news) → 1 "news"
///   - Disaster catalogs (gdacs, copernicus) → 1 "disaster"
pub(crate) fn effective_source_diversity(source_types: &HashSet<SourceType>) -> usize {
    let mut count = 0;
    let mut has_flight = false;
    let mut has_news = false;
    let mut has_disaster_catalog = false;
    for st in source_types {
        if st.is_flight_source() {
            if !has_flight {
                has_flight = true;
                count += 1;
            }
        } else if matches!(st, SourceType::Gdelt | SourceType::GdeltGeo | SourceType::RssNews) {
            if !has_news {
                has_news = true;
                count += 1;
            }
        } else if matches!(st, SourceType::Gdacs | SourceType::Copernicus) {
            if !has_disaster_catalog {
                has_disaster_catalog = true;
                count += 1;
            }
        } else {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

/// Pull entity names from an event using multiple signals, then normalize
/// and filter out low-quality entities (ISP orgs, callsigns, etc.).
pub(crate) fn extract_entities(event: &InsertableEvent, max_per_event: usize) -> HashSet<String> {
    // Position telemetry (flights, vessels) has no semantic content for clustering.
    // Tags like "military" or "high_value" are operational metadata, not intelligence
    // entities. These events still reach correlation rules via CorrelationWindow.
    if matches!(
        event.event_type,
        EventType::FlightPosition | EventType::VesselPosition
    ) {
        return HashSet::new();
    }

    let mut raw = HashSet::new();

    // 1. entity_name
    if let Some(ref name) = event.entity_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            raw.insert(trimmed.to_string());
        }
    }

    // 2. Tags starting with "actor:"
    for tag in &event.tags {
        if let Some(actor) = tag.strip_prefix("actor:") {
            let trimmed = actor.trim();
            if !trimmed.is_empty() {
                raw.insert(trimmed.to_string());
            }
        }
    }

    // 3. GeoEvent (GeoConfirmed): extract from conflict field in payload
    if event.event_type == EventType::GeoEvent {
        if let Some(conflict) = event.payload.get("conflict").and_then(|v| v.as_str()) {
            let trimmed = conflict.trim().to_string();
            if !trimmed.is_empty() {
                raw.insert(trimmed);
            }
        }
    }

    // 4. payload.enrichment.entities (array of objects with "name")
    // Cap entities per event to prevent over-merging from generic enrichment
    if let Some(enrichment) = event.payload.get("enrichment")
        && let Some(entities_arr) = enrichment.get("entities").and_then(|v| v.as_array())
    {
        for entry in entities_arr.iter().take(max_per_event) {
            if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    raw.insert(trimmed.to_string());
                }
            }
        }
    }

    // Normalize and filter
    raw.into_iter()
        .map(|name| {
            let normalized = normalize_entity(&name);
            (name, normalized)
        })
        .filter(|(original, _)| is_entity_worthy(event, original))
        .map(|(_, normalized)| normalized)
        .filter(|n| !n.is_empty())
        .collect()
}

pub(crate) const GENERIC_TOPICS: &[&str] = &[
    "diplomatic-rhetoric", "tariff-dispute", "defense-spending", "eu-politics",
    "trade-policy", "armed-conflict", "military-activity", "geopolitical-tensions",
    "international-relations", "security-concerns", "diplomatic-relations",
    "diplomatic-negotiations", "activism", "cultural-event",
    // Sports/entertainment -- not intelligence-relevant
    "baseball", "football", "soccer", "basketball", "sports", "entertainment",
    "celebrity", "music", "film",
];

/// Pull topic strings from an event using multiple signals.
pub(crate) fn extract_topics(event: &InsertableEvent, max_enrichment_topics: usize) -> HashSet<String> {
    // Position telemetry -- same rationale as extract_entities().
    if matches!(
        event.event_type,
        EventType::FlightPosition | EventType::VesselPosition
    ) {
        return HashSet::new();
    }

    let mut out = HashSet::new();

    // 1. payload.enrichment.topics (array of strings) -- cap at 3 most specific
    if let Some(enrichment) = event.payload.get("enrichment")
        && let Some(topics_arr) = enrichment.get("topics").and_then(|v| v.as_array())
    {
        // Take only the most specific enrichment topics
        // to avoid over-merging from generic topics like "regional-tensions"
        for entry in topics_arr.iter().take(max_enrichment_topics) {
            if let Some(t) = entry.as_str() {
                let trimmed = t.trim();
                if !trimmed.is_empty() && !is_generic_topic(trimmed) {
                    out.insert(trimmed.to_string());
                }
            }
        }
    }

    // 2. Tags -- prefixed "topic:" tags, plus unprefixed tags that look like topics
    for tag in &event.tags {
        if let Some(topic) = tag.strip_prefix("topic:") {
            let trimmed = topic.trim();
            if !trimmed.is_empty() {
                out.insert(trimmed.to_string());
            }
        } else if tag.strip_prefix("actor:").is_none()
            && tag.strip_prefix("source:").is_none()
            && tag.strip_prefix("query:").is_none()
        {
            // Unprefixed tags (e.g. "Ukraine", "military", "cyber") are useful topic signals,
            // but skip language names and generic source tags
            let trimmed = tag.trim().to_lowercase();
            if trimmed.len() >= 3 && !is_language_tag(&trimmed) {
                out.insert(trimmed);
            }
        }
    }

    // Filter out generic topics that connect nearly everything
    out.retain(|t| !GENERIC_TOPICS.contains(&t.as_str()));

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine() {
        // London (51.5074, -0.1278) to Paris (48.8566, 2.3522) ~ 344 km
        let d = distance_km(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((d - 344.0).abs() < 5.0);
    }

    #[test]
    fn test_streaming_idf_rare_terms_score_higher() {
        let mut idf = StreamingIdf::new(&sr_config::IdfConfig::default());
        let common: HashSet<String> = ["ukraine"].iter().map(|s| s.to_string()).collect();
        let rare: HashSet<String> = ["uss_nimitz"].iter().map(|s| s.to_string()).collect();

        // Observe "ukraine" 100 times
        for _ in 0..100 {
            idf.observe(&common);
        }
        // Observe "uss_nimitz" once
        idf.observe(&rare);

        let common_score = idf.score("ukraine");
        let rare_score = idf.score("uss_nimitz");
        assert!(rare_score > common_score, "Rare term should have higher IDF: {rare_score} > {common_score}");
    }

    #[test]
    fn test_burst_detector_detects_spike() {
        let mut bd = BurstDetector::new(&sr_config::BurstConfig::default());
        let topics: HashSet<String> = ["cyber-attack"].iter().map(|s| s.to_string()).collect();

        // Simulate many observations to build baseline
        for _ in 0..50 {
            bd.observe(&topics);
        }

        // After sustained observations, ratio should be around 1.0
        let ratio = bd.burst_ratio("cyber-attack");
        // Just verify it returns a reasonable value (short/long converge)
        assert!(ratio > 0.5 && ratio < 5.0, "Expected reasonable ratio after sustained activity, got {ratio}");
    }

    #[test]
    fn test_anomaly_score_graduated() {
        let mut bd = BurstDetector::new(&sr_config::BurstConfig::default());

        // No observations -> burst_ratio returns 1.0 for unknown topic -> anomaly 0.0
        assert_eq!(bd.anomaly_score("unknown"), 0.0, "Unknown quiet topic should be 0.0");

        let topics: HashSet<String> = ["cyber"].iter().map(|s| s.to_string()).collect();
        for _ in 0..500 {
            bd.observe(&topics);
        }
        let ratio = bd.burst_ratio("cyber");
        assert!(ratio >= 3.0, "500 rapid observations should produce burst ratio >= 3.0, got {}", ratio);
        assert!(bd.anomaly_score("cyber") >= 0.75, "High burst ratio should give anomaly >= 0.75");

        assert_eq!(bd.anomaly_score("quiet"), 0.0, "Unobserved topic should be 0.0");

        let mut bd2 = BurstDetector::new(&sr_config::BurstConfig::default());
        let base_topics: HashSet<String> = ["stable"].iter().map(|s| s.to_string()).collect();
        for _ in 0..200 {
            bd2.observe(&base_topics);
        }
        let stable_ratio = bd2.burst_ratio("stable");
        let stable_score = bd2.anomaly_score("stable");
        if stable_ratio <= 1.0 {
            assert_eq!(stable_score, 0.0);
        } else if stable_ratio <= 1.5 {
            assert_eq!(stable_score, 0.25);
        } else if stable_ratio <= 2.0 {
            assert_eq!(stable_score, 0.5);
        } else if stable_ratio <= 3.0 {
            assert_eq!(stable_score, 0.75);
        } else {
            assert_eq!(stable_score, 1.0);
        }

        let mut bd3 = BurstDetector::new(&sr_config::BurstConfig::default());
        let hot: HashSet<String> = ["hot"].iter().map(|s| s.to_string()).collect();
        for _ in 0..500 {
            bd3.observe(&hot);
        }
        let hot_score = bd3.anomaly_score("hot");
        assert!(hot_score > 0.0, "Hot topic should have positive anomaly score");

        let mixed: HashSet<String> = ["hot", "cold"].iter().map(|s| s.to_string()).collect();
        let composite = bd3.composite_anomaly_score(&mixed);
        assert!((composite - hot_score / 2.0).abs() < 0.01,
            "Composite of ({}, 0.0) should be ~{}, got {}", hot_score, hot_score / 2.0, composite);

        let empty: HashSet<String> = HashSet::new();
        assert_eq!(bd3.composite_anomaly_score(&empty), 0.0, "Empty topics should return 0.0");

        let single: HashSet<String> = ["hot"].iter().map(|s| s.to_string()).collect();
        assert_eq!(bd3.composite_anomaly_score(&single), hot_score,
            "Single-topic composite should equal individual score");
    }
}
