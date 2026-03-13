use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Query / body parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub query: String,
    pub page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub ips: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn shodan_api_key() -> Result<String, ApiError> {
    std::env::var("SHODAN_API_KEY")
        .map_err(|_| anyhow::anyhow!("SHODAN_API_KEY environment variable not set").into())
}

fn shodan_http_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("SituationReport/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client")
    })
}

// ---------------------------------------------------------------------------
// GET /api/shodan/search?query={query}&page={page}
// ---------------------------------------------------------------------------

pub async fn search_shodan(
    State(_state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let api_key = shodan_api_key()?;
    let page = params.page.unwrap_or(1);

    let url = format!(
        "https://api.shodan.io/shodan/host/search?key={key}&query={query}&page={page}",
        key = api_key,
        query = urlencoding::encode(&params.query),
        page = page,
    );

    let client = shodan_http_client();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Shodan search request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("Shodan API error: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Shodan response: {e}"))?;

    Ok(Json(body))
}

// ---------------------------------------------------------------------------
// GET /api/shodan/host/{ip}
// ---------------------------------------------------------------------------

pub async fn host_lookup(
    State(_state): State<AppState>,
    Path(ip): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let api_key = shodan_api_key()?;

    let url = format!(
        "https://api.shodan.io/shodan/host/{ip}?key={key}",
        ip = ip,
        key = api_key,
    );

    let client = shodan_http_client();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Shodan host lookup failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("Shodan API error: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Shodan response: {e}"))?;

    Ok(Json(body))
}

// ---------------------------------------------------------------------------
// GET /api/shodan/alerts -- list all Shodan network alerts
// ---------------------------------------------------------------------------

pub async fn list_alerts(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let api_key = shodan_api_key()?;
    let client = shodan_http_client();

    let alerts = sr_sources::shodan::list_alerts_api(client, &api_key)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list Shodan alerts: {e}"))?;

    let json = serde_json::to_value(&alerts)
        .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))?;

    Ok(Json(json))
}

// ---------------------------------------------------------------------------
// GET /api/shodan/api-info -- get plan status and credit usage
// ---------------------------------------------------------------------------

pub async fn api_info(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let api_key = shodan_api_key()?;
    let client = shodan_http_client();

    let info = sr_sources::shodan::get_api_info(client, &api_key)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get Shodan API info: {e}"))?;

    let json = serde_json::to_value(&info)
        .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))?;

    Ok(Json(json))
}

// ---------------------------------------------------------------------------
// POST /api/shodan/scan -- submit IPs for on-demand scan
// ---------------------------------------------------------------------------

pub async fn submit_scan(
    State(_state): State<AppState>,
    Json(body): Json<ScanRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let api_key = shodan_api_key()?;
    let client = shodan_http_client();

    if body.ips.is_empty() {
        return Err(anyhow::anyhow!("No IPs provided for scan").into());
    }

    if body.ips.len() > 1000 {
        return Err(
            anyhow::anyhow!("Too many IPs (max 1000 per request, got {})", body.ips.len()).into(),
        );
    }

    let result = sr_sources::shodan::submit_scan_api(client, &api_key, &body.ips)
        .await
        .map_err(|e| anyhow::anyhow!("Scan submission failed: {e}"))?;

    let json = serde_json::to_value(&result)
        .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))?;

    Ok(Json(json))
}

// ---------------------------------------------------------------------------
// POST /api/shodan/discover -- trigger manual discovery run
// ---------------------------------------------------------------------------

pub async fn trigger_discovery(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // This returns immediately with an acknowledgment. The actual discovery
    // runs on its normal polling cycle (ShodanDiscovery source). To trigger
    // it immediately, we inform the user that the next discovery run will
    // use the default budget.
    //
    // For a true manual trigger, the frontend should call the individual
    // search/alert endpoints directly. This route provides a status check.

    let api_key = shodan_api_key()?;
    let client = shodan_http_client();

    let info = sr_sources::shodan::get_api_info(client, &api_key)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get API info: {e}"))?;

    let alerts = sr_sources::shodan::list_alerts_api(client, &api_key)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list alerts: {e}"))?;

    let total_monitored: u64 = alerts.iter().map(|a| a.size).sum();

    Ok(Json(serde_json::json!({
        "status": "acknowledged",
        "message": "Discovery runs automatically every 24 hours. Use individual API endpoints for immediate actions.",
        "current_state": {
            "query_credits_remaining": info.query_credits,
            "scan_credits_remaining": info.scan_credits,
            "monitored_ips_used": total_monitored,
            "monitored_ips_limit": info.usage_limits.monitored_ips,
            "active_alerts": alerts.len(),
        }
    })))
}

// ---------------------------------------------------------------------------
// Inline URL-encoding helper
// ---------------------------------------------------------------------------

mod urlencoding {
    use std::fmt::Write;

    pub fn encode(input: &str) -> String {
        let mut out = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'~' => out.push(byte as char),
                _ => {
                    let _ = write!(out, "%{:02X}", byte);
                }
            }
        }
        out
    }
}
