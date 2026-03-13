use std::collections::{HashMap, HashSet};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use sr_types::{EventType, Severity, SourceType};

use crate::common::{region_for_country, urlencode};
use crate::{DataSource, InsertableEvent, SourceContext};

// ---------------------------------------------------------------------------
// ICS-relevant ports
// ---------------------------------------------------------------------------

const ICS_PORTS: &[u16] = &[
    102,   // Siemens S7comm
    502,   // Modbus
    789,   // Red Lion Crimson
    1089,  // FF HSE
    1091,  // FF HSE
    1911,  // Fox / Niagara
    2222,  // EtherNet/IP implicit
    2404,  // IEC 60870-5-104
    4840,  // OPC UA
    4911,  // Niagara Fox SSL
    9600,  // OMRON FINS
    18245, // GE SRTP
    20000, // DNP3
    34962, // PROFINET
    34964, // PROFINET
    44818, // EtherNet/IP explicit
    47808, // BACnet
    55553, // Honeywell CEE
    55555, // Crestron CTP
];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn api_key() -> anyhow::Result<String> {
    std::env::var("SHODAN_API_KEY")
        .map_err(|_| anyhow::anyhow!("SHODAN_API_KEY not set"))
}

fn is_ics_port(port: u16) -> bool {
    ICS_PORTS.contains(&port)
}

// ===========================================================================
// Strong types (from SHODAN_RESEARCH.md)
// ===========================================================================

/// Core banner object from Shodan — both search results and stream emit this.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Banner {
    pub ip_str: Option<String>,
    pub ip: Option<u64>,
    pub port: u16,
    pub transport: Option<String>,
    pub timestamp: Option<String>,
    pub hostnames: Option<Vec<String>>,
    pub domains: Option<Vec<String>>,
    pub org: Option<String>,
    pub asn: Option<String>,
    pub isp: Option<String>,
    pub os: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub data: Option<String>,
    pub tags: Option<Vec<String>>,
    pub vulns: Option<HashMap<String, serde_json::Value>>,
    pub location: Option<Location>,
    pub http: Option<HttpInfo>,
    pub ssl: Option<SslInfo>,
    #[serde(rename = "_shodan")]
    pub shodan_meta: Option<ShodanMeta>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Location {
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub city: Option<String>,
    pub region_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HttpInfo {
    pub title: Option<String>,
    pub server: Option<String>,
    pub status: Option<u16>,
    pub host: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SslInfo {
    pub cert: Option<serde_json::Value>,
    pub cipher: Option<serde_json::Value>,
    pub versions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShodanMeta {
    pub id: Option<String>,
    pub module: Option<String>,
    pub crawler: Option<String>,
}

/// Search API response wrapper.
#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub matches: Vec<Banner>,
    pub total: u64,
}

/// Alert object returned by alert CRUD endpoints.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Alert {
    pub id: String,
    pub name: String,
    pub created: Option<String>,
    pub size: u64,
    pub filters: AlertFilters,
    pub triggers: HashMap<String, serde_json::Value>,
    pub has_triggers: bool,
    pub expires: Option<u64>,
    pub expiration: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlertFilters {
    pub ip: Vec<String>,
}

/// InternetDB response -- free, no auth, no credits.
#[derive(Debug, Deserialize, Serialize)]
pub struct InternetDbEntry {
    pub ip: String,
    pub ports: Vec<u16>,
    pub cpes: Vec<String>,
    pub hostnames: Vec<String>,
    pub tags: Vec<String>,
    pub vulns: Vec<String>,
}

/// API info / plan status.
#[derive(Debug, Deserialize, Serialize)]
pub struct ApiInfo {
    pub plan: String,
    pub query_credits: u64,
    pub scan_credits: u64,
    pub monitored_ips: u64,
    pub usage_limits: UsageLimits,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UsageLimits {
    pub query_credits: i64,
    pub scan_credits: i64,
    pub monitored_ips: i64,
}

/// Scan submission response.
#[derive(Debug, Deserialize, Serialize)]
pub struct ScanResult {
    pub id: String,
    pub count: u64,
    pub credits_left: u64,
}

// ===========================================================================
// Discovery queries
// ===========================================================================

struct DiscoveryQuery {
    query: &'static str,
    category: &'static str,
    priority: u8,
    max_pages: u16,
}

const DISCOVERY_QUERIES: &[DiscoveryQuery] = &[
    // -- IRAN - Critical Infrastructure (Priority 1) --
    // NOTE: `tag:ics` is a Corporate-only filter. We use port-based queries instead
    // which work on all plans (Membership, Edu, Freelancer, Small Business, etc.)
    // Each port targets a specific ICS/SCADA protocol.
    DiscoveryQuery { query: "port:502 country:IR",                    category: "iran_modbus",    priority: 1, max_pages: 20 },
    DiscoveryQuery { query: "port:102 country:IR",                    category: "iran_s7",        priority: 1, max_pages: 20 },
    DiscoveryQuery { query: "port:20000 country:IR",                  category: "iran_dnp3",      priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:47808 country:IR",                  category: "iran_bacnet",    priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:44818 country:IR",                  category: "iran_etherip",   priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:2404 country:IR",                   category: "iran_iec104",    priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:4840 country:IR",                   category: "iran_opcua",     priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:1911 country:IR",                   category: "iran_fox",       priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:9600 country:IR",                   category: "iran_fins",      priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:34962 country:IR",                  category: "iran_profinet",  priority: 2, max_pages: 5 },
    DiscoveryQuery { query: "port:1883 country:IR",                   category: "iran_mqtt",      priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:161 country:IR",                    category: "iran_snmp",      priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "\"Schneider Electric\" country:IR",      category: "iran_schneider", priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "\"Siemens\" port:102 country:IR",        category: "iran_siemens",   priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "\"EnergyICT\" country:IR",               category: "iran_energy",    priority: 2, max_pages: 5 },
    // -- IRAN - Network Infrastructure --
    DiscoveryQuery { query: "port:179 country:IR",                    category: "iran_bgp",       priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:53 country:IR",                     category: "iran_dns",       priority: 3, max_pages: 10 },
    // -- GULF STATES - ICS (port-based, edu-compatible) --
    DiscoveryQuery { query: "port:502 country:AE",                    category: "uae_modbus",     priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:102 country:AE",                    category: "uae_s7",         priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:47808 country:AE",                  category: "uae_bacnet",     priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:502 country:SA",                    category: "saudi_modbus",   priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:102 country:SA",                    category: "saudi_s7",       priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:47808 country:SA",                  category: "saudi_bacnet",   priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:502 country:BH",                    category: "bahrain_modbus", priority: 2, max_pages: 5 },
    DiscoveryQuery { query: "port:502 country:QA",                    category: "qatar_modbus",   priority: 2, max_pages: 5 },
    DiscoveryQuery { query: "port:502 country:KW",                    category: "kuwait_modbus",  priority: 2, max_pages: 5 },
    // -- ISRAEL - Retaliatory Strike Detection --
    DiscoveryQuery { query: "port:502 country:IL",                    category: "israel_modbus",  priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:102 country:IL",                    category: "israel_s7",      priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:47808 country:IL",                  category: "israel_bacnet",  priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:44818 country:IL",                  category: "israel_etherip", priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:2404 country:IL",                   category: "israel_iec104",  priority: 2, max_pages: 5 },
    // -- IRAQ/SYRIA/LEBANON - Transit / Spillover --
    DiscoveryQuery { query: "port:502 country:IQ",                    category: "iraq_modbus",    priority: 3, max_pages: 5 },
    DiscoveryQuery { query: "port:102 country:IQ",                    category: "iraq_s7",        priority: 3, max_pages: 5 },
    DiscoveryQuery { query: "port:502 country:SY",                    category: "syria_modbus",   priority: 3, max_pages: 3 },
    DiscoveryQuery { query: "port:502 country:LB",                    category: "lebanon_modbus", priority: 3, max_pages: 3 },
    // -- MARITIME - Hormuz/Port ICS --
    DiscoveryQuery { query: "port:502 org:\"port\"",                  category: "maritime_modbus", priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "\"NMEA\" country:IR",                    category: "iran_maritime",   priority: 2, max_pages: 5 },
    DiscoveryQuery { query: "\"Kongsberg\" country:IR",               category: "iran_kongsberg",  priority: 3, max_pages: 5 },
];

// ===========================================================================
// Alert allocation strategy
// ===========================================================================

struct AlertAllocation {
    name: &'static str,
    max_ips: u64,
    categories: &'static [&'static str],
}

const ALERT_ALLOCATIONS: &[AlertAllocation] = &[
    AlertAllocation {
        name: "Iran ICS",
        max_ips: 10_000,
        categories: &[
            "iran_modbus", "iran_s7", "iran_dnp3",
            "iran_bacnet", "iran_etherip", "iran_iec104",
            "iran_opcua", "iran_fox", "iran_fins", "iran_profinet",
            "iran_schneider", "iran_siemens",
        ],
    },
    AlertAllocation {
        name: "Iran Network Infrastructure",
        max_ips: 15_000,
        categories: &["iran_snmp", "iran_mqtt", "iran_bgp", "iran_dns", "iran_energy"],
    },
    AlertAllocation {
        name: "Gulf States ICS",
        max_ips: 15_000,
        categories: &[
            "uae_modbus", "uae_s7", "uae_bacnet",
            "saudi_modbus", "saudi_s7", "saudi_bacnet",
            "bahrain_modbus", "qatar_modbus", "kuwait_modbus",
        ],
    },
    AlertAllocation {
        name: "Israel ICS",
        max_ips: 10_000,
        categories: &[
            "israel_modbus", "israel_s7", "israel_bacnet",
            "israel_etherip", "israel_iec104",
        ],
    },
    AlertAllocation {
        name: "Iraq Syria Lebanon ICS",
        max_ips: 5_000,
        categories: &[
            "iraq_modbus", "iraq_s7",
            "syria_modbus", "lebanon_modbus",
        ],
    },
    AlertAllocation {
        name: "Maritime Hormuz",
        max_ips: 5_000,
        categories: &["maritime_modbus", "iran_maritime", "iran_kongsberg"],
    },
];

/// Recommended triggers for ICS monitoring alerts.
const RECOMMENDED_TRIGGERS: &str =
    "new_service,industrial_control_system,malware,open_database,iot,vulnerable,ssl_expired,internet_scanner,uncommon";

/// Default credit budget per automated discovery run (conservative).
const DEFAULT_DISCOVERY_BUDGET: u64 = 1000;

/// Default max pages per query during automated discovery (conservative).
/// 3 pages = 300 results per query, ~80 credits total for all 27 queries.
const DEFAULT_MAX_PAGES_PER_QUERY: u16 = 3;

// ===========================================================================
// Alert CRUD helpers
// ===========================================================================

async fn create_alert(
    http: &reqwest::Client,
    key: &str,
    name: &str,
    ips: &[String],
) -> anyhow::Result<Alert> {
    let url = format!("https://api.shodan.io/shodan/alert?key={}", key);
    let body = serde_json::json!({
        "name": name,
        "filters": { "ip": ips },
        "expires": 0
    });
    let resp = crate::rate_limit::check_rate_limit(
        http.post(&url).json(&body).send().await?,
        "shodan",
    )?;
    let alert: Alert = resp.json().await?;
    info!(alert_id = %alert.id, size = alert.size, name = name, "Alert created");
    Ok(alert)
}

async fn edit_alert(
    http: &reqwest::Client,
    key: &str,
    alert_id: &str,
    ips: &[String],
) -> anyhow::Result<Alert> {
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}?key={}",
        alert_id, key
    );
    let body = serde_json::json!({ "filters": { "ip": ips } });
    let resp = crate::rate_limit::check_rate_limit(
        http.post(&url).json(&body).send().await?,
        "shodan",
    )?;
    let alert: Alert = resp.json().await?;
    Ok(alert)
}

#[allow(dead_code)]
async fn delete_alert(
    http: &reqwest::Client,
    key: &str,
    alert_id: &str,
) -> anyhow::Result<()> {
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}?key={}",
        alert_id, key
    );
    crate::rate_limit::check_rate_limit(
        http.delete(&url).send().await?,
        "shodan",
    )?;
    info!(alert_id = alert_id, "Alert deleted");
    Ok(())
}

pub async fn list_alerts_api(
    http: &reqwest::Client,
    key: &str,
) -> anyhow::Result<Vec<Alert>> {
    let url = format!("https://api.shodan.io/shodan/alert/info?key={}", key);
    let resp = crate::rate_limit::check_rate_limit(
        http.get(&url).send().await?,
        "shodan",
    )?;
    let alerts: Vec<Alert> = resp.json().await?;
    Ok(alerts)
}

async fn enable_triggers(
    http: &reqwest::Client,
    key: &str,
    alert_id: &str,
    triggers: &str,
) -> anyhow::Result<()> {
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}/trigger/{}?key={}",
        alert_id, triggers, key
    );
    crate::rate_limit::check_rate_limit(
        http.put(&url).send().await?,
        "shodan",
    )?;
    info!(alert_id = alert_id, triggers = triggers, "Triggers enabled");
    Ok(())
}

pub async fn get_api_info(
    http: &reqwest::Client,
    key: &str,
) -> anyhow::Result<ApiInfo> {
    let url = format!("https://api.shodan.io/api-info?key={}", key);
    let resp = crate::rate_limit::check_rate_limit(
        http.get(&url).send().await?,
        "shodan",
    )?;
    let info: ApiInfo = resp.json().await?;
    Ok(info)
}

pub async fn submit_scan_api(
    http: &reqwest::Client,
    key: &str,
    ips: &[String],
) -> anyhow::Result<ScanResult> {
    let url = format!("https://api.shodan.io/shodan/scan?key={}", key);
    let body = [("ips", ips.join(","))];
    let resp = crate::rate_limit::check_rate_limit(
        http.post(&url).form(&body).send().await?,
        "shodan",
    )?;
    let result: ScanResult = resp.json().await?;
    info!(
        scan_id = %result.id,
        count = result.count,
        credits_remaining = result.credits_left,
        "On-demand scan submitted"
    );
    Ok(result)
}

// ===========================================================================
// ShodanStream -- Alert Stream Consumer (replaces broken country stream)
// ===========================================================================

pub struct ShodanStream;

impl Default for ShodanStream {
    fn default() -> Self {
        Self::new()
    }
}

impl ShodanStream {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataSource for ShodanStream {
    fn id(&self) -> &str {
        "shodan-stream"
    }

    fn name(&self) -> &str {
        "Shodan Alert Stream"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(0) // streaming, not polled
    }

    fn is_streaming(&self) -> bool {
        true
    }

    async fn poll(&self, _ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        Ok(vec![])
    }

    async fn start_stream(
        &self,
        _ctx: &SourceContext,
        tx: broadcast::Sender<InsertableEvent>,
    ) -> anyhow::Result<()> {
        let key = api_key()?;

        // Build a client with NO timeout for the infinite stream.
        let stream_client = reqwest::Client::builder()
            .user_agent("SituationReport/0.1")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build stream client: {e}"))?;

        let mut backoff_secs = 1u64;

        loop {
            let url = format!(
                "https://stream.shodan.io/shodan/alert?key={}",
                key
            );
            info!("Connecting to Shodan alert stream");

            match stream_client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    backoff_secs = 1; // reset on successful connect
                    info!("Connected to Shodan alert stream");

                    let mut stream = resp.bytes_stream();
                    let mut buffer = Vec::new();

                    while let Some(chunk) = stream.next().await {
                        let chunk = match chunk {
                            Ok(c) => c,
                            Err(e) => {
                                error!(error = %e, "Error reading Shodan alert stream chunk");
                                break; // reconnect
                            }
                        };

                        buffer.extend_from_slice(&chunk);

                        // Process complete NDJSON lines.
                        while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = buffer.drain(..=newline_pos).collect();
                            let line = String::from_utf8_lossy(&line);
                            let line = line.trim();

                            if line.is_empty() {
                                // Heartbeat, skip.
                                continue;
                            }

                            let banner: serde_json::Value = match serde_json::from_str(line) {
                                Ok(v) => v,
                                Err(e) => {
                                    warn!(
                                        error = %e,
                                        line_preview = &line[..line.len().min(200)],
                                        "Failed to parse Shodan alert banner JSON"
                                    );
                                    continue;
                                }
                            };

                            let ip = banner
                                .get("ip_str")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();

                            let port = banner
                                .get("port")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as u16;

                            let country_code = banner
                                .pointer("/location/country_code")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            let latitude = banner
                                .pointer("/location/latitude")
                                .and_then(|v| v.as_f64());

                            let longitude = banner
                                .pointer("/location/longitude")
                                .and_then(|v| v.as_f64());

                            let region = region_for_country(country_code);

                            let tags: Vec<String> = banner
                                .get("tags")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|t| t.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();

                            let has_ics = tags.iter().any(|t| t == "ics");
                            let has_vulns = banner.get("vulns").is_some();

                            let severity = if has_vulns {
                                Severity::Critical
                            } else if has_ics || is_ics_port(port) {
                                Severity::High
                            } else {
                                Severity::Info
                            };

                            let org_opt = banner.get("org").and_then(|v| v.as_str());
                            let asn = banner
                                .get("asn")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            debug!(
                                ip = %ip,
                                port = port,
                                country = country_code,
                                severity = severity.as_str(),
                                org = org_opt.unwrap_or(""),
                                asn = asn,
                                ics = has_ics,
                                "Shodan alert banner received"
                            );

                            let entity_id_str = format!("{}:{}", ip, port);
                            let title_str = format!("Shodan banner: {}:{}", ip, port);

                            let event = InsertableEvent {
                                event_time: Utc::now(),
                                source_type: SourceType::Shodan,
                                source_id: Some("shodan-stream".to_string()),
                                longitude,
                                latitude,
                                region_code: region.map(String::from),
                                entity_id: Some(entity_id_str),
                                entity_name: org_opt.map(String::from),
                                event_type: EventType::ShodanBanner,
                                severity,
                                confidence: None,
                                tags,
                                title: Some(title_str),
                                description: None,
                                payload: banner,
                                heading: None,
                                speed: None,
                                altitude: None,
                            };

                            let _ = tx.send(event);
                        }
                    }

                    warn!("Shodan alert stream ended, reconnecting...");
                }
                Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                    // Respect 429 Retry-After header for rate limiting
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(60);
                    warn!(
                        retry_after_secs = retry_after,
                        "Shodan alert stream rate limited (429)"
                    );
                    tokio::time::sleep(Duration::from_secs(retry_after)).await;
                    continue; // skip the normal backoff
                }
                Ok(resp) => {
                    error!(
                        status = %resp.status(),
                        "Shodan alert stream connection rejected"
                    );
                }
                Err(e) => {
                    error!(error = %e, "Shodan alert stream connection error");
                }
            }

            // Exponential backoff: 1s -> 2s -> 4s -> ... -> 60s max
            info!(backoff_secs = backoff_secs, "Reconnecting after backoff");
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(60);
        }
    }
}

// ===========================================================================
// ShodanDiscovery -- Discovery + Monitor Management (daily)
// ===========================================================================

pub struct ShodanDiscovery;

impl Default for ShodanDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl ShodanDiscovery {
    pub fn new() -> Self {
        Self
    }
}

/// Holds IPs discovered during a run, categorized for alert allocation.
struct DiscoveryResults {
    /// category -> set of IPs
    by_category: HashMap<String, HashSet<String>>,
}

impl DiscoveryResults {
    fn new() -> Self {
        Self {
            by_category: HashMap::new(),
        }
    }

    fn insert(&mut self, category: &str, ip: String) {
        self.by_category
            .entry(category.to_string())
            .or_default()
            .insert(ip);
    }

    fn total_unique_ips(&self) -> usize {
        let mut all: HashSet<&String> = HashSet::new();
        for ips in self.by_category.values() {
            all.extend(ips.iter());
        }
        all.len()
    }

    /// Collect IPs for a given set of categories (for alert allocation).
    fn ips_for_categories(&self, categories: &[&str]) -> Vec<String> {
        let mut ips: HashSet<String> = HashSet::new();
        for cat in categories {
            if let Some(cat_ips) = self.by_category.get(*cat) {
                ips.extend(cat_ips.iter().cloned());
            }
        }
        let mut result: Vec<String> = ips.into_iter().collect();
        result.sort();
        result
    }
}

#[async_trait]
impl DataSource for ShodanDiscovery {
    fn id(&self) -> &str {
        "shodan-discovery"
    }

    fn name(&self) -> &str {
        "Shodan Discovery + Monitor Manager"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(86_400) // 24 hours
    }

    fn is_streaming(&self) -> bool {
        false
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let key = api_key()?;
        let mut events = Vec::new();

        // -- Step 1: Check API credits --
        let info = get_api_info(&ctx.http, &key).await?;
        info!(
            plan = %info.plan,
            query_credits = info.query_credits,
            scan_credits = info.scan_credits,
            monitored_ips = info.monitored_ips,
            "Shodan API status"
        );

        if info.query_credits < 10 {
            warn!(
                credits = info.query_credits,
                "Insufficient query credits for discovery, skipping"
            );
            return Ok(events);
        }

        let budget = DEFAULT_DISCOVERY_BUDGET.min(info.query_credits);

        // -- Step 2: Run discovery queries (sorted by priority) --
        let mut sorted_indices: Vec<usize> = (0..DISCOVERY_QUERIES.len()).collect();
        sorted_indices.sort_by_key(|&i| DISCOVERY_QUERIES[i].priority);

        let mut results = DiscoveryResults::new();
        let mut credits_used: u64 = 0;

        for &idx in &sorted_indices {
            let query_def = &DISCOVERY_QUERIES[idx];

            if credits_used >= budget {
                warn!(
                    credits_used = credits_used,
                    budget = budget,
                    "Credit budget exhausted, stopping discovery"
                );
                break;
            }

            info!(
                query = query_def.query,
                category = query_def.category,
                priority = query_def.priority,
                "Running discovery query"
            );

            // Cap pages at the conservative default for automated runs.
            let max_pages = query_def.max_pages.min(DEFAULT_MAX_PAGES_PER_QUERY);
            let mut page: u16 = 1;

            loop {
                if page > max_pages || credits_used >= budget {
                    break;
                }

                let url = format!(
                    "https://api.shodan.io/shodan/host/search?key={}&query={}&page={}&minify=true",
                    key,
                    urlencode(query_def.query),
                    page
                );

                let resp = match ctx.http.get(&url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        error!(error = %e, query = query_def.query, "Discovery request failed");
                        break;
                    }
                };

                // Log non-success status with response body for diagnostics
                if !resp.status().is_success() && resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    error!(query = query_def.query, %status, body = %body.chars().take(500).collect::<String>(), "Shodan discovery API error");
                    break;
                }

                // Propagate 429 rate limits to the registry for proper backoff
                let resp = crate::rate_limit::check_rate_limit(resp, "shodan-discovery")?;

                let search_result: SearchResult = match resp.json().await {
                    Ok(r) => r,
                    Err(e) => {
                        error!(error = %e, "Failed to parse discovery response");
                        break;
                    }
                };

                credits_used += 1;

                for banner in &search_result.matches {
                    if let Some(ref ip) = banner.ip_str {
                        results.insert(query_def.category, ip.clone());

                        // Emit events for ICS-tagged results found during discovery.
                        let tags = banner.tags.as_deref().unwrap_or(&[]);
                        let has_ics = tags.iter().any(|t| t == "ics");
                        let has_vulns = banner.vulns.is_some();

                        if has_ics || has_vulns || is_ics_port(banner.port) {
                            let banner_json = serde_json::to_value(banner).unwrap_or_default();
                            let lat = banner.location.as_ref().and_then(|l| l.latitude);
                            let lon = banner.location.as_ref().and_then(|l| l.longitude);
                            let country_code = banner.location.as_ref()
                                .and_then(|l| l.country_code.as_deref())
                                .unwrap_or("");
                            let region = region_for_country(country_code);
                            let entity_id_str = format!("{}:{}", ip, banner.port);
                            let title_str = format!("Shodan banner: {}:{}", ip, banner.port);
                            let severity = if has_vulns { Severity::Critical } else if has_ics || is_ics_port(banner.port) { Severity::High } else { Severity::Low };
                            let tags_vec: Vec<String> = tags.iter().map(|s| s.to_string()).collect();

                            events.push(InsertableEvent {
                                event_time: Utc::now(),
                                source_type: SourceType::Shodan,
                                source_id: Some("shodan-discovery".to_string()),
                                longitude: lon,
                                latitude: lat,
                                region_code: region.map(String::from),
                                entity_id: Some(entity_id_str),
                                entity_name: banner.org.clone(),
                                event_type: EventType::ShodanBanner,
                                severity,
                                confidence: None,
                                tags: tags_vec,
                                title: Some(title_str),
                                description: None,
                                payload: banner_json,
                                heading: None,
                                speed: None,
                                altitude: None,
                            });
                        }
                    }
                }

                info!(
                    query = query_def.query,
                    total = search_result.total,
                    page_results = search_result.matches.len(),
                    unique_ips = results.total_unique_ips(),
                    credits_used = credits_used,
                    "Discovery page {page} complete"
                );

                // No more results.
                if search_result.matches.is_empty()
                    || (page as u64 * 100) >= search_result.total
                {
                    break;
                }

                page += 1;

                // Rate limit: 1 req/sec for search API.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // Small delay between queries to avoid rate limiting.
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        info!(
            total_unique_ips = results.total_unique_ips(),
            total_credits = credits_used,
            "Discovery phase complete"
        );

        // -- Step 3: Load discovered IPs into Shodan alerts --
        // Get existing alerts to avoid duplicates.
        let existing_alerts = match list_alerts_api(&ctx.http, &key).await {
            Ok(alerts) => alerts,
            Err(e) => {
                error!(error = %e, "Failed to list existing alerts");
                Vec::new()
            }
        };

        for alloc in ALERT_ALLOCATIONS {
            let ips = results.ips_for_categories(alloc.categories);
            if ips.is_empty() {
                continue;
            }

            // Cap at allocation max.
            let ips: Vec<String> = ips.into_iter().take(alloc.max_ips as usize).collect();

            info!(
                alert_name = alloc.name,
                ip_count = ips.len(),
                max_ips = alloc.max_ips,
                "Loading IPs into alert"
            );

            // Find existing alert with matching name.
            let existing = existing_alerts.iter().find(|a| a.name == alloc.name);

            let alert = if let Some(existing_alert) = existing {
                // Update existing alert with new IP list.
                match edit_alert(&ctx.http, &key, &existing_alert.id, &ips).await {
                    Ok(a) => Some(a),
                    Err(e) => {
                        error!(
                            error = %e,
                            alert_name = alloc.name,
                            "Failed to update alert"
                        );
                        None
                    }
                }
            } else {
                // Create new alert.
                match create_alert(&ctx.http, &key, alloc.name, &ips).await {
                    Ok(a) => Some(a),
                    Err(e) => {
                        error!(
                            error = %e,
                            alert_name = alloc.name,
                            "Failed to create alert"
                        );
                        None
                    }
                }
            };

            // Enable recommended triggers on the alert.
            if let Some(ref alert) = alert
                && let Err(e) =
                    enable_triggers(&ctx.http, &key, &alert.id, RECOMMENDED_TRIGGERS).await
                {
                    error!(
                        error = %e,
                        alert_id = %alert.id,
                        "Failed to enable triggers"
                    );
                }

            // Rate limit between alert operations.
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        info!(
            events = events.len(),
            "Shodan discovery + monitor management complete"
        );

        Ok(events)
    }
}

// ===========================================================================
// ShodanSearch -- Periodic ICS count polling (lightweight)
// ===========================================================================

/// Default ICS queries for the Shodan count endpoint.
/// NOTE: `tag:ics` is a Corporate-only ($1099/mo+) restricted filter.
/// We use port-based queries instead which work on all plans including edu.
/// Each port corresponds to a specific ICS/SCADA protocol:
///   502 = Modbus, 102 = S7comm, 47808 = BACnet, 44818 = EtherNet/IP,
///   20000 = DNP3, 2404 = IEC 60870-5-104, 4840 = OPC UA
const DEFAULT_ICS_QUERIES: &[&str] = &[
    // Iran - primary ICS protocols
    "port:502 country:IR",
    "port:102 country:IR",
    "port:47808 country:IR",
    "port:44818 country:IR",
    "port:20000 country:IR",
    "port:2404 country:IR",
    // Israel
    "port:502 country:IL",
    "port:102 country:IL",
    "port:47808 country:IL",
    // Ukraine
    "port:502 country:UA",
    "port:102 country:UA",
    // Saudi Arabia
    "port:502 country:SA",
    "port:47808 country:SA",
    // UAE
    "port:502 country:AE",
    "port:47808 country:AE",
    // Iraq
    "port:502 country:IQ",
];

pub struct ShodanSearch;

impl Default for ShodanSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl ShodanSearch {
    pub fn new() -> Self {
        Self
    }

    /// Extract the country code from a Shodan query string such as
    /// `"tag:ics country:IR"`. Returns `None` if no country facet is found.
    fn country_from_query(query: &str) -> Option<&str> {
        query
            .split_whitespace()
            .find_map(|token| token.strip_prefix("country:"))
    }
}

#[async_trait]
impl DataSource for ShodanSearch {
    fn id(&self) -> &str {
        "shodan-search"
    }

    fn name(&self) -> &str {
        "Shodan ICS Monitor"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(3600) // 1 hour
    }

    fn is_streaming(&self) -> bool {
        false
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        let key = api_key()?;
        let mut events = Vec::new();

        for query in DEFAULT_ICS_QUERIES {
            let url = format!(
                "https://api.shodan.io/shodan/host/count?key={key}&query={query}&facets=country",
                key = key,
                query = urlencode(query),
            );

            let resp = crate::rate_limit::check_rate_limit(
                ctx.http.get(&url).send().await?,
                "shodan-search",
            )?;

            let body: serde_json::Value = resp.json().await?;

            let country_code = Self::country_from_query(query).unwrap_or("");
            let region = region_for_country(country_code);

            info!(
                query = %query,
                total = ?body.get("total"),
                "Shodan ICS count result"
            );

            let payload = serde_json::json!({
                "query": query,
                "total": body.get("total"),
                "facets": body.get("facets"),
                "region": region,
            });

            events.push(InsertableEvent {
                event_time: Utc::now(),
                source_type: SourceType::Shodan,
                source_id: Some("shodan-search".to_string()),
                longitude: None,
                latitude: None,
                region_code: region.map(String::from),
                entity_id: None,
                entity_name: None,
                event_type: EventType::ShodanCount,
                severity: Severity::Info,
                confidence: None,
                tags: vec![],
                title: Some(format!("Shodan ICS count: {}", query)),
                description: None,
                payload,
                heading: None,
                speed: None,
                altitude: None,
            });

            // Rate limit between count queries.
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        Ok(events)
    }
}

// ===========================================================================
// ShodanCameraFinder -- Find open webcams near geographic coordinates
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraResult {
    pub ip: String,
    pub port: u16,
    pub latitude: f64,
    pub longitude: f64,
    pub country: Option<String>,
    pub city: Option<String>,
    pub org: Option<String>,
    pub screenshot_url: String,
    pub shodan_url: String,
    pub last_seen: String,
}

pub struct ShodanCameraFinder {
    client: reqwest::Client,
    api_key: String,
}

impl ShodanCameraFinder {
    pub fn new(client: reqwest::Client) -> anyhow::Result<Self> {
        let api_key = api_key()?;
        Ok(Self { client, api_key })
    }

    /// Search for open webcams within radius_km of (lat, lon).
    /// Uses Shodan search API: "has_screenshot:true screenshot.label:webcam geo:{lat},{lon},{radius_km}"
    /// Returns up to 10 results with cached screenshot URLs and locations.
    ///
    /// Ethical: Only reads Shodan's pre-existing indexed data.
    /// No auth attempts, no direct camera connections.
    pub async fn find_cameras(&self, lat: f64, lon: f64, radius_km: f64) -> anyhow::Result<Vec<CameraResult>> {
        let query = format!(
            "has_screenshot:true screenshot.label:webcam geo:{},{},{}",
            lat, lon, radius_km as u32
        );
        let url = format!(
            "https://api.shodan.io/shodan/host/search?key={}&query={}&minify=false",
            self.api_key,
            urlencode(&query),
        );

        let resp = crate::rate_limit::check_rate_limit(
            self.client.get(&url).send().await?,
            "shodan-cameras",
        )?;

        let result: SearchResult = resp.json().await?;

        let cameras: Vec<CameraResult> = result.matches.iter()
            .filter_map(|banner| {
                let ip = banner.ip_str.as_deref()?;
                let loc = banner.location.as_ref()?;
                let lat = loc.latitude?;
                let lon = loc.longitude?;

                Some(CameraResult {
                    ip: ip.to_string(),
                    port: banner.port,
                    latitude: lat,
                    longitude: lon,
                    country: loc.country_name.clone(),
                    city: loc.city.clone(),
                    org: banner.org.clone(),
                    screenshot_url: format!("https://www.shodan.io/host/{}/image", ip),
                    shodan_url: format!("https://www.shodan.io/host/{}", ip),
                    last_seen: banner.timestamp.clone().unwrap_or_default(),
                })
            })
            .take(10)
            .collect();

        info!(
            lat = lat,
            lon = lon,
            radius_km = radius_km,
            found = cameras.len(),
            "Shodan camera search complete"
        );

        Ok(cameras)
    }
}
