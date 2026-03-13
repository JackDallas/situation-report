# Shodan ICS/Infrastructure Monitor — Implementation Spec

> **Target**: Claude Code / Rust implementation
> **Plan**: Shodan Academic API (Lifetime)
> **Budget**: 199,999 query credits | 65,536 scan credits | 131,071 monitor IPs

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    SHODAN MONITOR                           │
│                                                             │
│  ┌─────────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │  Discovery   │───▶│   Monitor    │───▶│  Alert Stream │  │
│  │  (REST API)  │    │  Management  │    │  (Firehose)   │  │
│  └─────────────┘    └──────────────┘    └───────┬───────┘  │
│        │                                         │          │
│        │            ┌──────────────┐             │          │
│        └───────────▶│  On-Demand   │◀────────────┘          │
│                     │  Scanner     │   (event-triggered)    │
│                     └──────────────┘                        │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                  Storage Layer                       │   │
│  │  banners.jsonl │ baselines.db │ alerts.jsonl         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

Three-phase pipeline:
1. **Discovery** — REST API searches consume query credits, find ICS/infrastructure IPs
2. **Monitor** — Load IPs into alerts (uses monitor IP slots), enable triggers
3. **Stream** — Subscribe to private firehose (`stream.shodan.io/shodan/alert`), get 100% of banners for monitored IPs in real-time

Plus an event-driven **Scanner** that burns scan credits for immediate rescans post-strike.

---

## API Reference (Direct HTTP — no crate)

The existing Rust crates (`shodan-client`, `shodan`) are incomplete and don't support the Streaming API. Implement directly against HTTP endpoints using `reqwest`.

### Base URLs
```
REST API:      https://api.shodan.io
Streaming API: https://stream.shodan.io
InternetDB:    https://internetdb.shodan.io   (free, no auth)
```

### Authentication
All endpoints (except InternetDB) require `?key={API_KEY}` as query parameter.

### Credit Costs
| Operation | Credit Type | Cost |
|-----------|------------|------|
| `/shodan/host/search` | Query | 1 per page (100 results) |
| `/shodan/host/{ip}` | **None** | Free, unlimited |
| `/dns/domain/{domain}` | Query | 1 per page |
| `/shodan/scan` | Scan | 1 per IP |
| Alert creation | Monitor IP slots | 1 per IP in alert |
| InternetDB lookup | **None** | Free, no auth |
| Streaming API | **None** | Free with API subscription |

---

## Crate Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
futures-util = "0.3"       # for StreamExt on response bytes
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
tokio-util = { version = "0.7", features = ["io"] }
anyhow = "1"
```

---

## Data Types

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core banner object from Shodan — both search results and stream emit this.
/// The stream sends one JSON object per line (newline-delimited).
/// Fields are heavily optional — Shodan only includes what it finds.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Banner {
    pub ip_str: Option<String>,
    pub ip: Option<u64>,           // integer form
    pub port: u16,
    pub transport: Option<String>, // "tcp" or "udp"
    pub timestamp: Option<String>, // ISO 8601
    pub hostnames: Option<Vec<String>>,
    pub domains: Option<Vec<String>>,
    pub org: Option<String>,
    pub asn: Option<String>,
    pub isp: Option<String>,
    pub os: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub data: Option<String>,      // raw banner text
    pub tags: Option<Vec<String>>, // ["ics", "vpn", "starttls", ...]
    pub vulns: Option<HashMap<String, serde_json::Value>>, // CVE map
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
    pub id: Option<String>,       // unique banner ID for dedup
    pub module: Option<String>,   // "http", "ssh", "modbus", etc
    pub crawler: Option<String>,
}

/// Search API response wrapper
#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub matches: Vec<Banner>,
    pub total: u64,
}

/// Alert object returned by alert CRUD endpoints
#[derive(Debug, Deserialize, Serialize)]
pub struct Alert {
    pub id: String,
    pub name: String,
    pub created: Option<String>,
    pub size: u64,                              // number of IPs monitored
    pub filters: AlertFilters,
    pub triggers: HashMap<String, serde_json::Value>,
    pub has_triggers: bool,
    pub expires: Option<u64>,
    pub expiration: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AlertFilters {
    pub ip: Vec<String>,  // IPs or CIDR ranges
}

/// InternetDB response — free, no auth, no credits
#[derive(Debug, Deserialize)]
pub struct InternetDbEntry {
    pub ip: String,
    pub ports: Vec<u16>,
    pub cpes: Vec<String>,
    pub hostnames: Vec<String>,
    pub tags: Vec<String>,
    pub vulns: Vec<String>,
}

/// API info / plan status
#[derive(Debug, Deserialize)]
pub struct ApiInfo {
    pub plan: String,
    pub query_credits: u64,
    pub scan_credits: u64,
    pub monitored_ips: u64,
    pub usage_limits: UsageLimits,
}

#[derive(Debug, Deserialize)]
pub struct UsageLimits {
    pub query_credits: i64,   // -1 = unlimited
    pub scan_credits: i64,
    pub monitored_ips: i64,
}

/// Scan submission response
#[derive(Debug, Deserialize)]
pub struct ScanResult {
    pub id: String,
    pub count: u64,
    pub credits_left: u64,
}

/// Available trigger definition
#[derive(Debug, Deserialize)]
pub struct TriggerDef {
    pub name: String,
    pub rule: String,
    pub description: String,
}
```

---

## Phase 1: Discovery Engine

Uses REST search API to find ICS/infrastructure IPs worth monitoring.
Costs 1 query credit per page of 100 results.

### Queries

These are the discovery queries, grouped by priority. Each query targets a specific
protocol/service combination across countries of interest.

```rust
/// Discovery query with metadata for credit budgeting
struct DiscoveryQuery {
    query: &'static str,
    category: &'static str,
    priority: u8,           // 1 = critical, 2 = high, 3 = supplementary
    max_pages: u16,         // cap pages to limit credit burn
}

const DISCOVERY_QUERIES: &[DiscoveryQuery] = &[
    // ── IRAN — Critical Infrastructure (Priority 1) ──────────────
    DiscoveryQuery { query: "tag:ics country:IR",            category: "iran_ics",     priority: 1, max_pages: 50 },
    DiscoveryQuery { query: "port:502 country:IR",           category: "iran_modbus",  priority: 1, max_pages: 20 },
    DiscoveryQuery { query: "port:102 country:IR",           category: "iran_s7",      priority: 1, max_pages: 20 },
    DiscoveryQuery { query: "port:20000 country:IR",         category: "iran_dnp3",    priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:47808 country:IR",         category: "iran_bacnet",  priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:44818 country:IR",         category: "iran_etherip", priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "port:1883 country:IR",          category: "iran_mqtt",    priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:161 country:IR",           category: "iran_snmp",    priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "\"Schneider Electric\" country:IR", category: "iran_schneider", priority: 1, max_pages: 10 },
    DiscoveryQuery { query: "\"Siemens\" port:102 country:IR",   category: "iran_siemens",  priority: 1, max_pages: 10 },
    // Power grid / oil & gas specific
    DiscoveryQuery { query: "port:2404 country:IR",          category: "iran_iec104",  priority: 1, max_pages: 10 }, // IEC 60870-5-104
    DiscoveryQuery { query: "\"EnergyICT\" country:IR",      category: "iran_energy",  priority: 2, max_pages: 5 },

    // ── IRAN — Network Infrastructure ────────────────────────────
    DiscoveryQuery { query: "port:179 country:IR",           category: "iran_bgp",     priority: 2, max_pages: 10 }, // BGP routers
    DiscoveryQuery { query: "port:53 country:IR",            category: "iran_dns",     priority: 3, max_pages: 10 }, // DNS servers

    // ── GULF STATES — ICS (Collateral/Spillover) ─────────────────
    DiscoveryQuery { query: "tag:ics country:AE",            category: "uae_ics",      priority: 2, max_pages: 20 },
    DiscoveryQuery { query: "tag:ics country:BH",            category: "bahrain_ics",  priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "tag:ics country:SA",            category: "saudi_ics",    priority: 2, max_pages: 20 },
    DiscoveryQuery { query: "tag:ics country:QA",            category: "qatar_ics",    priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "tag:ics country:KW",            category: "kuwait_ics",   priority: 2, max_pages: 10 },

    // ── ISRAEL — Retaliatory Strike Detection ────────────────────
    DiscoveryQuery { query: "tag:ics country:IL",            category: "israel_ics",   priority: 1, max_pages: 30 },
    DiscoveryQuery { query: "port:502 country:IL",           category: "israel_modbus", priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "port:102 country:IL",           category: "israel_s7",    priority: 2, max_pages: 10 },

    // ── IRAQ/SYRIA/LEBANON — Transit / Spillover ─────────────────
    DiscoveryQuery { query: "tag:ics country:IQ",            category: "iraq_ics",     priority: 3, max_pages: 10 },
    DiscoveryQuery { query: "tag:ics country:SY",            category: "syria_ics",    priority: 3, max_pages: 5 },
    DiscoveryQuery { query: "tag:ics country:LB",            category: "lebanon_ics",  priority: 3, max_pages: 5 },

    // ── MARITIME — Hormuz/Port ICS ───────────────────────────────
    DiscoveryQuery { query: "port:502 org:\"port\"",         category: "maritime_modbus", priority: 2, max_pages: 10 },
    DiscoveryQuery { query: "\"NMEA\" country:IR",           category: "iran_maritime", priority: 2, max_pages: 5 },
    DiscoveryQuery { query: "\"Kongsberg\" country:IR",      category: "iran_kongsberg", priority: 3, max_pages: 5 },
];
```

### Discovery Pseudocode

```rust
/// Run all discovery queries, collect unique IPs, respect credit budget
async fn run_discovery(
    client: &reqwest::Client,
    api_key: &str,
    max_total_credits: u64,
) -> anyhow::Result<DiscoveryResults> {
    let mut results = DiscoveryResults::new();
    let mut credits_used: u64 = 0;

    // Sort queries by priority so critical ones run first if budget is tight
    let mut queries = DISCOVERY_QUERIES.to_vec();
    queries.sort_by_key(|q| q.priority);

    for query_def in &queries {
        if credits_used >= max_total_credits {
            tracing::warn!("Credit budget exhausted at {credits_used}, stopping discovery");
            break;
        }

        tracing::info!(
            query = query_def.query,
            category = query_def.category,
            "Running discovery"
        );

        let mut page = 1;
        loop {
            if page > query_def.max_pages || credits_used >= max_total_credits {
                break;
            }

            // GET https://api.shodan.io/shodan/host/search
            //   ?key={api_key}
            //   &query={query}
            //   &page={page}
            //   &minify=true        ← saves bandwidth, still has ip/port/tags/org/asn
            let url = format!(
                "https://api.shodan.io/shodan/host/search?key={}&query={}&page={}&minify=true",
                api_key,
                urlencoding::encode(query_def.query),
                page
            );

            let resp: SearchResult = client
                .get(&url)
                .send().await?
                .error_for_status()?
                .json().await?;

            credits_used += 1;

            for banner in &resp.matches {
                if let Some(ref ip) = banner.ip_str {
                    results.insert(ip.clone(), DiscoveredIp {
                        ip: ip.clone(),
                        port: banner.port,
                        category: query_def.category.to_string(),
                        priority: query_def.priority,
                        org: banner.org.clone(),
                        asn: banner.asn.clone(),
                        tags: banner.tags.clone().unwrap_or_default(),
                        country: banner.location.as_ref()
                            .and_then(|l| l.country_code.clone()),
                        product: banner.product.clone(),
                    });
                }
            }

            tracing::info!(
                total = resp.total,
                page_results = resp.matches.len(),
                unique_ips = results.len(),
                credits = credits_used,
                "Page {page} complete"
            );

            // No more pages
            if resp.matches.is_empty() || (page as u64 * 100) >= resp.total {
                break;
            }

            page += 1;

            // Rate limit: 1 req/sec for search
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    tracing::info!(
        total_ips = results.len(),
        total_credits = credits_used,
        "Discovery complete"
    );

    Ok(results)
}

/// Enrich discovered IPs with InternetDB (free, no credits, no auth)
/// Provides: ports, vulns, CPEs, hostnames, tags
/// Use for bulk pre-screening before committing monitor slots
async fn enrich_with_internetdb(
    client: &reqwest::Client,
    ips: &[String],
) -> Vec<InternetDbEntry> {
    let mut enriched = Vec::new();

    for ip in ips {
        // GET https://internetdb.shodan.io/{ip}
        // No auth needed, no credits consumed
        let url = format!("https://internetdb.shodan.io/{}", ip);

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(entry) = resp.json::<InternetDbEntry>().await {
                    enriched.push(entry);
                }
            }
            _ => {} // 404 = IP not in database, skip
        }

        // InternetDB is generous but don't hammer it
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    enriched
}

/// Free host lookup — does NOT consume query credits
/// Use for detailed single-IP investigation
async fn lookup_host(
    client: &reqwest::Client,
    api_key: &str,
    ip: &str,
) -> anyhow::Result<serde_json::Value> {
    // GET https://api.shodan.io/shodan/host/{ip}?key={api_key}
    // Cost: 0 credits
    let url = format!(
        "https://api.shodan.io/shodan/host/{}?key={}",
        ip, api_key
    );
    let resp = client.get(&url).send().await?.json().await?;
    Ok(resp)
}
```

### Credit Budget Planner

```rust
/// Calculate credit costs before execution
fn estimate_discovery_cost(queries: &[DiscoveryQuery]) -> CreditEstimate {
    let min_credits: u64 = queries.len() as u64; // at least 1 page each
    let max_credits: u64 = queries.iter()
        .map(|q| q.max_pages as u64)
        .sum();

    CreditEstimate {
        min_credits,
        max_credits,
        // Reserve 50% of monthly budget for ad-hoc searches
        recommended_cap: 100_000, // half of 199,999
    }
}
```

---

## Phase 2: Monitor Management

Load discovered IPs into Shodan alerts. Each alert can hold IPs/CIDRs.
Shodan auto-scans monitored assets at least daily.

### Alert CRUD

```rust
/// Create a new network alert (monitor)
/// Each IP in the alert consumes 1 monitor slot (131,071 total)
async fn create_alert(
    client: &reqwest::Client,
    api_key: &str,
    name: &str,
    ips: &[String],  // individual IPs or CIDR notation
) -> anyhow::Result<Alert> {
    // POST https://api.shodan.io/shodan/alert?key={api_key}
    // Body: { "name": "...", "filters": { "ip": [...] }, "expires": 0 }
    let url = format!("https://api.shodan.io/shodan/alert?key={}", api_key);

    let body = serde_json::json!({
        "name": name,
        "filters": {
            "ip": ips
        },
        "expires": 0  // 0 = never expires
    });

    let alert: Alert = client
        .post(&url)
        .json(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    tracing::info!(
        alert_id = %alert.id,
        size = alert.size,
        "Alert created: {name}"
    );

    Ok(alert)
}

/// Edit existing alert — replace IP list
async fn edit_alert(
    client: &reqwest::Client,
    api_key: &str,
    alert_id: &str,
    ips: &[String],
) -> anyhow::Result<Alert> {
    // POST https://api.shodan.io/shodan/alert/{id}?key={api_key}
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}?key={}",
        alert_id, api_key
    );

    let body = serde_json::json!({
        "filters": { "ip": ips }
    });

    let alert: Alert = client.post(&url).json(&body).send().await?
        .error_for_status()?.json().await?;

    Ok(alert)
}

/// Delete an alert
async fn delete_alert(
    client: &reqwest::Client,
    api_key: &str,
    alert_id: &str,
) -> anyhow::Result<()> {
    // DELETE https://api.shodan.io/shodan/alert/{id}?key={api_key}
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}?key={}",
        alert_id, api_key
    );
    client.delete(&url).send().await?.error_for_status()?;
    Ok(())
}

/// List all alerts on account
async fn list_alerts(
    client: &reqwest::Client,
    api_key: &str,
) -> anyhow::Result<Vec<Alert>> {
    // GET https://api.shodan.io/shodan/alert/info?key={api_key}
    let url = format!("https://api.shodan.io/shodan/alert/info?key={}", api_key);
    let alerts: Vec<Alert> = client.get(&url).send().await?
        .error_for_status()?.json().await?;
    Ok(alerts)
}
```

### Trigger Management

```rust
/// Available triggers:
///   any                      — match any service discovered
///   industrial_control_system — tag:ics
///   internet_scanner          — device is scanning the internet
///   new_service               — new port/service appears
///   open_database             — exposed database (mongo, elastic, etc)
///   iot                       — IoT device detected
///   vulnerable                — has known CVE
///   ssl_expired               — SSL cert expired
///   uncommon                  — unusual service for the host
///   malware                   — known malware indicators
///
/// For ICS monitoring, enable ALL of these:
const RECOMMENDED_TRIGGERS: &str =
    "new_service,industrial_control_system,malware,open_database,iot,vulnerable,ssl_expired,internet_scanner,uncommon";

/// Enable triggers on an alert
async fn enable_triggers(
    client: &reqwest::Client,
    api_key: &str,
    alert_id: &str,
    triggers: &str,  // comma-separated trigger names
) -> anyhow::Result<()> {
    // PUT https://api.shodan.io/shodan/alert/{id}/trigger/{trigger}?key={api_key}
    let url = format!(
        "https://api.shodan.io/shodan/alert/{}/trigger/{}?key={}",
        alert_id, triggers, api_key
    );
    client.put(&url).send().await?.error_for_status()?;
    tracing::info!(alert_id, triggers, "Triggers enabled");
    Ok(())
}
```

### Monitor Slot Allocation Strategy

```rust
/// Organize alerts by category for the 131,071 IP budget.
/// Separate alerts make it easy to manage/delete subsets.
///
/// Recommended allocation:
///   iran_ics          — up to 10,000 IPs  (Modbus/S7/DNP3/BACnet/IEC104)
///   iran_infra        — up to 15,000 IPs  (power grid, oil/gas, water)
///   iran_nuclear_area — up to  3,000 IPs  (Natanz/Fordow/Isfahan/Bushehr/Arak geolocated)
///   iran_network      — up to 10,000 IPs  (ISP gateways, BGP routers, DNS)
///   gulf_ics          — up to 15,000 IPs  (UAE/BH/SA/QA/KW ICS)
///   israel_ics        — up to 10,000 IPs  (retaliatory strike detection)
///   iraq_syria_leb    — up to  5,000 IPs  (transit corridor, spillover)
///   maritime_hormuz   — up to  5,000 IPs  (port SCADA, maritime nav)
///   ───────────────────────────────────────
///   SUBTOTAL            ~73,000 IPs
///   RESERVE             ~58,000 IPs        (expansion, ad-hoc monitoring)

struct AlertAllocation {
    name: &'static str,
    max_ips: u64,
    categories: &'static [&'static str], // maps to DiscoveryQuery.category
}

const ALERT_ALLOCATIONS: &[AlertAllocation] = &[
    AlertAllocation {
        name: "Iran ICS",
        max_ips: 10_000,
        categories: &["iran_ics", "iran_modbus", "iran_s7", "iran_dnp3",
                       "iran_bacnet", "iran_etherip", "iran_iec104",
                       "iran_schneider", "iran_siemens"],
    },
    AlertAllocation {
        name: "Iran Network Infrastructure",
        max_ips: 15_000,
        categories: &["iran_snmp", "iran_mqtt", "iran_bgp", "iran_dns", "iran_energy"],
    },
    AlertAllocation {
        name: "Gulf States ICS",
        max_ips: 15_000,
        categories: &["uae_ics", "bahrain_ics", "saudi_ics", "qatar_ics", "kuwait_ics"],
    },
    AlertAllocation {
        name: "Israel ICS",
        max_ips: 10_000,
        categories: &["israel_ics", "israel_modbus", "israel_s7"],
    },
    AlertAllocation {
        name: "Iraq Syria Lebanon ICS",
        max_ips: 5_000,
        categories: &["iraq_ics", "syria_ics", "lebanon_ics"],
    },
    AlertAllocation {
        name: "Maritime Hormuz",
        max_ips: 5_000,
        categories: &["maritime_modbus", "iran_maritime", "iran_kongsberg"],
    },
];
```

---

## Phase 3: Alert Stream (Private Firehose)

This is the core runtime loop. `stream.shodan.io/shodan/alert` returns 100% of
banners for all monitored IPs. Newline-delimited JSON, persistent HTTP connection.
Empty lines are heartbeats — ignore them.

### Stream Consumer

```rust
use futures_util::StreamExt;
use tokio::io::AsyncBufReadExt;

/// Subscribe to the private alert firehose
/// This runs forever — spawn as a tokio task
///
/// GET https://stream.shodan.io/shodan/alert?key={api_key}
///   - Returns: newline-delimited JSON (one Banner per line)
///   - Empty lines = heartbeat (keep-alive), ignore them
///   - Connection may drop — implement reconnect with backoff
///
/// To subscribe to a SPECIFIC alert only:
///   GET https://stream.shodan.io/shodan/alert/{alert_id}?key={api_key}
async fn subscribe_alert_stream(
    api_key: &str,
    banner_tx: tokio::sync::mpsc::Sender<Banner>,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(0)) // no timeout — stream is infinite
        .build()?;

    let mut backoff_secs = 1u64;

    loop {
        tracing::info!("Connecting to alert stream...");

        let url = format!(
            "https://stream.shodan.io/shodan/alert?key={}",
            api_key
        );

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                backoff_secs = 1; // reset backoff on successful connect
                tracing::info!("Connected to alert stream");

                // Stream response body line-by-line
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            buffer.push_str(&text);

                            // Process complete lines
                            while let Some(newline_pos) = buffer.find('\n') {
                                let line = buffer[..newline_pos].trim().to_string();
                                buffer = buffer[newline_pos + 1..].to_string();

                                // Skip empty lines (heartbeats)
                                if line.is_empty() {
                                    continue;
                                }

                                // Parse banner JSON
                                match serde_json::from_str::<Banner>(&line) {
                                    Ok(banner) => {
                                        if banner_tx.send(banner).await.is_err() {
                                            tracing::error!("Banner channel closed");
                                            return Ok(());
                                        }
                                    }
                                    Err(e) => {
                                        // Could be a debug event: {"event":"debug","discarded":N}
                                        // Log and continue
                                        tracing::warn!(
                                            error = %e,
                                            line_preview = &line[..line.len().min(200)],
                                            "Failed to parse banner"
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Stream chunk error");
                            break; // reconnect
                        }
                    }
                }

                tracing::warn!("Stream ended, reconnecting...");
            }
            Ok(resp) => {
                tracing::error!(
                    status = %resp.status(),
                    "Stream connection failed"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Stream connection error");
            }
        }

        // Exponential backoff: 1s, 2s, 4s, 8s, ... max 60s
        tracing::info!(backoff_secs, "Reconnecting after backoff");
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(60);
    }
}
```

### Alternative: Country/ASN/Port Filtered Streams (1% sample)

```rust
/// These streams return ~1% of global data on non-enterprise plans.
/// Useful for broad sampling but NOT comprehensive.
/// The alert stream above is preferred for monitored IPs.

/// Stream by country — newline-delimited JSON
/// GET https://stream.shodan.io/shodan/countries/{codes}?key={api_key}
/// Example: /shodan/countries/IR,IL,IQ,SA,AE,BH,QA,KW,SY,LB,YE,JO
fn country_stream_url(api_key: &str, countries: &[&str]) -> String {
    format!(
        "https://stream.shodan.io/shodan/countries/{}?key={}",
        countries.join(","),
        api_key
    )
}

/// Stream by ASN — filtered view of banners
/// GET https://stream.shodan.io/shodan/asn/{asns}?key={api_key}
/// Example: /shodan/asn/44244,197207,58224,12880
fn asn_stream_url(api_key: &str, asns: &[&str]) -> String {
    format!(
        "https://stream.shodan.io/shodan/asn/{}?key={}",
        asns.join(","),
        api_key
    )
}

/// Stream by port — e.g. only ICS ports globally
/// GET https://stream.shodan.io/shodan/ports/{ports}?key={api_key}
fn port_stream_url(api_key: &str, ports: &[u16]) -> String {
    let port_str: Vec<String> = ports.iter().map(|p| p.to_string()).collect();
    format!(
        "https://stream.shodan.io/shodan/ports/{}?key={}",
        port_str.join(","),
        api_key
    )
}

// ICS-relevant ports for port stream filter
const ICS_PORTS: &[u16] = &[
    102,    // Siemens S7comm
    502,    // Modbus
    789,    // Red Lion Crimson
    1089,   // FF HSE
    1091,   // FF HSE
    1911,   // Fox / Niagara
    2222,   // EtherNet/IP implicit
    2404,   // IEC 60870-5-104
    4840,   // OPC UA
    4911,   // Niagara Fox SSL
    9600,   // OMRON FINS
    18245,  // GE SRTP
    20000,  // DNP3
    34962,  // PROFINET
    34964,  // PROFINET
    44818,  // EtherNet/IP explicit
    47808,  // BACnet
    55553,  // Honeywell CEE
    55555,  // Crestron CTP
];
```

---

## Phase 4: Banner Processing & Alerting

Process banners from the stream, detect changes, generate alerts.

```rust
/// Banner processor — runs as a tokio task consuming from the channel
async fn process_banners(
    mut banner_rx: tokio::sync::mpsc::Receiver<Banner>,
    baseline: &BaselineDb,           // previous known state per IP:port
    alert_tx: tokio::sync::mpsc::Sender<OsintAlert>,
) {
    while let Some(banner) = banner_rx.recv().await {
        let ip = banner.ip_str.as_deref().unwrap_or("unknown");
        let port = banner.port;
        let tags = banner.tags.as_deref().unwrap_or(&[]);
        let country = banner.location.as_ref()
            .and_then(|l| l.country_code.as_deref())
            .unwrap_or("??");

        // ─── ICS Detection ───────────────────────────────────────
        if tags.contains(&"ics".to_string()) {
            alert_tx.send(OsintAlert {
                severity: Severity::High,
                kind: AlertKind::IcsBannerDetected,
                ip: ip.to_string(),
                port,
                country: country.to_string(),
                detail: format!(
                    "ICS service: {} ({})",
                    banner.product.as_deref().unwrap_or("unknown"),
                    banner.shodan_meta.as_ref()
                        .and_then(|m| m.module.as_deref())
                        .unwrap_or("unknown")
                ),
                timestamp: banner.timestamp.clone().unwrap_or_default(),
            }).await.ok();
        }

        // ─── Vulnerability Detection ─────────────────────────────
        if let Some(ref vulns) = banner.vulns {
            for cve_id in vulns.keys() {
                alert_tx.send(OsintAlert {
                    severity: Severity::Critical,
                    kind: AlertKind::VulnerabilityFound,
                    ip: ip.to_string(),
                    port,
                    country: country.to_string(),
                    detail: format!("CVE: {cve_id}"),
                    timestamp: banner.timestamp.clone().unwrap_or_default(),
                }).await.ok();
            }
        }

        // ─── Service Disappearance (Strike Damage) ───────────────
        // Compare against baseline: if an IP:port was previously serving
        // a banner and now returns error/empty/timeout, the service died.
        // This requires maintaining a baseline DB (see below).
        if let Some(prev) = baseline.get(ip, port) {
            let current_product = banner.product.as_deref().unwrap_or("");
            let prev_product = prev.product.as_deref().unwrap_or("");

            // Product changed or disappeared
            if !current_product.is_empty()
                && !prev_product.is_empty()
                && current_product != prev_product
            {
                alert_tx.send(OsintAlert {
                    severity: Severity::High,
                    kind: AlertKind::ServiceChanged,
                    ip: ip.to_string(),
                    port,
                    country: country.to_string(),
                    detail: format!("Was: {prev_product} → Now: {current_product}"),
                    timestamp: banner.timestamp.clone().unwrap_or_default(),
                }).await.ok();
            }
        }

        // ─── New Service (not in baseline) ───────────────────────
        if baseline.get(ip, port).is_none() {
            alert_tx.send(OsintAlert {
                severity: Severity::Medium,
                kind: AlertKind::NewService,
                ip: ip.to_string(),
                port,
                country: country.to_string(),
                detail: format!(
                    "New: {} on port {}",
                    banner.product.as_deref().unwrap_or("unknown"),
                    port
                ),
                timestamp: banner.timestamp.clone().unwrap_or_default(),
            }).await.ok();
        }

        // ─── Update Baseline ─────────────────────────────────────
        baseline.upsert(ip, port, &banner);

        // ─── Persist Raw Banner ──────────────────────────────────
        // Append to daily JSONL file: banners/YYYY-MM-DD.jsonl
        append_jsonl("banners", &banner);
    }
}

#[derive(Debug)]
enum Severity { Critical, High, Medium, Low, Info }

#[derive(Debug)]
enum AlertKind {
    IcsBannerDetected,
    VulnerabilityFound,
    ServiceChanged,
    ServiceDisappeared,
    NewService,
    MassOutage,          // multiple IPs in same ASN go dark simultaneously
    ScannerDetected,     // IP tagged as internet_scanner
}

struct OsintAlert {
    severity: Severity,
    kind: AlertKind,
    ip: String,
    port: u16,
    country: String,
    detail: String,
    timestamp: String,
}
```

### Mass Outage Detection

```rust
/// Detect when multiple IPs in the same ASN/country stop responding.
/// Run periodically (e.g., every 5 minutes) against baseline.
///
/// If Shodan's daily scan cycle shows N previously-live IPs in an ASN
/// now have no recent banners, and this exceeds a threshold,
/// flag as potential strike/infrastructure damage.
///
/// Cross-reference with:
///   - IODA (internet connectivity)
///   - FIRMS (thermal hotspots)
///   - VIIRS (nighttime lights)
///   - USGS (seismic events)
fn detect_mass_outage(baseline: &BaselineDb) -> Vec<MassOutageEvent> {
    let mut events = Vec::new();

    // Group baseline entries by ASN
    // For each ASN, count IPs with banners older than 48 hours
    //   (Shodan rescans monitored assets at least daily)
    // If >30% of ASN's IPs have stale banners → potential outage
    // If >60% → high confidence infrastructure damage

    // Pseudocode:
    //   for (asn, ips) in baseline.group_by_asn() {
    //       let stale = ips.iter()
    //           .filter(|ip| ip.last_seen < now - 48h)
    //           .count();
    //       let ratio = stale as f64 / ips.len() as f64;
    //       if ratio > 0.30 {
    //           events.push(MassOutageEvent { asn, ratio, stale_count: stale, ... });
    //       }
    //   }

    events
}
```

---

## Phase 5: On-Demand Scanner (Event-Triggered)

When external signals confirm a strike event (FIRMS thermal, USGS seismic, NOTAM
airspace closure), burn scan credits to force immediate rescans of affected IPs.

```rust
/// Submit IPs for immediate on-demand scanning
/// Cost: 1 scan credit per IP
/// Results appear in the alert stream (not returned directly)
///
/// POST https://api.shodan.io/shodan/scan?key={api_key}
/// Body: ips={comma-separated IPs}
async fn submit_scan(
    client: &reqwest::Client,
    api_key: &str,
    ips: &[String],
) -> anyhow::Result<ScanResult> {
    let url = format!("https://api.shodan.io/shodan/scan?key={}", api_key);

    let body = [("ips", ips.join(","))];

    let result: ScanResult = client
        .post(&url)
        .form(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    tracing::info!(
        scan_id = %result.id,
        count = result.count,
        credits_remaining = result.credits_left,
        "On-demand scan submitted"
    );

    Ok(result)
}

/// Check scan status
/// GET https://api.shodan.io/shodan/scan/{id}?key={api_key}
async fn check_scan_status(
    client: &reqwest::Client,
    api_key: &str,
    scan_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let url = format!(
        "https://api.shodan.io/shodan/scan/{}?key={}",
        scan_id, api_key
    );
    let status = client.get(&url).send().await?.json().await?;
    Ok(status)
}

/// Event-driven rescan: triggered by external OSINT signals
///
/// Input: country code or ASN where strike was detected
/// Action: pull all monitored IPs in that region, submit for rescan
async fn emergency_rescan(
    client: &reqwest::Client,
    api_key: &str,
    baseline: &BaselineDb,
    country: Option<&str>,
    asn: Option<&str>,
    max_credits: u64,
) -> anyhow::Result<()> {
    let mut target_ips: Vec<String> = Vec::new();

    // Filter baseline by country/ASN
    for entry in baseline.iter() {
        let matches = match (country, asn) {
            (Some(c), _) => entry.country.as_deref() == Some(c),
            (_, Some(a)) => entry.asn.as_deref() == Some(a),
            _ => false,
        };

        if matches {
            target_ips.push(entry.ip.clone());
        }

        if target_ips.len() as u64 >= max_credits {
            break;
        }
    }

    tracing::info!(
        count = target_ips.len(),
        country = country.unwrap_or("any"),
        asn = asn.unwrap_or("any"),
        "Emergency rescan targeting"
    );

    // Shodan accepts batches of IPs
    // Split into chunks if very large
    for chunk in target_ips.chunks(1000) {
        let ips: Vec<String> = chunk.to_vec();
        submit_scan(client, api_key, &ips).await?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    Ok(())
}
```

---

## Utility: Plan Status Check

```rust
/// GET https://api.shodan.io/api-info?key={api_key}
/// Use this to check remaining credits before any operation
async fn get_plan_info(
    client: &reqwest::Client,
    api_key: &str,
) -> anyhow::Result<ApiInfo> {
    let url = format!("https://api.shodan.io/api-info?key={}", api_key);
    let info: ApiInfo = client.get(&url).send().await?
        .error_for_status()?.json().await?;
    Ok(info)
}
```

---

## Main Entrypoint Pseudocode

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::init();

    let api_key = std::env::var("SHODAN_API_KEY")
        .expect("SHODAN_API_KEY env var required");

    let client = reqwest::Client::new();

    // ── Step 0: Check plan status ────────────────────────────────
    let info = get_plan_info(&client, &api_key).await?;
    tracing::info!(
        plan = %info.plan,
        query_credits = info.query_credits,
        scan_credits = info.scan_credits,
        monitored_ips = info.monitored_ips,
        "Plan info"
    );

    // ── Step 1: Discovery ────────────────────────────────────────
    // Run once or periodically (e.g., weekly) to find new IPs
    // Cap at 100K query credits to keep 100K in reserve
    let discovered = run_discovery(&client, &api_key, 100_000).await?;

    // Enrich with InternetDB (free)
    let enriched = enrich_with_internetdb(
        &client,
        &discovered.ips().collect::<Vec<_>>()
    ).await;

    // ── Step 2: Create/Update Monitors ───────────────────────────
    for allocation in ALERT_ALLOCATIONS {
        let ips: Vec<String> = discovered
            .filter_by_categories(allocation.categories)
            .take(allocation.max_ips as usize)
            .collect();

        if ips.is_empty() {
            continue;
        }

        let alert = create_alert(&client, &api_key, allocation.name, &ips).await?;
        enable_triggers(&client, &api_key, &alert.id, RECOMMENDED_TRIGGERS).await?;

        tracing::info!(
            name = allocation.name,
            ips = ips.len(),
            alert_id = %alert.id,
            "Monitor created"
        );
    }

    // ── Step 3: Start Stream Consumer ────────────────────────────
    let (banner_tx, banner_rx) = tokio::sync::mpsc::channel::<Banner>(10_000);
    let (alert_tx, mut alert_rx) = tokio::sync::mpsc::channel::<OsintAlert>(1_000);

    let baseline = BaselineDb::open("baseline.db")?;

    // Spawn stream consumer (runs forever, reconnects on drop)
    let stream_key = api_key.clone();
    let stream_handle = tokio::spawn(async move {
        subscribe_alert_stream(&stream_key, banner_tx).await
    });

    // Spawn banner processor
    let baseline_clone = baseline.clone();
    let processor_handle = tokio::spawn(async move {
        process_banners(banner_rx, &baseline_clone, alert_tx).await
    });

    // Spawn alert consumer (log, webhook, dashboard integration)
    let alert_handle = tokio::spawn(async move {
        while let Some(alert) = alert_rx.recv().await {
            tracing::warn!(
                severity = ?alert.severity,
                kind = ?alert.kind,
                ip = %alert.ip,
                port = alert.port,
                country = %alert.country,
                detail = %alert.detail,
                "OSINT ALERT"
            );

            // TODO: send to dashboard websocket
            // TODO: send webhook notification
            // TODO: cross-reference with FIRMS/IODA/USGS events
        }
    });

    // Spawn mass outage detector (periodic check)
    let baseline_clone2 = baseline.clone();
    let outage_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_secs(300) // every 5 min
        );
        loop {
            interval.tick().await;
            let outages = detect_mass_outage(&baseline_clone2);
            for outage in outages {
                tracing::error!(
                    asn = %outage.asn,
                    stale_ratio = outage.ratio,
                    stale_count = outage.stale_count,
                    "MASS OUTAGE DETECTED"
                );
            }
        }
    });

    // Wait for all tasks
    tokio::select! {
        _ = stream_handle => tracing::error!("Stream task exited"),
        _ = processor_handle => tracing::error!("Processor task exited"),
        _ = alert_handle => tracing::error!("Alert task exited"),
        _ = outage_handle => tracing::error!("Outage detector exited"),
    }

    Ok(())
}
```

---

## Key Operational Notes

### Rate Limits
- REST API: ~1 request/second sustained (Shodan doesn't publish exact limits, but 1/sec is safe)
- Streaming API: No rate limit (it's a persistent connection, data pushes to you)
- InternetDB: No documented limit, but ~10 req/sec is courteous

### Credit Reset
- Query and scan credits reset on the **1st of each month**
- Monitor IP slots are persistent (not monthly)

### Streaming API Behaviour
- Connection is HTTP with chunked transfer encoding
- Heartbeat: empty newlines sent periodically to keep connection alive
- If client can't keep up, Shodan drops banners (use `?debug=1` to see `{"event":"debug","discarded":N}`)
- Connection drops: implement reconnect with exponential backoff
- All banners for monitored IPs flow through, not just trigger matches

### Banner Deduplication
- Each banner has a unique `_shodan.id` field
- Use this to dedup if running multiple stream consumers

### Data Volume Estimate
- Shodan scans monitored assets at least daily
- For ~50,000 monitored IPs, expect ~50,000-200,000 banners/day
  (multiple ports per IP, multiple services per port)
- At ~1KB per banner JSON, that's ~50-200MB/day uncompressed
- Compress with gzip for storage (~5-20MB/day)

### What the Alert Stream Does NOT Give You
- Historical data (use search API or `shodan download` for that)
- IPs not in your monitor list (use country/ASN streams for sampling)
- Banner data during your connection downtime (no replay capability — use bulk downloads to fill gaps)

---

## Cross-Reference Integration Points

The Shodan monitor feeds into the broader OSINT dashboard pipeline:

| Shodan Signal | Cross-Reference With | Combined Intelligence |
|---|---|---|
| ICS banner disappears (Iran ASN) | FIRMS thermal hotspot at same time | Confirmed strike on industrial facility |
| Mass IPs go dark in Iran ASN | IODA connectivity drop + VIIRS nighttime lights | Infrastructure-level destruction |
| New ICS vuln appears on Iran IP | NOTAM airspace closure nearby | Pre-strike reconnaissance or staging |
| Scanner tag on IP probing Israel ICS | GreyNoise classification | Targeted recon vs background noise |
| Service change on Bushehr-area IP | USGS seismic event + CTBTO radionuclide | Nuclear facility damage assessment |
| Multiple Gulf ICS go dark | AIS vessel traffic Hormuz drops | Hormuz strait infrastructure attack |
