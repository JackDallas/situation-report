use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, info, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::common;
use crate::rate_limit::AuthError;
use crate::{DataSource, InsertableEvent, SourceContext};

/// ReliefWeb API v2 base URL.
const API_BASE: &str = "https://api.reliefweb.int/v2";

/// Application name sent to ReliefWeb API for tracking.
/// Must be pre-approved via https://apidoc.reliefweb.int/parameters#appname
/// Register at: https://docs.google.com/forms/d/e/1FAIpQLScR5EE_SBhweLLg_2xMCnXNbT6md4zxqIB00OL0yZWyrqX_Nw/viewform
fn app_name() -> Option<String> {
    std::env::var("RELIEFWEB_APPNAME").ok()
}

/// Maximum reports to fetch per poll cycle.
const REPORTS_LIMIT: u32 = 50;

/// Maximum disasters to fetch per poll cycle.
const DISASTERS_LIMIT: u32 = 20;

/// How often to poll the disasters endpoint (every N report polls).
/// Reports are polled every cycle; disasters every DISASTER_POLL_INTERVAL cycles.
const DISASTER_POLL_INTERVAL: u32 = 6;

// ---------------------------------------------------------------------------
// API response types
// ---------------------------------------------------------------------------

/// Top-level API response envelope.
#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    data: Vec<ApiItem>,
}

/// A single item (report or disaster) in the API response.
#[derive(Debug, Deserialize)]
struct ApiItem {
    id: String,
    #[serde(default)]
    fields: ApiFields,
}

/// Fields returned for reports and disasters.
/// Not all fields are present on all item types.
#[derive(Debug, Default, Deserialize)]
struct ApiFields {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    date: Option<DateFields>,
    #[serde(default)]
    country: Vec<CountryField>,
    #[serde(default)]
    primary_country: Option<CountryField>,
    #[serde(default)]
    source: Vec<SourceField>,
    #[serde(default)]
    theme: Vec<NameField>,
    #[serde(default)]
    disaster: Vec<DisasterField>,
    #[serde(default)]
    format: Vec<NameField>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    url_alias: Option<String>,
    #[serde(default, rename = "body-html")]
    body_html: Option<String>,
    // Disaster-specific fields
    #[serde(default)]
    glide: Option<String>,
    #[serde(default, rename = "type")]
    disaster_types: Vec<NameField>,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
struct DateFields {
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    changed: Option<String>,
    #[serde(default)]
    original: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[allow(dead_code)]
struct CountryField {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    iso3: Option<String>,
    #[serde(default)]
    id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct SourceField {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    shortname: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct NameField {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DisasterField {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    glide: Option<String>,
    #[serde(default, rename = "type")]
    disaster_types: Vec<NameField>,
}

// ---------------------------------------------------------------------------
// Source implementation
// ---------------------------------------------------------------------------

/// ReliefWeb (UN OCHA) data source.
///
/// Polls the ReliefWeb API v2 for humanitarian reports and disaster declarations.
/// Reports include situation reports, assessments, press releases, and analysis
/// from UN agencies, NGOs, and governments worldwide.
pub struct ReliefwebSource {
    /// Dedup set of report/disaster IDs already processed.
    seen: Mutex<HashSet<String>>,
    /// Cycle counter for interleaving disaster polls.
    poll_count: Mutex<u32>,
    /// High-water mark: most recent report creation date seen.
    watermark: Mutex<Option<String>>,
}

impl Default for ReliefwebSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ReliefwebSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
            poll_count: Mutex::new(0),
            watermark: Mutex::new(None),
        }
    }

    /// Fetch reports from the ReliefWeb API.
    async fn fetch_reports(
        &self,
        ctx: &SourceContext,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let watermark = {
            let wm = self.watermark.lock().unwrap_or_else(|e| e.into_inner());
            wm.clone()
        };

        // Build POST body for reports query
        let mut filter = serde_json::json!({
            "field": "status",
            "value": ["published"],
            "operator": "OR"
        });

        // If we have a watermark, add date filter
        if let Some(ref wm) = watermark {
            filter = serde_json::json!({
                "operator": "AND",
                "conditions": [
                    {
                        "field": "status",
                        "value": ["published"],
                        "operator": "OR"
                    },
                    {
                        "field": "date.created",
                        "value": { "from": wm }
                    }
                ]
            });
        }

        let body = serde_json::json!({
            "limit": REPORTS_LIMIT,
            "fields": {
                "include": [
                    "title", "date", "country", "primary_country",
                    "source", "theme", "disaster", "format",
                    "body-html", "url_alias", "status"
                ]
            },
            "filter": filter,
            "sort": ["date.created:desc"]
        });

        let appname = app_name().ok_or_else(|| {
            AuthError {
                source: "reliefweb".into(),
                message: "RELIEFWEB_APPNAME env var not set. Register at https://apidoc.reliefweb.int/parameters#appname".into(),
            }
        })?;
        let url = format!("{}/reports?appname={}", API_BASE, appname);
        debug!(url = %url, "Polling ReliefWeb reports");

        let resp = ctx.http
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AuthError {
                source: "reliefweb".into(),
                message: format!("HTTP {}: {}", status, &body_text[..body_text.len().min(200)]),
            }.into());
        }
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "ReliefWeb reports API returned HTTP {}: {}",
                status,
                &body_text[..body_text.len().min(200)]
            );
        }

        let api_resp: ApiResponse = resp.json().await?;
        let mut events = Vec::new();
        let mut newest_date: Option<String> = watermark.clone();

        for item in &api_resp.data {
            let source_id = format!("reliefweb:{}", item.id);

            // Dedup
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&source_id) {
                    continue;
                }
                seen.insert(source_id.clone());
                if seen.len() > 5000 {
                    seen.clear();
                    seen.insert(source_id.clone());
                }
            }

            let title = item.fields.title.as_deref().unwrap_or("Untitled Report");

            // Parse event time
            let event_time = item.fields.date.as_ref()
                .and_then(|d| d.created.as_deref())
                .and_then(parse_iso_date)
                .unwrap_or_else(Utc::now);

            // Track newest created date for watermark
            if let Some(ref created) = item.fields.date.as_ref()
                .and_then(|d| d.created.clone())
            {
                match &newest_date {
                    Some(nd) if created > nd => {
                        newest_date = Some(created.clone());
                    }
                    None => {
                        newest_date = Some(created.clone());
                    }
                    _ => {}
                }
            }

            // Determine primary country for geocoding
            let primary_country = item.fields.primary_country.as_ref()
                .or_else(|| item.fields.country.first());

            let (latitude, longitude) = primary_country
                .and_then(|c| {
                    // Try country name first
                    c.name.as_deref()
                        .and_then(common::country_center_for_name)
                        // Then try ISO3 -> ISO2 conversion
                        .or_else(|| {
                            c.iso3.as_deref()
                                .and_then(iso3_to_iso2)
                                .and_then(common::country_center)
                        })
                })
                .map(|(lat, lon)| (Some(lat), Some(lon)))
                .unwrap_or((None, None));

            let region_code = latitude.zip(longitude)
                .and_then(|(lat, lon)| common::region_from_coords(lat, lon))
                .map(String::from);

            // Determine event type based on themes and disaster association
            let event_type = classify_event_type(&item.fields);

            // Determine severity
            let severity = classify_severity(&item.fields);

            // Build tags
            let tags = build_tags(&item.fields);

            // Build description from body excerpt (char-boundary-safe truncation)
            let description = item.fields.body_html.as_deref()
                .map(strip_html)
                .map(|s| {
                    if s.len() > 500 {
                        let mut end = 497;
                        while !s.is_char_boundary(end) && end > 0 {
                            end -= 1;
                        }
                        format!("{}...", &s[..end])
                    } else {
                        s
                    }
                });

            // Build payload
            let report_url = item.fields.url_alias.as_deref()
                .map(|alias| format!("https://reliefweb.int{}", alias))
                .unwrap_or_else(|| format!("https://reliefweb.int/node/{}", item.id));

            let source_orgs: Vec<&str> = item.fields.source.iter()
                .filter_map(|s| s.shortname.as_deref().or(s.name.as_deref()))
                .collect();

            let countries: Vec<&str> = item.fields.country.iter()
                .filter_map(|c| c.name.as_deref())
                .collect();

            let themes: Vec<&str> = item.fields.theme.iter()
                .filter_map(|t| t.name.as_deref())
                .collect();

            let formats: Vec<&str> = item.fields.format.iter()
                .filter_map(|f| f.name.as_deref())
                .collect();

            let disaster_refs: Vec<serde_json::Value> = item.fields.disaster.iter()
                .map(|d| serde_json::json!({
                    "name": d.name,
                    "glide": d.glide,
                    "types": d.disaster_types.iter()
                        .filter_map(|t| t.name.as_deref())
                        .collect::<Vec<_>>(),
                }))
                .collect();

            let payload = serde_json::json!({
                "url": report_url,
                "source_org": source_orgs,
                "format": formats,
                "themes": themes,
                "countries": countries,
                "disaster_refs": disaster_refs,
                "reliefweb_id": item.id,
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Reliefweb,
                source_id: Some(source_id),
                longitude,
                latitude,
                region_code,
                entity_id: None,
                entity_name: primary_country.and_then(|c| c.name.clone()),
                event_type,
                severity,
                confidence: Some(0.95), // UN-curated data is high confidence
                tags,
                title: Some(title.to_string()),
                description,
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        // Update watermark
        if let Some(nd) = newest_date {
            let mut wm = self.watermark.lock().unwrap_or_else(|e| e.into_inner());
            *wm = Some(nd);
        }

        Ok(events)
    }

    /// Fetch disaster declarations from the ReliefWeb API.
    async fn fetch_disasters(
        &self,
        ctx: &SourceContext,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        let body = serde_json::json!({
            "limit": DISASTERS_LIMIT,
            "fields": {
                "include": [
                    "name", "glide", "date", "status",
                    "country", "type", "url_alias"
                ]
            },
            "filter": {
                "field": "status",
                "value": ["current", "alert"],
                "operator": "OR"
            },
            "sort": ["date.created:desc"]
        });

        let appname = app_name().ok_or_else(|| {
            AuthError {
                source: "reliefweb".into(),
                message: "RELIEFWEB_APPNAME env var not set. Register at https://apidoc.reliefweb.int/parameters#appname".into(),
            }
        })?;
        let url = format!("{}/disasters?appname={}", API_BASE, appname);
        debug!(url = %url, "Polling ReliefWeb disasters");

        let resp = ctx.http
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AuthError {
                source: "reliefweb".into(),
                message: format!("HTTP {}: {}", status, &body_text[..body_text.len().min(200)]),
            }.into());
        }
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "ReliefWeb disasters API returned HTTP {}: {}",
                status,
                &body_text[..body_text.len().min(200)]
            );
        }

        let api_resp: ApiResponse = resp.json().await?;
        let mut events = Vec::new();

        for item in &api_resp.data {
            let source_id = format!("reliefweb:disaster:{}", item.id);

            // Dedup
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&source_id) {
                    continue;
                }
                seen.insert(source_id.clone());
            }

            let name = item.fields.name.as_deref()
                .or(item.fields.title.as_deref())
                .unwrap_or("Unnamed Disaster");

            let event_time = item.fields.date.as_ref()
                .and_then(|d| d.created.as_deref())
                .and_then(parse_iso_date)
                .unwrap_or_else(Utc::now);

            let primary_country = item.fields.country.first();

            let (latitude, longitude) = primary_country
                .and_then(|c| {
                    c.name.as_deref()
                        .and_then(common::country_center_for_name)
                        .or_else(|| {
                            c.iso3.as_deref()
                                .and_then(iso3_to_iso2)
                                .and_then(common::country_center)
                        })
                })
                .map(|(lat, lon)| (Some(lat), Some(lon)))
                .unwrap_or((None, None));

            let region_code = latitude.zip(longitude)
                .and_then(|(lat, lon)| common::region_from_coords(lat, lon))
                .map(String::from);

            // Disaster type classification
            let disaster_type_names: Vec<&str> = item.fields.disaster_types.iter()
                .filter_map(|t| t.name.as_deref())
                .collect();

            let event_type = classify_disaster_event_type(&disaster_type_names);
            let severity = classify_disaster_severity(
                item.fields.status.as_deref(),
                &item.fields.glide,
                &disaster_type_names,
            );

            let mut tags = vec!["reliefweb".to_string(), "disaster".to_string()];
            for dt in &disaster_type_names {
                tags.push(dt.to_lowercase().replace(' ', "-"));
            }
            for country in &item.fields.country {
                if let Some(ref cname) = country.name {
                    tags.push(format!("country:{}", cname));
                }
            }
            if item.fields.glide.is_some() {
                tags.push("glide".to_string());
            }
            if item.fields.status.as_deref() == Some("alert") {
                tags.push("alert".to_string());
            }

            let disaster_url = item.fields.url_alias.as_deref()
                .map(|alias| format!("https://reliefweb.int{}", alias))
                .unwrap_or_else(|| format!("https://reliefweb.int/node/{}", item.id));

            let countries: Vec<&str> = item.fields.country.iter()
                .filter_map(|c| c.name.as_deref())
                .collect();

            let payload = serde_json::json!({
                "url": disaster_url,
                "glide": item.fields.glide,
                "status": item.fields.status,
                "disaster_types": disaster_type_names,
                "countries": countries,
                "reliefweb_id": item.id,
                "item_type": "disaster",
            });

            events.push(InsertableEvent {
                event_time,
                source_type: SourceType::Reliefweb,
                source_id: Some(source_id),
                longitude,
                latitude,
                region_code,
                entity_id: None,
                entity_name: primary_country.and_then(|c| c.name.clone()),
                event_type,
                severity,
                confidence: Some(0.98), // Official disaster declarations
                tags,
                title: Some(name.to_string()),
                description: Some(format!("Disaster declaration: {}", name)),
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });
        }

        Ok(events)
    }
}

#[async_trait]
impl DataSource for ReliefwebSource {
    fn id(&self) -> &str {
        "reliefweb"
    }

    fn name(&self) -> &str {
        "ReliefWeb (UN OCHA)"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(600) // 10 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let mut all_events = Vec::new();

        // Always fetch reports
        match self.fetch_reports(ctx).await {
            Ok(events) => {
                if !events.is_empty() {
                    info!(count = events.len(), "Fetched ReliefWeb reports");
                }
                all_events.extend(events);
            }
            Err(e) => {
                // Propagate auth errors so the registry can park this source
                if crate::rate_limit::is_auth_error(&e) {
                    return Err(e);
                }
                warn!(error = %e, "Failed to fetch ReliefWeb reports");
            }
        }

        // Periodically fetch disasters (every N polls)
        let should_poll_disasters = {
            let mut count = self.poll_count.lock().unwrap_or_else(|e| e.into_inner());
            *count += 1;
            *count % DISASTER_POLL_INTERVAL == 0
        };

        if should_poll_disasters {
            match self.fetch_disasters(ctx).await {
                Ok(events) => {
                    if !events.is_empty() {
                        info!(count = events.len(), "Fetched ReliefWeb disasters");
                    }
                    all_events.extend(events);
                }
                Err(e) => {
                    if crate::rate_limit::is_auth_error(&e) {
                        return Err(e);
                    }
                    warn!(error = %e, "Failed to fetch ReliefWeb disasters");
                }
            }
        }

        Ok(all_events)
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Parse an ISO 8601 date string into a UTC DateTime.
fn parse_iso_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            // Try without timezone (some ReliefWeb dates lack the Z suffix)
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}

/// Convert ISO 3166-1 alpha-3 to alpha-2 country code.
fn iso3_to_iso2(iso3: &str) -> Option<&'static str> {
    match iso3.to_uppercase().as_str() {
        "AFG" => Some("AF"),
        "BGD" => Some("BD"),
        "BDI" => Some("BI"),
        "BFA" => Some("BF"),
        "CAF" => Some("CF"),
        "CMR" => Some("CM"),
        "COD" => Some("CD"),
        "COL" => Some("CO"),
        "DJI" => Some("DJ"),
        "EGY" => Some("EG"),
        "ERI" => Some("ER"),
        "ETH" => Some("ET"),
        "GIN" => Some("GN"),
        "GTM" => Some("GT"),
        "HND" => Some("HN"),
        "HTI" => Some("HT"),
        "IDN" => Some("ID"),
        "IND" => Some("IN"),
        "IRN" => Some("IR"),
        "IRQ" => Some("IQ"),
        "ISR" => Some("IL"),
        "JOR" => Some("JO"),
        "KEN" => Some("KE"),
        "KHM" => Some("KH"),
        "LAO" => Some("LA"),
        "LBN" => Some("LB"),
        "LBR" => Some("LR"),
        "LBY" => Some("LY"),
        "LKA" => Some("LK"),
        "MDG" => Some("MG"),
        "MLI" => Some("ML"),
        "MMR" => Some("MM"),
        "MOZ" => Some("MZ"),
        "MWI" => Some("MW"),
        "NER" => Some("NE"),
        "NGA" => Some("NG"),
        "NPL" => Some("NP"),
        "PAK" => Some("PK"),
        "PHL" => Some("PH"),
        "PSE" => Some("PS"),
        "RWA" => Some("RW"),
        "SDN" => Some("SD"),
        "SLE" => Some("SL"),
        "SLV" => Some("SV"),
        "SOM" => Some("SO"),
        "SSD" => Some("SS"),
        "SYR" => Some("SY"),
        "TCD" => Some("TD"),
        "TUR" => Some("TR"),
        "UGA" => Some("UG"),
        "UKR" => Some("UA"),
        "VEN" => Some("VE"),
        "YEM" => Some("YE"),
        "ZWE" => Some("ZW"),
        // Additional common ones
        "USA" => Some("US"),
        "GBR" => Some("GB"),
        "FRA" => Some("FR"),
        "DEU" => Some("DE"),
        "CHN" => Some("CN"),
        "JPN" => Some("JP"),
        "RUS" => Some("RU"),
        "SAU" => Some("SA"),
        "BRA" => Some("BR"),
        _ => None,
    }
}

/// Classify event type based on report themes and disaster association.
fn classify_event_type(fields: &ApiFields) -> EventType {
    let themes: Vec<String> = fields.theme.iter()
        .filter_map(|t| t.name.as_ref())
        .map(|n| n.to_lowercase())
        .collect();

    let has_disaster = !fields.disaster.is_empty();

    // Check themes for conflict indicators
    let is_conflict = themes.iter().any(|t| {
        t.contains("conflict") || t.contains("armed") || t.contains("violence")
        || t.contains("protection") || t.contains("mine action")
        || t.contains("peacekeeping")
    });

    if is_conflict {
        return EventType::ConflictEvent;
    }

    // Natural disaster types map to geo_event
    if has_disaster {
        return EventType::GeoEvent;
    }

    // Check for epidemic/health themes
    let is_health = themes.iter().any(|t| {
        t.contains("health") || t.contains("epidemic") || t.contains("pandemic")
        || t.contains("cholera") || t.contains("ebola") || t.contains("covid")
    });

    if is_health {
        return EventType::GeoEvent;
    }

    // Default: humanitarian reports are news articles
    EventType::NewsArticle
}

/// Classify disaster event type based on disaster type names.
fn classify_disaster_event_type(disaster_types: &[&str]) -> EventType {
    for dt in disaster_types {
        let lower = dt.to_lowercase();
        if lower.contains("conflict") || lower.contains("violence")
            || lower.contains("complex emergency")
        {
            return EventType::ConflictEvent;
        }
    }
    // All other disaster types (earthquake, flood, cyclone, epidemic, etc.)
    EventType::GeoEvent
}

/// Classify severity based on report metadata.
fn classify_severity(fields: &ApiFields) -> Severity {
    let formats: Vec<String> = fields.format.iter()
        .filter_map(|f| f.name.as_ref())
        .map(|n| n.to_lowercase())
        .collect();

    let themes: Vec<String> = fields.theme.iter()
        .filter_map(|t| t.name.as_ref())
        .map(|n| n.to_lowercase())
        .collect();

    // Flash updates and emergency appeals are high severity
    let is_urgent = formats.iter().any(|f| {
        f.contains("flash update") || f.contains("emergency appeal")
        || f.contains("emergency plan")
    });
    if is_urgent {
        return Severity::High;
    }

    // Situation reports for active disasters are medium
    let is_sitrep = formats.iter().any(|f| {
        f.contains("situation report") || f.contains("sitrep")
    });

    // Key action / response themes bump severity
    let has_urgent_theme = themes.iter().any(|t| {
        t.contains("disaster") || t.contains("conflict") || t.contains("epidemic")
        || t.contains("famine") || t.contains("displacement")
    });

    if is_sitrep && has_urgent_theme {
        return Severity::Medium;
    }
    if is_sitrep || !fields.disaster.is_empty() {
        return Severity::Medium;
    }

    // Analysis, assessments, policy docs are low
    Severity::Low
}

/// Classify severity for disaster declarations.
fn classify_disaster_severity(
    status: Option<&str>,
    glide: &Option<String>,
    disaster_types: &[&str],
) -> Severity {
    // Alert status = high
    if status == Some("alert") {
        return Severity::High;
    }

    // GLIDE number means internationally recognized disaster
    if glide.is_some() {
        return Severity::High;
    }

    // Certain disaster types are inherently high severity
    let is_severe_type = disaster_types.iter().any(|dt| {
        let lower = dt.to_lowercase();
        lower.contains("earthquake") || lower.contains("tsunami")
        || lower.contains("cyclone") || lower.contains("typhoon")
        || lower.contains("hurricane") || lower.contains("volcano")
        || lower.contains("complex emergency")
    });

    if is_severe_type {
        return Severity::High;
    }

    Severity::Medium
}

/// Build tags from report fields.
fn build_tags(fields: &ApiFields) -> Vec<String> {
    let mut tags = vec!["reliefweb".to_string()];

    // Add format types (situation report, flash update, etc.)
    for f in &fields.format {
        if let Some(ref name) = f.name {
            tags.push(format!("format:{}", name.to_lowercase().replace(' ', "-")));
        }
    }

    // Add theme tags
    for t in &fields.theme {
        if let Some(ref name) = t.name {
            tags.push(name.to_lowercase().replace(' ', "-"));
        }
    }

    // Add country tags
    for c in &fields.country {
        if let Some(ref name) = c.name {
            tags.push(format!("country:{}", name));
        }
    }

    // Add source organization tags
    for s in &fields.source {
        if let Some(ref shortname) = s.shortname {
            tags.push(format!("source:{}", shortname));
        }
    }

    // Add disaster reference tags
    for d in &fields.disaster {
        if let Some(ref name) = d.name {
            tags.push(format!("disaster:{}", name));
        }
    }

    tags
}

/// Strip HTML tags from a string, producing plain text.
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut last_was_space = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // Add space after block elements
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if !in_tag => {
                if ch.is_whitespace() {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(ch);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Decode basic HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn parse_iso_date_rfc3339() {
        let dt = parse_iso_date("2026-03-04T12:30:00+00:00");
        assert!(dt.is_some());
        assert_eq!(dt.unwrap().year(), 2026);
    }

    #[test]
    fn parse_iso_date_without_tz() {
        let dt = parse_iso_date("2026-03-04T12:30:00");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_iso_date_invalid() {
        assert!(parse_iso_date("not-a-date").is_none());
        assert!(parse_iso_date("").is_none());
    }

    #[test]
    fn iso3_to_iso2_known() {
        assert_eq!(iso3_to_iso2("SYR"), Some("SY"));
        assert_eq!(iso3_to_iso2("AFG"), Some("AF"));
        assert_eq!(iso3_to_iso2("UKR"), Some("UA"));
        assert_eq!(iso3_to_iso2("YEM"), Some("YE"));
        assert_eq!(iso3_to_iso2("SDN"), Some("SD"));
        assert_eq!(iso3_to_iso2("SSD"), Some("SS"));
        assert_eq!(iso3_to_iso2("COD"), Some("CD"));
        assert_eq!(iso3_to_iso2("HTI"), Some("HT"));
    }

    #[test]
    fn iso3_to_iso2_case_insensitive() {
        assert_eq!(iso3_to_iso2("syr"), Some("SY"));
        assert_eq!(iso3_to_iso2("Afg"), Some("AF"));
    }

    #[test]
    fn iso3_to_iso2_unknown() {
        assert_eq!(iso3_to_iso2("XXX"), None);
        assert_eq!(iso3_to_iso2(""), None);
    }

    #[test]
    fn strip_html_basic() {
        assert_eq!(strip_html("<p>Hello world</p>"), "Hello world");
        assert_eq!(strip_html("<b>bold</b> text"), "bold text");
    }

    #[test]
    fn strip_html_entities() {
        assert_eq!(strip_html("A &amp; B"), "A & B");
        assert_eq!(strip_html("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn strip_html_nested() {
        assert_eq!(
            strip_html("<div><p>paragraph one</p><p>paragraph two</p></div>"),
            "paragraph one paragraph two"
        );
    }

    #[test]
    fn strip_html_empty() {
        assert_eq!(strip_html(""), "");
        assert_eq!(strip_html("<br/>"), "");
    }

    #[test]
    fn classify_conflict_themes() {
        let fields = ApiFields {
            theme: vec![
                NameField { name: Some("Protection and Human Rights".to_string()) },
                NameField { name: Some("Armed Conflict".to_string()) },
            ],
            ..Default::default()
        };
        assert_eq!(classify_event_type(&fields), EventType::ConflictEvent);
    }

    #[test]
    fn classify_disaster_themes() {
        let fields = ApiFields {
            disaster: vec![DisasterField {
                name: Some("Earthquake in Turkey".to_string()),
                glide: Some("EQ-2023-000015-TUR".to_string()),
                disaster_types: vec![NameField { name: Some("Earthquake".to_string()) }],
            }],
            ..Default::default()
        };
        assert_eq!(classify_event_type(&fields), EventType::GeoEvent);
    }

    #[test]
    fn classify_health_themes() {
        let fields = ApiFields {
            theme: vec![
                NameField { name: Some("Health".to_string()) },
                NameField { name: Some("Epidemic".to_string()) },
            ],
            ..Default::default()
        };
        assert_eq!(classify_event_type(&fields), EventType::GeoEvent);
    }

    #[test]
    fn classify_default_news() {
        let fields = ApiFields {
            theme: vec![
                NameField { name: Some("Food and Nutrition".to_string()) },
            ],
            ..Default::default()
        };
        assert_eq!(classify_event_type(&fields), EventType::NewsArticle);
    }

    #[test]
    fn severity_flash_update_is_high() {
        let fields = ApiFields {
            format: vec![NameField { name: Some("Flash Update".to_string()) }],
            ..Default::default()
        };
        assert_eq!(classify_severity(&fields), Severity::High);
    }

    #[test]
    fn severity_emergency_appeal_is_high() {
        let fields = ApiFields {
            format: vec![NameField { name: Some("Emergency Appeal".to_string()) }],
            ..Default::default()
        };
        assert_eq!(classify_severity(&fields), Severity::High);
    }

    #[test]
    fn severity_sitrep_with_disaster_is_medium() {
        let fields = ApiFields {
            format: vec![NameField { name: Some("Situation Report".to_string()) }],
            disaster: vec![DisasterField::default()],
            ..Default::default()
        };
        assert_eq!(classify_severity(&fields), Severity::Medium);
    }

    #[test]
    fn severity_analysis_is_low() {
        let fields = ApiFields {
            format: vec![NameField { name: Some("Analysis".to_string()) }],
            ..Default::default()
        };
        assert_eq!(classify_severity(&fields), Severity::Low);
    }

    #[test]
    fn disaster_severity_with_glide_is_high() {
        assert_eq!(
            classify_disaster_severity(
                Some("current"),
                &Some("EQ-2023-000015-TUR".to_string()),
                &["Earthquake"],
            ),
            Severity::High,
        );
    }

    #[test]
    fn disaster_severity_alert_is_high() {
        assert_eq!(
            classify_disaster_severity(Some("alert"), &None, &["Flood"]),
            Severity::High,
        );
    }

    #[test]
    fn disaster_severity_earthquake_is_high() {
        assert_eq!(
            classify_disaster_severity(Some("current"), &None, &["Earthquake"]),
            Severity::High,
        );
    }

    #[test]
    fn disaster_severity_default_is_medium() {
        assert_eq!(
            classify_disaster_severity(Some("current"), &None, &["Flood"]),
            Severity::Medium,
        );
    }

    #[test]
    fn disaster_event_type_conflict() {
        assert_eq!(
            classify_disaster_event_type(&["Complex Emergency"]),
            EventType::ConflictEvent,
        );
    }

    #[test]
    fn disaster_event_type_natural() {
        assert_eq!(
            classify_disaster_event_type(&["Earthquake", "Tsunami"]),
            EventType::GeoEvent,
        );
    }

    #[test]
    fn build_tags_includes_reliefweb() {
        let fields = ApiFields {
            country: vec![CountryField {
                name: Some("Syria".to_string()),
                iso3: Some("SYR".to_string()),
                id: Some(233),
            }],
            theme: vec![NameField { name: Some("Protection".to_string()) }],
            format: vec![NameField { name: Some("Situation Report".to_string()) }],
            source: vec![SourceField {
                name: Some("United Nations".to_string()),
                shortname: Some("UN".to_string()),
            }],
            ..Default::default()
        };
        let tags = build_tags(&fields);
        assert!(tags.contains(&"reliefweb".to_string()));
        assert!(tags.contains(&"country:Syria".to_string()));
        assert!(tags.contains(&"protection".to_string()));
        assert!(tags.contains(&"source:UN".to_string()));
        assert!(tags.contains(&"format:situation-report".to_string()));
    }

    #[test]
    fn default_interval_is_10_minutes() {
        let source = ReliefwebSource::new();
        assert_eq!(source.default_interval(), Duration::from_secs(600));
    }

    #[test]
    fn source_id_is_reliefweb() {
        let source = ReliefwebSource::new();
        assert_eq!(source.id(), "reliefweb");
    }

    #[test]
    fn source_name() {
        let source = ReliefwebSource::new();
        assert_eq!(source.name(), "ReliefWeb (UN OCHA)");
    }
}
