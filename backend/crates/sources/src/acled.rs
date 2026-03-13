use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use chrono::DateTime;

use sr_types::{EventType, Severity, SourceType};

use crate::common::{json_as_f64, region_for_country_name};
use crate::{DataSource, InsertableEvent, SourceContext};

/// Countries of interest for ACLED conflict data.
const COUNTRIES_FILTER: &str = "Iran|Israel|Ukraine|Russia|Yemen|Syria|Lebanon|Sudan|Myanmar";

/// ACLED data has roughly a 1-week lag, so we look back 10 days to be safe.
const LOOKBACK_DAYS: i64 = 10;

/// Maximum records per page from the ACLED API.
const PAGE_LIMIT: u32 = 5000;

/// OAuth2 token endpoint.
const TOKEN_URL: &str = "https://acleddata.com/oauth/token";

/// ACLED API base URL (new v3 endpoint).
const API_BASE: &str = "https://acleddata.com/api/acled/read";

pub struct AcledSource {
    /// Cached OAuth2 bearer token + expiry.
    cached_token: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: chrono::DateTime<Utc>,
}

/// OAuth2 token response.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Top-level JSON envelope returned by the ACLED read API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AcledResponse {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    data: Vec<serde_json::Value>,
    #[serde(default)]
    count: Option<u64>,
}

impl Default for AcledSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AcledSource {
    pub fn new() -> Self {
        Self {
            cached_token: Mutex::new(None),
        }
    }

    /// Get a valid bearer token, refreshing or re-authenticating as needed.
    async fn get_bearer_token(&self, http: &reqwest::Client) -> anyhow::Result<String> {
        // Check cached token
        let existing_refresh_token = {
            let cache = self.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref tok) = *cache {
                // Return cached token if still valid (with 5-minute safety margin)
                if tok.expires_at > Utc::now() + chrono::Duration::minutes(5) {
                    return Ok(tok.access_token.clone());
                }
                // Token expired — try to use refresh token first
                tok.refresh_token.clone()
            } else {
                None
            }
        };

        // Try refresh token first (valid for 14 days, avoids re-sending credentials)
        if let Some(ref refresh_tok) = existing_refresh_token {
            debug!("ACLED OAuth2 token expired, attempting refresh");
            match self.request_token_with_refresh(http, refresh_tok).await {
                Ok(access_token) => return Ok(access_token),
                Err(e) => {
                    warn!("ACLED refresh token failed, will re-authenticate: {e}");
                    // Fall through to full password grant
                }
            }
        }

        // Full password grant
        let email = std::env::var("ACLED_EMAIL")
            .map_err(|_| anyhow::anyhow!("ACLED_EMAIL not set"))?;
        let password = std::env::var("ACLED_PASSWORD")
            .map_err(|_| anyhow::anyhow!("ACLED_PASSWORD not set"))?;

        info!("Fetching new ACLED OAuth2 token via password grant");

        let resp = http
            .post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("username", email.as_str()),
                ("password", password.as_str()),
                ("grant_type", "password"),
                ("client_id", "acled"),
            ])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %body,
                "ACLED OAuth2 token request failed — check ACLED_EMAIL and ACLED_PASSWORD"
            );
            anyhow::bail!(
                "ACLED OAuth2 token request returned HTTP {status}. \
                 Verify ACLED_EMAIL and ACLED_PASSWORD are correct myACLED credentials. \
                 Response: {body}"
            );
        }

        let token_resp: TokenResponse = resp.json().await?;
        self.cache_token(&token_resp)
    }

    /// Request a new access token using a refresh token.
    async fn request_token_with_refresh(
        &self,
        http: &reqwest::Client,
        refresh_token: &str,
    ) -> anyhow::Result<String> {
        let resp = http
            .post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
                ("client_id", "acled"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("ACLED refresh token request returned HTTP {}", resp.status());
        }

        let token_resp: TokenResponse = resp.json().await?;
        self.cache_token(&token_resp)
    }

    /// Store a token response in the cache and return the access token.
    fn cache_token(&self, token_resp: &TokenResponse) -> anyhow::Result<String> {
        let cached = CachedToken {
            access_token: token_resp.access_token.clone(),
            refresh_token: token_resp.refresh_token.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(token_resp.expires_in as i64),
        };

        let access_token = cached.access_token.clone();
        *self.cached_token.lock().unwrap_or_else(|e| e.into_inner()) = Some(cached);

        info!(
            expires_in = token_resp.expires_in,
            "Obtained new ACLED OAuth2 token"
        );

        Ok(access_token)
    }
}

#[async_trait]
impl DataSource for AcledSource {
    fn id(&self) -> &str {
        "acled"
    }

    fn name(&self) -> &str {
        "ACLED Conflict Data"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(6 * 60 * 60) // 6 hours
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        // Check that credentials are configured
        if std::env::var("ACLED_EMAIL").map_or(true, |v| v.is_empty()) {
            warn!("ACLED_EMAIL not set; skipping ACLED poll");
            return Ok(Vec::new());
        }
        if std::env::var("ACLED_PASSWORD").map_or(true, |v| v.is_empty()) {
            warn!("ACLED_PASSWORD not set; skipping ACLED poll");
            return Ok(Vec::new());
        }

        let token = self.get_bearer_token(&ctx.http).await?;

        let since_date = (Utc::now() - chrono::Duration::days(LOOKBACK_DAYS))
            .format("%Y-%m-%d")
            .to_string();

        let mut all_events: Vec<InsertableEvent> = Vec::new();
        let mut page: u32 = 1;

        loop {
            debug!(page, "Fetching ACLED page");

            // Use reqwest's .query() to properly URL-encode all parameters.
            // The `>` in event_date_where and `|` in country names must be
            // percent-encoded for the ACLED API to accept the request.
            //
            // Accept + Content-Type headers are required by the ACLED Drupal
            // backend for proper content negotiation alongside _format=json.
            let resp = ctx.http
                .get(API_BASE)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .query(&[
                    ("_format", "json"),
                    ("event_date", &since_date),
                    ("event_date_where", ">"),
                    ("country", COUNTRIES_FILTER),
                    ("limit", &PAGE_LIMIT.to_string()),
                    ("page", &page.to_string()),
                ])
                .send()
                .await?;

            let status = resp.status();
            if status == reqwest::StatusCode::FORBIDDEN {
                let body = resp.text().await.unwrap_or_default();

                // ACLED's Drupal backend returns different 403 messages:
                //   - "Access denied" → valid token but account lacks API access
                //     (requires myACLED Research tier or above)
                //   - "The resource owner or authorization server denied the
                //     request." → token itself is invalid/expired
                //   - "Consent must be accepted" → profile incomplete
                let is_token_issue = body.contains("resource owner")
                    || body.contains("authorization server");
                let is_access_tier = body.contains("Access denied");
                let is_consent = body.contains("Consent must be accepted")
                    || body.contains("required fields");

                if is_access_tier {
                    error!(
                        body = %body,
                        "ACLED API returned 403 — account lacks API access. \
                         myACLED requires Research tier or above for API access. \
                         Open tier (personal email) only gets aggregated data. \
                         Contact access@acleddata.com to verify your access level."
                    );
                    anyhow::bail!(
                        "ACLED API 403: account does not have API access. \
                         myACLED Research tier or above is required. \
                         Register with an institutional email or contact \
                         access@acleddata.com. Response: {body}"
                    );
                } else if is_consent {
                    error!(
                        body = %body,
                        "ACLED API returned 403 — profile incomplete. \
                         Log in at acleddata.com and accept terms/fill required fields."
                    );
                    anyhow::bail!(
                        "ACLED API 403: profile incomplete — log in at \
                         acleddata.com and accept terms of use. Response: {body}"
                    );
                } else {
                    // Token issue — invalidate cache so next poll re-authenticates
                    error!(
                        body = %body,
                        "ACLED API returned 403 — token rejected"
                    );
                    *self.cached_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    if is_token_issue {
                        anyhow::bail!(
                            "ACLED API 403: bearer token was rejected. \
                             Will re-authenticate on next poll. Response: {body}"
                        );
                    } else {
                        anyhow::bail!(
                            "ACLED API returned HTTP 403. Response: {body}"
                        );
                    }
                }
            }

            let resp = crate::rate_limit::check_rate_limit(resp, "acled")?;

            let body: AcledResponse = resp.json().await?;

            if body.success == Some(false) || body.data.is_empty() {
                break;
            }

            let page_count = body.data.len();

            for raw in body.data {
                let country = raw
                    .get("country")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let region = region_for_country_name(country).unwrap_or("other");

                // Inject region into a mutable copy of the data object.
                let mut data = raw.clone();
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("region".to_string(), serde_json::Value::String(region.to_string()));
                }

                // Extract fields for InsertableEvent
                let lat = data.get("latitude").and_then(json_as_f64);
                let lon = data.get("longitude").and_then(json_as_f64);
                let fatalities = data.get("fatalities").and_then(json_as_f64).unwrap_or(0.0);
                let severity = if fatalities > 10.0 {
                    Severity::Critical
                } else if fatalities > 0.0 {
                    Severity::High
                } else {
                    Severity::Medium
                };
                let entity_id = data.get("event_id_cnty").and_then(|v| v.as_str()).map(|s| s.to_string());
                let title = data.get("notes").and_then(|v| v.as_str()).map(|s| s.to_string());
                // Store raw ACLED classification in payload, use normalized event_type
                let _acled_type = data.get("event_type")
                    .or_else(|| data.get("disorder_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let event_date = data
                    .get("event_date")
                    .and_then(|v| v.as_str())
                    .and_then(|s| {
                        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                            .ok()
                            .and_then(|d| {
                                d.and_hms_opt(0, 0, 0)
                                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                            })
                    })
                    .unwrap_or_else(Utc::now);

                let mut tags = Vec::new();
                if let Some(actor1) = raw.get("actor1").and_then(|v| v.as_str())
                    && !actor1.is_empty()
                {
                    tags.push(format!("actor:{}", actor1));
                }
                if let Some(actor2) = raw.get("actor2").and_then(|v| v.as_str())
                    && !actor2.is_empty()
                {
                    tags.push(format!("actor:{}", actor2));
                }
                // Also extract sub_event_type for more granular classification
                if let Some(sub_type) = raw.get("sub_event_type").and_then(|v| v.as_str())
                    && !sub_type.is_empty()
                {
                    tags.push(format!("sub_type:{}", sub_type));
                }

                all_events.push(InsertableEvent {
                    event_time: event_date,
                    source_type: SourceType::Acled,
                    source_id: Some(self.id().to_string()),
                    longitude: lon,
                    latitude: lat,
                    region_code: Some(region.to_string()),
                    entity_id,
                    entity_name: None,
                    event_type: EventType::ConflictEvent,
                    severity,
                    confidence: None,
                    tags,
                    title,
                    description: None,
                    payload: data,
                    heading: None,
                    speed: None,
                    altitude: None,
                });
            }

            // If we received fewer than the page limit, there are no more pages.
            if (page_count as u32) < PAGE_LIMIT {
                break;
            }

            page += 1;
        }

        debug!(count = all_events.len(), "ACLED poll complete");
        Ok(all_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Token caching ───────────────────────────────────────────────────

    #[test]
    fn cached_token_returned_when_not_expired() {
        let source = AcledSource::new();

        // Insert a token that expires 1 hour from now (well beyond the 5-min margin).
        {
            let mut cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some(CachedToken {
                access_token: "tok_cached_abc".to_string(),
                refresh_token: Some("refresh_xyz".to_string()),
                expires_at: Utc::now() + chrono::Duration::hours(1),
            });
        }

        // Manually replicate the cache-check logic from get_bearer_token (the
        // early-return path) so we can verify it without needing an HTTP client.
        let result = {
            let cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref tok) = *cache {
                if tok.expires_at > Utc::now() + chrono::Duration::minutes(5) {
                    Some(tok.access_token.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        assert_eq!(result, Some("tok_cached_abc".to_string()));
    }

    #[test]
    fn expired_token_not_returned_from_cache() {
        let source = AcledSource::new();

        // Insert a token that expired 10 minutes ago.
        {
            let mut cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some(CachedToken {
                access_token: "tok_old".to_string(),
                refresh_token: None,
                expires_at: Utc::now() - chrono::Duration::minutes(10),
            });
        }

        let result = {
            let cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref tok) = *cache {
                if tok.expires_at > Utc::now() + chrono::Duration::minutes(5) {
                    Some(tok.access_token.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        assert!(result.is_none(), "Expired token should not be returned");
    }

    #[test]
    fn token_within_safety_margin_not_returned() {
        let source = AcledSource::new();

        // Token expires in 3 minutes — within the 5-minute safety margin.
        {
            let mut cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some(CachedToken {
                access_token: "tok_about_to_expire".to_string(),
                refresh_token: None,
                expires_at: Utc::now() + chrono::Duration::minutes(3),
            });
        }

        let result = {
            let cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref tok) = *cache {
                if tok.expires_at > Utc::now() + chrono::Duration::minutes(5) {
                    Some(tok.access_token.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        assert!(
            result.is_none(),
            "Token within 5-minute safety margin should not be returned"
        );
    }

    #[test]
    fn cache_token_stores_and_returns_access_token() {
        let source = AcledSource::new();

        let token_resp = TokenResponse {
            access_token: "new_tok_123".to_string(),
            token_type: "bearer".to_string(),
            expires_in: 3600,
            refresh_token: Some("ref_tok_456".to_string()),
        };

        let result = source.cache_token(&token_resp).unwrap();
        assert_eq!(result, "new_tok_123");

        // Verify the cached value.
        let cache = source.cached_token.lock().unwrap_or_else(|e| e.into_inner());
        let cached = cache.as_ref().expect("token should be cached");
        assert_eq!(cached.access_token, "new_tok_123");
        assert_eq!(cached.refresh_token.as_deref(), Some("ref_tok_456"));
        // expires_at should be roughly 1 hour from now.
        let diff = cached.expires_at - Utc::now();
        assert!(
            diff.num_seconds() > 3500 && diff.num_seconds() <= 3600,
            "expires_at should be ~3600s from now, got {}s",
            diff.num_seconds()
        );
    }

    // ── TokenResponse deserialization ────────────────────────────────────

    #[test]
    fn token_response_deserializes_with_refresh_token() {
        let json_str = r#"{
            "access_token": "eyJhbGciOiJSUzI1NiJ9.test",
            "token_type": "Bearer",
            "expires_in": 1800,
            "refresh_token": "dGhpcyBpcyBhIHJlZnJlc2ggdG9rZW4"
        }"#;

        let resp: TokenResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.access_token, "eyJhbGciOiJSUzI1NiJ9.test");
        assert_eq!(resp.token_type, "Bearer");
        assert_eq!(resp.expires_in, 1800);
        assert_eq!(
            resp.refresh_token.as_deref(),
            Some("dGhpcyBpcyBhIHJlZnJlc2ggdG9rZW4")
        );
    }

    #[test]
    fn token_response_deserializes_without_refresh_token() {
        let json_str = r#"{
            "access_token": "short_tok",
            "token_type": "Bearer",
            "expires_in": 900
        }"#;

        let resp: TokenResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.access_token, "short_tok");
        assert_eq!(resp.expires_in, 900);
        assert!(
            resp.refresh_token.is_none(),
            "refresh_token should default to None when absent"
        );
    }

    // ── AcledResponse deserialization ────────────────────────────────────

    #[test]
    fn acled_response_success_with_data() {
        let json_val = json!({
            "status": 200,
            "success": true,
            "count": 2,
            "data": [
                {"event_id_cnty": "UKR0001", "country": "Ukraine"},
                {"event_id_cnty": "UKR0002", "country": "Ukraine"}
            ]
        });

        let resp: AcledResponse = serde_json::from_value(json_val).unwrap();
        assert_eq!(resp.status, Some(200));
        assert_eq!(resp.success, Some(true));
        assert_eq!(resp.count, Some(2));
        assert_eq!(resp.data.len(), 2);
    }

    #[test]
    fn acled_response_empty_data() {
        let json_val = json!({
            "status": 200,
            "success": true,
            "count": 0,
            "data": []
        });

        let resp: AcledResponse = serde_json::from_value(json_val).unwrap();
        assert!(resp.data.is_empty());
        assert_eq!(resp.count, Some(0));
    }

    #[test]
    fn acled_response_missing_optional_fields() {
        // The API might return a minimal envelope.
        let json_val = json!({
            "data": [{"event_id_cnty": "SYR0001"}]
        });

        let resp: AcledResponse = serde_json::from_value(json_val).unwrap();
        assert!(resp.status.is_none());
        assert!(resp.success.is_none());
        assert!(resp.count.is_none());
        assert_eq!(resp.data.len(), 1);
    }

    #[test]
    fn acled_response_failure() {
        let json_val = json!({
            "status": 403,
            "success": false,
            "data": []
        });

        let resp: AcledResponse = serde_json::from_value(json_val).unwrap();
        assert_eq!(resp.success, Some(false));
        assert!(resp.data.is_empty());
    }

    // ── Event record → InsertableEvent transformation ───────────────────

    /// Build an InsertableEvent from a raw ACLED JSON record, replicating the
    /// transformation logic in `poll()` so we can verify it in isolation.
    fn parse_acled_record(raw: serde_json::Value) -> InsertableEvent {
        let country = raw
            .get("country")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let region = region_for_country_name(country).unwrap_or("other");

        let mut data = raw.clone();
        if let Some(obj) = data.as_object_mut() {
            obj.insert(
                "region".to_string(),
                serde_json::Value::String(region.to_string()),
            );
        }

        let lat = data.get("latitude").and_then(json_as_f64);
        let lon = data.get("longitude").and_then(json_as_f64);
        let fatalities = data
            .get("fatalities")
            .and_then(json_as_f64)
            .unwrap_or(0.0);
        let severity = if fatalities > 10.0 {
            Severity::Critical
        } else if fatalities > 0.0 {
            Severity::High
        } else {
            Severity::Medium
        };
        let entity_id = data
            .get("event_id_cnty")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let title = data
            .get("notes")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let event_date = data
            .get("event_date")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| {
                        d.and_hms_opt(0, 0, 0)
                            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                    })
            })
            .unwrap_or_else(Utc::now);

        let mut tags = Vec::new();
        if let Some(actor1) = raw.get("actor1").and_then(|v| v.as_str())
            && !actor1.is_empty()
        {
            tags.push(format!("actor:{}", actor1));
        }
        if let Some(actor2) = raw.get("actor2").and_then(|v| v.as_str())
            && !actor2.is_empty()
        {
            tags.push(format!("actor:{}", actor2));
        }
        if let Some(sub_type) = raw.get("sub_event_type").and_then(|v| v.as_str())
            && !sub_type.is_empty()
        {
            tags.push(format!("sub_type:{}", sub_type));
        }

        InsertableEvent {
            event_time: event_date,
            source_type: SourceType::Acled,
            source_id: Some("acled".to_string()),
            longitude: lon,
            latitude: lat,
            region_code: Some(region.to_string()),
            entity_id,
            entity_name: None,
            event_type: EventType::ConflictEvent,
            severity,
            confidence: None,
            tags,
            title,
            description: None,
            payload: data,
            heading: None,
            speed: None,
            altitude: None,
        }
    }

    #[test]
    fn parse_full_acled_event() {
        let raw = json!({
            "event_id_cnty": "UKR12345",
            "event_date": "2026-02-28",
            "event_type": "Battles",
            "sub_event_type": "Armed clash",
            "actor1": "Military Forces of Ukraine",
            "actor2": "Military Forces of Russia",
            "country": "Ukraine",
            "latitude": "48.5734",
            "longitude": "37.9904",
            "fatalities": "3",
            "notes": "Clashes reported near Bakhmut between Ukrainian and Russian forces."
        });

        let event = parse_acled_record(raw);

        assert_eq!(event.source_type, SourceType::Acled);
        assert_eq!(event.event_type, EventType::ConflictEvent);
        assert_eq!(event.entity_id.as_deref(), Some("UKR12345"));
        assert_eq!(event.region_code.as_deref(), Some("eastern-europe"));
        assert_eq!(event.severity, Severity::High); // fatalities=3, > 0 but <= 10
        assert_eq!(
            event.title.as_deref(),
            Some("Clashes reported near Bakhmut between Ukrainian and Russian forces.")
        );

        // Coordinates parsed from strings via json_as_f64.
        let lat = event.latitude.expect("latitude should be present");
        let lon = event.longitude.expect("longitude should be present");
        assert!((lat - 48.5734).abs() < 0.001);
        assert!((lon - 37.9904).abs() < 0.001);

        // Tags should contain both actors and the sub_event_type.
        assert!(event.tags.contains(&"actor:Military Forces of Ukraine".to_string()));
        assert!(event.tags.contains(&"actor:Military Forces of Russia".to_string()));
        assert!(event.tags.contains(&"sub_type:Armed clash".to_string()));

        // event_time should be 2026-02-28 00:00:00 UTC.
        assert_eq!(
            event.event_time,
            DateTime::<Utc>::from_naive_utc_and_offset(
                chrono::NaiveDate::from_ymd_opt(2026, 2, 28)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                Utc,
            )
        );

        // Payload should have injected region field.
        assert_eq!(
            event.payload.get("region").and_then(|v| v.as_str()),
            Some("eastern-europe")
        );
    }

    #[test]
    fn severity_critical_when_fatalities_above_ten() {
        let raw = json!({
            "event_id_cnty": "YEM0099",
            "event_date": "2026-01-15",
            "country": "Yemen",
            "latitude": "15.3694",
            "longitude": "44.1910",
            "fatalities": "25",
            "notes": "Airstrike on Houthi positions"
        });

        let event = parse_acled_record(raw);
        assert_eq!(event.severity, Severity::Critical);
        assert_eq!(event.region_code.as_deref(), Some("middle-east"));
    }

    #[test]
    fn severity_medium_when_zero_fatalities() {
        let raw = json!({
            "event_id_cnty": "SYR0050",
            "event_date": "2026-03-01",
            "country": "Syria",
            "latitude": "33.5138",
            "longitude": "36.2765",
            "fatalities": "0",
            "notes": "Protest in Damascus"
        });

        let event = parse_acled_record(raw);
        assert_eq!(event.severity, Severity::Medium);
    }

    #[test]
    fn unknown_country_maps_to_other_region() {
        let raw = json!({
            "event_id_cnty": "COL0001",
            "event_date": "2026-02-10",
            "country": "Colombia",
            "latitude": "4.711",
            "longitude": "-74.0721",
            "fatalities": "1"
        });

        let event = parse_acled_record(raw);
        assert_eq!(event.region_code.as_deref(), Some("other"));
    }

    #[test]
    fn missing_actors_produce_no_actor_tags() {
        let raw = json!({
            "event_id_cnty": "SDN0010",
            "event_date": "2026-01-20",
            "country": "Sudan",
            "latitude": "15.5007",
            "longitude": "32.5599",
            "fatalities": "0",
            "notes": "Demonstration in Khartoum"
        });

        let event = parse_acled_record(raw);
        assert!(
            !event.tags.iter().any(|t| t.starts_with("actor:")),
            "No actor tags when actor fields are absent"
        );
        assert_eq!(event.region_code.as_deref(), Some("africa"));
    }

    #[test]
    fn empty_actor_strings_are_not_tagged() {
        let raw = json!({
            "event_id_cnty": "IRN0001",
            "event_date": "2026-02-01",
            "country": "Iran",
            "latitude": "35.6892",
            "longitude": "51.3890",
            "fatalities": "0",
            "actor1": "",
            "actor2": "",
            "sub_event_type": ""
        });

        let event = parse_acled_record(raw);
        assert!(
            event.tags.is_empty(),
            "Empty strings should not produce tags"
        );
    }

    #[test]
    fn fatalities_as_number_instead_of_string() {
        // json_as_f64 should handle both numeric and string representations.
        let raw = json!({
            "event_id_cnty": "LBN0001",
            "event_date": "2026-02-15",
            "country": "Lebanon",
            "latitude": 33.8938,
            "longitude": 35.5018,
            "fatalities": 7
        });

        let event = parse_acled_record(raw);
        assert_eq!(event.severity, Severity::High);
        // Numeric lat/lon should also work.
        assert!((event.latitude.unwrap() - 33.8938).abs() < 0.001);
        assert!((event.longitude.unwrap() - 35.5018).abs() < 0.001);
    }
}
