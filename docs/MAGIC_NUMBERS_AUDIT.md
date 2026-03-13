# Magic Numbers Audit — Situation Report Backend

**Date:** 2026-03-03
**Scope:** All `.rs` files in `backend/crates/` (sources, intel, embeddings, entity-graph, pipeline, db, server, types, config)
**Baseline:** `PipelineConfig` in `backend/crates/config/src/lib.rs` already externalizes ~80 parameters from `situation_graph.rs` and `pipeline.rs`. Items already in `PipelineConfig` are **not listed** here.

---

## Table of Contents

1. [sr-sources (Data Sources)](#1-sr-sources)
2. [sr-intel (Intelligence Layer)](#2-sr-intel)
3. [sr-embeddings (Vector Embeddings)](#3-sr-embeddings)
4. [sr-entity-graph (Entity Graph)](#4-sr-entity-graph)
5. [sr-pipeline (Pipeline — non-config items)](#5-sr-pipeline)
6. [sr-db (Database)](#6-sr-db)
7. [sr-server (HTTP Server)](#7-sr-server)
8. [Summary Statistics](#8-summary-statistics)
9. [Priority Recommendations](#9-priority-recommendations)

---

## 1. sr-sources

### 1.1 registry.rs — Source Orchestration

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 91 | `"SituationReport/0.1"` | HTTP User-Agent string | **Configure** — version should track release |
| 92 | `Duration::from_secs(30)` | Global HTTP client timeout | **Configure** — per-source override may be needed |
| 129-130 | `Duration::from_secs(10)` | Stream reconnect delay | **Configure** |
| 134-135 | `10u64 * 2u64.pow(consecutive_failures.min(4)).min(300)` | Stream backoff: base=10s, max=300s, power cap=4 | **Configure** — expose base, max, exponent cap |
| 278 | `30 * (consecutive_failures as u64)).min(600)` | 429 rate-limit additive backoff, max 600s | **Configure** |
| 319 | `30u64 * 2u64.pow(consecutive_failures.min(4)).min(300)` | Error backoff: base=30s, max=300s | **Configure** |

### 1.2 rate_limit.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 28 | `DEFAULT_RETRY_SECS: u64 = 60` | Default Retry-After when header absent | **Configure** |

### 1.3 common.rs — Region/Country Mappings

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| all | `callsign_country()` match table | Callsign prefix -> country | **Leave as-is** — static reference data |
| all | `mmsi_country()` match table | MMSI prefix -> country | **Leave as-is** — static reference data |
| all | `region_for_country()` match table | Country -> region mapping | **Leave as-is** — could be data file but rarely changes |
| all | `country_center()` / `region_center()` | Lat/lon centers for countries/regions | **Leave as-is** — geographic constants |

### 1.4 gdelt.rs — GDELT Doc API

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16-24 | `QUERIES` array (7 strings) | Search terms polled in rotation | **Configure** — operational tuning |
| 108 | `Duration::from_secs(15 * 60)` | Poll interval (15 min) | Already overridable via DB `source_config.poll_interval_secs` |
| 121 | `maxrecords=250` | Max records per API request | **Configure** |
| 126 | `Duration::from_secs(15)` | Per-request HTTP timeout | **Configure** (shared with registry timeout?) |
| 130 | `Duration::from_secs(2)` | Retry delay | **Configure** |
| 205-207 | Tone thresholds `-5.0`, `-2.0` | Severity mapping: <-5=High, <-2=Medium, else Low | **Configure** |

### 1.5 gdelt_geo.rs — GDELT GEO 2.0

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 15-23 | `GEO_QUERIES` array (7 strings) | Geographic queries polled in rotation | **Configure** |
| 64 | `Duration::from_secs(20 * 60)` | Poll interval (20 min) | Already overridable via DB |
| 74 | `maxpoints=250` | Max points per API request | **Configure** |
| 80 | `Duration::from_secs(15)` | Per-request HTTP timeout | **Configure** |
| 84 | `Duration::from_secs(2)` | Retry delay | **Configure** |
| 135 | Tone thresholds `-5.0`, `-2.0` | Same as gdelt.rs | **Configure** |

### 1.6 geoconfirmed.rs — GeoConfirmed

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 14 | `BASE_URL` | API base URL | **Leave as-is** — rarely changes |
| 16 | `PAGE_SIZE: u32 = 50` | Items per page | **Configure** |
| 20-28 | `CONFLICTS` array (7 conflict/region pairs) | Which conflicts to poll | **Configure** — operational tuning |
| 87-110 | Equipment category ranges (10-19=tank, etc.) | Icon number -> equipment name mapping | **Leave as-is** — GeoConfirmed API spec |
| 134 | `Duration::from_secs(60 * 60)` | Poll interval (60 min) | Already overridable via DB |

### 1.7 opensky.rs — OpenSky Network

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 17-22 | `BOUNDING_BOXES` (4 regions) | Geographic areas to monitor | **Configure** |
| 25-43 | `MILITARY_PREFIXES` (16 callsign prefixes) | Military callsign filter | **Configure** — need to add/remove prefixes operationally |
| 46 | `TOKEN_URL` | OAuth2 endpoint | **Leave as-is** — API spec |
| 49 | `TOKEN_EXPIRY_MARGIN_SECS: u64 = 30` | Pre-expiry refresh margin | **Leave as-is** |
| 128 | `.take(300)` | Error body truncation for logging | **Leave as-is** |
| 172 | `Duration::from_secs(90)` | Poll interval (90s) | Already overridable via DB |

### 1.8 adsb.rs — ADS-B Flight Tracking (AirplanesLive, adsb.lol, adsb.fi)

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 29 | `USER_AGENT` string | HTTP User-Agent | **Configure** (shared with registry) |
| 32-40 | `HIGH_VALUE_TYPES` (17 aircraft type codes) | Type codes triggering events | **Configure** |
| 43-59 | `MILITARY_CALLSIGN_PREFIXES` (16 prefixes) | Military aircraft filter | **Configure** (overlaps opensky) |
| 62-67 | `POINT_QUERIES` (4 lat/lon/radius queries) | Geographic monitoring points | **Configure** |
| 547 | `Duration::from_millis(1500)` | Minimum inter-request gap | **Configure** |
| 548 | `Duration::from_secs(120)` | Poll interval (120s) | Already overridable via DB |

### 1.9 ais.rs — Maritime AIS

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 12 | `KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30)` | WebSocket keepalive | **Leave as-is** |
| 20 | `AISSTREAM_URL` | WebSocket endpoint | **Leave as-is** — API spec |
| 24-31 | `BOUNDING_BOXES` (6 maritime regions) | Monitored sea areas | **Configure** |
| 34-38 | `MILITARY_MMSI_PREFIXES` (3 prefixes) | Naval vessel filter | **Configure** |
| 418 | `Duration::from_secs(60)` | Stats logging interval | **Leave as-is** |
| 506 | `message_count % 1000` | Log every Nth message | **Leave as-is** |

### 1.10 firms.rs — NASA FIRMS

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 18-24 | `BOUNDING_BOXES` (5 regions) | Monitored areas | **Configure** |
| 96 | `Duration::from_secs(30 * 60)` | Poll interval (30 min) | Already overridable via DB |
| 117 | `/1` in URL path | 1-day data range | **Configure** — could want 2-day for redundancy |
| 173 | `seen.len() > 50_000` | Dedup set rotation cap | **Configure** |
| 179-183 | FRP thresholds `100.0`, `50.0` | Severity: >100=High, >50=Medium, else Low | **Configure** |
| 187-190 | Confidence mappings `0.9`, `0.6`, `0.3` | "high"->0.9, "nominal"->0.6, "low"->0.3 | **Configure** |

### 1.11 usgs.rs — Earthquake Data

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16-17 | `FEED_URL` | USGS GeoJSON feed URL | **Leave as-is** — API spec |
| 20-25 | `BOUNDING_BOXES` (4 regions) | Monitored seismic zones | **Configure** |
| 89 | `5.0` km depth threshold | Shallow quake / possible explosion filter | **Configure** |
| 127 | `Duration::from_secs(300)` | Poll interval (5 min) | Already overridable via DB |
| 211-217 | Magnitude thresholds `5.0`, `3.0` | Severity: >=5=High, >=3=Medium | **Configure** |
| 259 | `seen.len() > 10_000` | Dedup set rotation cap | **Configure** |

### 1.12 notam.rs — NOTAM Aviation Notices

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| ~15 | `NATS_UK_URL` | NATS UK API base URL | **Leave as-is** — API spec |
| ~20-40 | `CRITICAL_FIRS` list | FIRs of interest | **Configure** — operational tuning |
| ~45-55 | `PRIORITY_QCODE_PREFIXES` | Q-code patterns for priority NOTAMs | **Configure** |
| ~60-80 | `NATS_CONFLICT_QCODES` | Q-codes indicating conflict | **Configure** |
| ~85 | `NATS_POLL_INTERVAL_SECS: 3600` | Poll interval (60 min) | Already overridable via DB |

### 1.13 shodan.rs — Shodan (Stream + Discovery + Search)

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| ~20-40 | `ICS_PORTS` list (19 ports) | Industrial control system port filter | **Configure** — new ICS protocols emerge |
| various | API URLs | Shodan API endpoints | **Leave as-is** — API spec |
| various | Poll intervals | Various timing parameters | Already overridable via DB |

### 1.14 cloudflare.rs — Cloudflare Radar

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16-18 | `COUNTRIES` (12 country codes) | Countries to monitor | **Configure** |
| 21 | `COUNTRIES_PER_CYCLE: usize = 2` | Countries per poll cycle (rotation) | **Configure** |
| 91, 180 | `dateRange=7d` | Radar lookback window | **Configure** |
| 268 | `Duration::from_secs(15 * 60)` | Radar poll interval (15 min) | Already overridable via DB |
| 301 | `Duration::from_millis(250)` | Inter-country request delay | **Configure** |
| 326 | `Duration::from_secs(30 * 60)` | BGP poll interval (30 min) | Already overridable via DB |
| 337 | `per_page=25` | BGP events page size | **Configure** |

### 1.15 ioda.rs — Internet Outage Detection

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 15 | `COUNTRIES` (6 country codes) | Countries to monitor | **Configure** |
| 203 | `Duration::from_secs(10 * 60)` | Poll interval (10 min) | Already overridable via DB |
| 210 | `20 * 60` | Lookback window (20 min) | **Configure** |

### 1.16 bgp.rs — BGP Route Monitoring

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 21 | `DEDUP_WINDOW: Duration = Duration::from_secs(300)` | 5-min dedup window | **Configure** |
| 28-46 | `MONITORED_ASNS` (13 ASN entries) | ASNs to watch | **Configure** — high operational value |
| 49 | `RIS_LIVE_URL` | RIPE RIS Live WebSocket URL | **Leave as-is** — API spec |
| 52 | `SUBSCRIBE_MSG` | WebSocket subscribe JSON | Derived from `MONITORED_ASNS` |
| 309 | `Duration::from_secs(300)` | Stats logging interval | **Leave as-is** |

### 1.17 otx.rs — AlienVault OTX

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 14-20 | `SEARCH_QUERIES` (5 queries) | Threat search terms | **Configure** |
| 84 | `Duration::hours(2)` | Lookback window (2 hr) | **Configure** |
| 89 | `limit=50` | Subscribed pulse limit | **Configure** |
| 133 | `limit=20` | Search pulse limit | **Configure** |
| 214 | `Duration::from_secs(60 * 60)` | Poll interval (1 hr) | Already overridable via DB |

### 1.18 certstream.rs — Certificate Transparency

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16 | `CERTSTREAM_URL` | WebSocket endpoint | **Leave as-is** — API spec |
| 20-31 | `DOMAIN_PATTERNS` (10 suffixes) | Domain patterns to watch | **Configure** — very high operational value |
| 130, 257 | `backoff_secs * 2).min(60)` | Max reconnect backoff (60s) | **Configure** |
| 245 | `.take(5)` | Max tags from domain list | **Leave as-is** |

### 1.19 ooni.rs — OONI Censorship Detection

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 15 | `COUNTRIES` (7 country codes) | Countries to monitor | **Configure** |
| 85 | `Duration::from_secs(30 * 60)` | Poll interval (30 min) | Already overridable via DB |
| 92 | `Duration::hours(2)` | Lookback window (2 hr) | **Configure** |
| 97 | `limit=100` | Max measurements per query | **Configure** |

### 1.20 nuclear.rs — Nuclear/Radiation Monitoring

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16-20 | `REGIONS` (3 regions with km distances) | Nuclear monitoring regions + radii | **Configure** |
| 23 | `BASELINE_CPM: f64 = 50.0` | Normal background radiation (counts/min) | **Configure** |
| 26 | `ALERT_CPM: f64 = 100.0` | Alert threshold (counts/min) | **Configure** |
| 83 | `Duration::from_secs(1800)` | Poll interval (30 min) | Already overridable via DB |
| 94 | `Duration::hours(24)` | Lookback window (24 hr) | **Configure** |
| 216 | `seen.len() > 50_000` | Dedup set rotation cap | **Configure** |

### 1.21 gfw.rs — Global Fishing Watch

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| various | `COMBINED_BBOX` + 3 sub-bounding boxes | Monitored maritime areas | **Configure** |
| 47 | `PAGE_LIMIT: u32 = 100` | API page size | **Configure** |
| ~50 | `EVENTS_API_URL` | GFW API endpoint | **Leave as-is** |
| 100 | `Duration::from_secs(30 * 60)` | Poll interval (30 min) | Already overridable via DB |
| 115 | `Duration::hours(24)` | Lookback window (24 hr) | **Configure** |

### 1.22 gpsjam.rs — GPS Interference

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 27 | `Duration::from_secs(6 * 60 * 60)` | Poll interval (6 hr) | Already overridable via DB |
| 86 | `percentage < 10.0` | Minimum interference threshold to emit event | **Configure** |
| 90-97 | Thresholds `50.0`, `30.0`, `15.0` | Severity: >50=Critical, >30=High, >15=Medium | **Configure** |

### 1.23 telegram.rs — Telegram OSINT

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 29-118 | `CHANNELS` (14 channel configs) | Telegram channels + region/tags | **Configure** — highest operational tuning need |
| 125-136 | `CRITICAL_KEYWORDS` (16), `HIGH_KEYWORDS` (14) | Severity keyword lists | **Configure** |
| 200 | `.take(100)` | Title truncation (100 chars) | **Leave as-is** |
| 202 | `.take(500)` | Description truncation (500 chars) | **Leave as-is** |
| 392, 609 | `backoff_secs * 2).min(120)` | Max reconnect backoff (120s) | **Configure** |
| 456-463 | `INTERVAL '12 hours'` | Backfill lookback window | **Configure** |
| 475 | `.limit(200)` | Backfill message limit | **Configure** |
| 500 | `Duration::from_millis(500)` | Resolve delay | **Leave as-is** |
| 520 | `Duration::from_secs(2)` | Inter-channel delay | **Leave as-is** |
| 533 | `update_queue_limit: Some(500)` | Telegram update queue capacity | **Configure** |
| 539 | `Duration::from_secs(300)` | Stats logging interval | **Leave as-is** |
| 584 | `message_count % 100` | Log every Nth message | **Leave as-is** |

### 1.24 rss_news.rs — RSS News Feeds

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 15-16 | `RSS_USER_AGENT` | HTTP User-Agent for RSS | **Configure** (shared) |
| 29-67 | `FEEDS` (25 feed URLs + regions) | RSS feed list | **Configure** — core operational tuning |
| 70 | `FEEDS_PER_POLL: usize = 3` | Feeds per poll cycle (rotation) | **Configure** |
| 254 | `Duration::from_secs(5 * 60)` | Poll interval (5 min) | Already overridable via DB |
| 278 | `Duration::from_secs(10)` | Per-feed request timeout | **Configure** |
| 315 | `current.len() > 5_000` | Dedup buffer rotation threshold | **Configure** |
| 330 | `item.description.len() > 2000` | Description truncation | **Leave as-is** |

---

## 2. sr-intel

### 2.1 client.rs — Claude API Client

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 11 | `API_URL = "https://api.anthropic.com/v1/messages"` | Anthropic API endpoint | **Leave as-is** |
| 12 | `API_VERSION = "2023-06-01"` | API version header | **Leave as-is** — API spec |
| 82 | `Duration::from_secs(120)` | HTTP client timeout | **Configure** |
| 125 | `0..4u32` | Max retry attempts | **Configure** |
| 127 | `500 * 2u64.pow(attempt)` | Retry backoff: 500ms base, exponential | **Configure** |
| 136 | `"prompt-caching-2024-07-31"` | Beta feature header | **Leave as-is** — API spec |

### 2.2 enrich.rs — Haiku Enrichment

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 13 | `"claude-haiku-4-5-20251001"` | Default enrichment model | **Already env-configurable** via `INTEL_ENRICHMENT_MODEL` |
| 41 | `max_tokens: 1024` | Enrichment response max tokens | **Configure** |

### 2.3 analyze.rs — Sonnet Analysis

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 18 | `"claude-sonnet-4-6"` | Default analysis model | **Already env-configurable** via `INTEL_ANALYSIS_MODEL` |
| 52 | `max_tokens: 8192` | Analysis response max tokens | **Configure** |
| 255-261 | Tempo thresholds `20.0`, `5.0` events/min | HIGH/ELEVATED/NORMAL tempo classification | **Configure** |
| 265-270 | Analysis intervals `900`, `3600`, `7200` secs | HIGH=15min, ELEVATED=60min, NORMAL=120min | **Configure** |

### 2.4 budget.rs — Budget Management

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 18-22 | `HAIKU_PRICING: (1.0, 5.0, 0.10)` per M tokens | Haiku input/output/cache pricing | **Configure** — prices change with model updates |
| 24-28 | `SONNET_PRICING: (3.0, 15.0, 0.30)` per M tokens | Sonnet input/output/cache pricing | **Configure** — prices change |
| 98 | `10.0` USD | Default daily budget cap | **Already env-configurable** via `INTEL_DAILY_BUDGET_USD` |
| 195 | `0.8` (80%) | Sonnet budget threshold (skip Sonnet above 80% spend) | **Configure** |

### 2.5 ollama.rs — Local GPU Enrichment

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 129 | `"qwen2.5:7b"` | Default Ollama model | **Already env-configurable** via `OLLAMA_MODEL` |
| 132 | `Duration::from_secs(120)` | HTTP timeout | **Configure** |
| 138 | `Semaphore::new(1)` | GPU concurrency limit | **Configure** — multi-GPU setups |
| 173-176 | `Duration::from_secs(5)` | Health check timeout | **Leave as-is** |
| 244 | `num_ctx: 4096` | Enrichment context window | **Configure** |
| 303 | `num_ctx: max_tokens.max(2048)` | Minimum context window | **Configure** |
| 350 | `num_ctx: 8192, temperature: 0.1` | Narrative generation params | **Configure** |
| 402 | `num_ctx: 8192, temperature: 0.1` | Analysis generation params | **Configure** |
| 466 | `num_ctx: 2048` | Merge audit context window | **Configure** |

### 2.6 narrative.rs — Situation Narrative Generation

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 100 | "Keep under 300 words total" | Narrative word limit (in prompt) | **Leave as-is** — prompt engineering |
| 121, 179 | `1500` | Narrative max_tokens | **Configure** |
| 147 | `source_types.len() >= 3` | Sonnet escalation: multi-source threshold | **Configure** |
| 247 | `event_count_since >= 30` | Regen trigger: significant new events | **Configure** |
| 254 | `Duration::minutes(120)`, `event_count_since >= 10` | Time-based regen: 120min + 10 events | **Configure** |
| 284 | `.take(15)` | Max recent events in prompt context | **Configure** |

### 2.7 titles.rs — Title Generation

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 22 | `entities.len() <= 5` | Entity filter bypass threshold | **Leave as-is** |
| 54 | `title_mentions * 2` | Entity scoring: +2 per title mention | **Leave as-is** — internal scoring |
| 73 | `part.len() >= 3` | Minimum name-part length for partial matching | **Leave as-is** |
| 76 | `part_mentions >= 2` | Partial match threshold | **Leave as-is** |
| 96-99 | `*score >= 1`, `.take(8)` | Entity relevance filter: min score 1, max 8 | **Leave as-is** |
| 106 | `.take(5)` | Fallback: first 5 entities | **Leave as-is** |
| 143 | `2048` | Ollama title completion max_tokens | **Configure** |
| 165 | `40` | Claude title completion max_tokens | **Configure** |

### 2.8 prompts.rs — Prompt Templates

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 78-79 | Description truncation `> 2000` -> `[..2000]` | User prompt description limit | **Leave as-is** — token budget |
| 218 | `.take(3)` | Max enrichment summaries in title prompt | **Leave as-is** |
| 225 | `.saturating_sub(5)` | Last 5 event titles for context | **Leave as-is** |
| 236-238 | `AGGREGATE_EVENT_TYPES` (5 types) | Types aggregated in analysis prompt | **Leave as-is** — prompt design |
| 257 | `.min(50)` | Max situations in analysis prompt | **Leave as-is** — token budget |
| 292 | `entry.1.len() < 3` | Max sample titles per aggregated type | **Leave as-is** |
| 305 | `.take(40)` | Max signal events in analysis prompt | **Leave as-is** — token budget |

### 2.9 search.rs — Exa Web Search

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 16 | `MAX_ARTICLES_PER_CLUSTER: usize = 10` | Stored article cap per cluster | **Configure** |
| 63 | `EXA_DAILY_CAP: u32 = 1_400` | Max Exa API requests per day ($10/day) | **Configure** |
| 104 | `hourly_count >= 60` | Hourly search cap | **Configure** |
| 110 | `Duration::from_secs(30)` | Per-request cooldown | **Configure** |
| 129 | `Duration::from_secs(86_400)` | Daily counter reset period | **Leave as-is** — 24h is correct |
| 144 | `0.007` | Exa cost per request (USD) | **Configure** — pricing changes |
| 162-163 | `GENERIC_REGIONS` list | Regions excluded from search queries | **Leave as-is** |

---

## 3. sr-embeddings

### 3.1 model.rs — BGE-M3 Model

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 25 | `fastembed::EmbeddingModel::BGEM3` | Model selection | **Leave as-is** — structural |

### 3.2 cache.rs — Embedding Cache

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| (constructor) | `max_events` parameter | Cache size — set by caller | **Already configurable** — passed from pipeline |

### 3.3 compose.rs — Text Composition

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 39-40 | `> 500` -> `[..500]` | Description truncation for embedding input | **Leave as-is** |

### 3.4 store.rs — pgvector Operations

No magic numbers found. All parameters are function arguments.

---

## 4. sr-entity-graph

### 4.1 graph.rs — In-Memory Graph

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 18 | `0.5` per hop | Impact decay factor (1.0 at source, 0.5^hops) | **Leave as-is** — well-reasoned default |
| 239 | `.truncate(20)` | Max impact assessments returned | **Leave as-is** |

### 4.2 model.rs — Entity Types and State Changes

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 144-157 | `trigger_keywords()` (11 keyword->change mappings) | State change detection keywords | **Leave as-is** — ACE 2005 ontology |
| 228 | `confidence: 0.5` | Default new entity confidence | **Leave as-is** |
| 169 | `default_confidence() -> 0.5` | Default relationship confidence | **Leave as-is** |
| 184-186 | `default_certainty() -> "alleged"` | Default state change certainty | **Leave as-is** |
| 294 | `source_reliability: 'F', info_credibility: '6'` | NATO STANAG defaults | **Leave as-is** |

### 4.3 resolve.rs — Entity Resolution

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 125, 131 | `< 4` | Minimum name length for fuzzy matching | **Leave as-is** |
| 139 | `> 0.5` | Substring overlap ratio threshold | **Configure** — affects entity resolution quality |
| 145 | `> 0.6` | Trigram similarity threshold | **Configure** — affects entity resolution quality |
| 175-178 | Corporate suffix list (12 suffixes) | Name normalization suffixes | **Leave as-is** — reference data |

### 4.4 state.rs — State Change Detection

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 56 | `.saturating_sub(3)` | Entity name fragment: last 1-3 words | **Leave as-is** |
| 58 | `entity.len() >= 3` | Minimum entity fragment length | **Leave as-is** |

### 4.5 queries.rs — DB Queries

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 174 | `LIMIT 20` | Max recent state changes per entity | **Leave as-is** |

---

## 5. sr-pipeline (non-config items)

Items already in `PipelineConfig` are omitted. The following are NOT yet externalized.

### 5.1 situation_graph.rs — Cluster Lifecycle

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 260-268 | `decay_params()` per-event-type half-life/offset | Temporal decay parameters by event type | Partially in config — the function exists but not all mappings are externalized. **Review overlap** |
| 286-293 | `geo_radius_km()` per-event-type radii | Geo matching radii by event type | Partially in config — **review overlap** |
| 302-309 | `size_penalty()` thresholds 20/40/60/80 | Cluster size penalty tiers | **Already in config** via `cluster_caps.size_penalty_tiers` |
| 452-498 | Region code -> country name mapping (~120 entries) | ISO 3166-1 alpha-2 display names | **Leave as-is** — reference data |
| 557-563 | Conflict topic keyword patterns (22 patterns) | `is_conflict_topic()` — severity escalation | **Configure** — operational tuning |
| 577-587 | Source reliability weights (8 sources) | `source_reliability()` — clustering weight | **Configure** |
| 603-608 | Critical severity escalation: `event_count >= 20`, `source_types.len() >= 3` | Active-phase critical escalation thresholds | **Configure** |
| 613 | High severity escalation: `event_count >= 10` | Developing/active high escalation threshold | **Configure** |
| 618 | Medium escalation: `source_types.len() >= 2` | Conflict topics multi-source medium threshold | **Configure** |
| 624 | `num_hours() > 48` | Stale cluster severity cap at 48h | **Configure** |
| 638 | `score: 0.2` (base certainty) | Base certainty score | **Leave as-is** — well-tuned |
| 641 | `extra_sources.min(3)`, `0.15` per source | Certainty: +0.15 per additional source, cap 3 | **Leave as-is** |
| 645-651 | Certainty: `event_count >= 5`, `>= 20` | Certainty bonus thresholds | **Leave as-is** |
| 669-673 | `is_generic_topic()` prefixes (8 prefixes) | Generic topic filter for clustering | **Configure** |
| 680-692 | `is_language_tag()` (50 languages) | Language tag filter | **Leave as-is** — comprehensive list |
| 773-777 | Gap tolerance base hours by severity | Critical=12, High=8, Medium=4, Low=2 | **Configure** |
| 781 | `peak_event_rate / 5.0`, clamp `0.0-3.0` | Activity factor for gap tolerance | **Configure** |
| 785 | `diversity / 2.0`, clamp `1.0-2.0` | Source diversity factor | **Configure** |
| 801 | Emerging->Developing: `event_count >= 3`, `source_diversity >= 2` | Phase transition thresholds | **Configure** |
| 802 | `max_severity_rank >= 3` | High/critical severity triggers transition | **Configure** |
| 813 | `hours_since_last_event > 6.0` | Emerging->Resolved stale threshold | **Configure** |
| 824-825 | Developing->Active: `event_velocity_5m >= 5`, 4-signal alignment | Phase transition thresholds | **Configure** |
| 853 | Active->Declining: `current_rate < peak_rate * 0.3` | Rate drop threshold (30% of peak) | **Configure** |
| 874 | Declining->Active: `current_rate > peak_rate * 0.7` | Rate recovery threshold (70% of peak) | **Configure** |
| 1123-1128 | `GENERIC_TOPICS` (14 topic strings) | Topics filtered from clustering | **Configure** |
| 1164 | `trimmed.len() >= 3` | Minimum topic tag length | **Leave as-is** |

### 5.2 pipeline.rs — Main Pipeline Logic

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| ~80 | `broadcast::channel(4096)` | Publish channel buffer size | **Configure** |
| ~95 | Embedding batch size `8` | Texts batched for embedding | **Configure** |
| ~100 | `EmbeddingCache::new(10_000)` | Embedding cache size | **Configure** |
| ~150 | `Duration::from_secs(60)` | Prune interval (60s) | **Already in config** (`sweep.prune_interval_secs`) |
| ~200 | `Duration::from_secs(300)` | Situations publish interval | **Already in config** (`interval.situations_publish_secs`) |
| ~300 | `query_backfill_events(pool, 6, 5000)` | Backfill: 6 hours, 5000 events max | **Configure** |
| various | Title regen signal count thresholds | Various title/narrative regen triggers | **Already in config** |

### 5.3 alerts.rs — Alert Engine

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 96 | `elapsed >= 30` | Situation-scoped dedup window (30 min) | **Configure** |
| 124 | `Duration::hours(2)` | Alert tracker cleanup cutoff | **Configure** |

### 5.4 window.rs — Correlation Window

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 172 | `111.0` km/degree | Latitude to km conversion constant | **Leave as-is** — physical constant |

### 5.5 rules/ — Correlation Rules

| File | Line | Value | Controls | Recommendation |
|------|------|-------|----------|----------------|
| infra_attack.rs | 33 | `Duration::from_secs(300)` | Time window (5 min) | **Configure** |
| infra_attack.rs | 50 | `shodan < 2 \|\| bgp < 3 \|\| outage < 2` | Minimum event counts | **Configure** |
| infra_attack.rs | 117 | `confidence: 0.75` | Rule confidence score | **Configure** |
| military_strike.rs | 33-34 | `Duration::from_secs(600)`, `radius = 100.0` | Time (10 min), radius (100 km) | **Configure** |
| military_strike.rs | 148 | `confidence: 0.7` | Rule confidence | **Configure** |
| confirmed_strike.rs | 33-34 | `Duration::from_secs(1800)`, `radius = 50.0` | Time (30 min), radius (50 km) | **Configure** |
| confirmed_strike.rs | 120 | `confidence: 0.85` | Rule confidence | **Configure** |
| coordinated_shutdown.rs | 33 | `Duration::from_secs(900)` | Time window (15 min) | **Configure** |
| coordinated_shutdown.rs | 48 | `bgp < 3 \|\| outage < 2 \|\| censorship < 2` | Minimum counts | **Configure** |
| coordinated_shutdown.rs | 113 | `confidence: 0.8` | Rule confidence | **Configure** |
| maritime_enforcement.rs | 33-34 | `Duration::from_secs(1800)`, `radius = 30.0` | Time (30 min), radius (30 km) | **Configure** |
| maritime_enforcement.rs | 105 | `confidence: 0.6` | Rule confidence | **Configure** |
| apt_staging.rs | 33 | `Duration::from_secs(3600)` | Time window (60 min) | **Configure** |
| apt_staging.rs | 134 | `confidence: 0.65` | Rule confidence | **Configure** |
| conflict_thermal.rs | 33-34 | `Duration::from_secs(6 * 3600)`, `radius = 50.0` | Time (6 hr), radius (50 km) | **Configure** |
| conflict_thermal.rs | 54 | `thermal < 3` | Min thermal anomalies | **Configure** |
| conflict_thermal.rs | 115 | `confidence: 0.8` | Rule confidence | **Configure** |
| gps_military.rs | 32 | `Duration::from_secs(1800)` | Time window (30 min) | **Configure** |
| gps_military.rs | 70 | `radius = 150.0` | Radius (150 km) | **Configure** |
| gps_military.rs | 188 | `confidence: 0.7` | Rule confidence | **Configure** |

---

## 6. sr-db

### 6.1 lib.rs — Connection Pool

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 8 | `.max_connections(20)` | PostgreSQL connection pool size | **Configure** — env var `DB_MAX_CONNECTIONS` |

### 6.2 queries.rs — SQL Queries

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 18 | `filter.limit.unwrap_or(100)` | Default event query limit | **Leave as-is** — API default |
| 67 | `params.limit.unwrap_or(1000)` | Default GeoJSON event limit | **Leave as-is** — API default |
| 79 | `limit: Some(50)` | Latest events endpoint limit | **Leave as-is** — API default |
| 534 | `hours: i32`, `limit: i64` | Backfill parameters (caller passes) | Caller-configurable |
| 546-554 | Backfill event type whitelist (16 types) | Types included in pipeline backfill | **Leave as-is** — structural |

---

## 7. sr-server

### 7.1 main.rs — Server Setup

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 40-41 | `"postgres://sitrep:<DB_PASSWORD>@localhost/situationreport"` | Default DATABASE_URL | **Already env-configurable** |
| 47 | `broadcast::channel(4096)` | Ingest broadcast buffer size | **Configure** |
| 237 | `Duration::from_secs(3600)` | Camera search cache TTL (1 hr) | **Configure** |
| 247 | `Duration::from_secs(15 * 60)` | Camera discovery interval (15 min) | **Configure** |
| 250 | `Duration::from_secs(120)` | Camera discovery initial delay (2 min) | **Leave as-is** |
| 273 | Grid cell rounding `lat * 2.0` | ~50km camera search grid | **Leave as-is** |
| 278 | `50.0` | Camera search radius (50 km) | **Configure** |
| 300 | `Duration::from_secs(2)` | Inter-camera-search delay | **Leave as-is** |
| 413 | `"0.0.0.0:3001"` | Default bind address | **Already env-configurable** via `BIND_ADDR` |

### 7.2 routes/analytics.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 62 | `Duration::hours(24)` | Default timeseries lookback | **Leave as-is** — API default |
| 90 | `LIMIT 500` | Max timeseries buckets returned | **Leave as-is** — API cap |
| 169 | `INTERVAL '7 days'` | Anomaly baseline window | **Leave as-is** — statistical requirement |
| 179 | `COALESCE(bl.baseline_mean, 10.0)` | Default baseline mean when no data | **Leave as-is** |
| 179 | `COALESCE(bl.baseline_stddev, 5.0)` | Default baseline stddev | **Leave as-is** |
| 194 | `> 2.0` | Z-score anomaly threshold | **Configure** |
| 196 | `LIMIT 50` | Max anomalies returned | **Leave as-is** |
| 350 | `Duration::from_secs(10)` | Initial baseline refresh delay | **Leave as-is** |
| 353 | `Duration::from_secs(3600)` | Baseline refresh interval (1 hr) | **Configure** |

### 7.3 routes/search.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 64 | `params.limit.unwrap_or(20).min(100)` | Search result limit (default 20, max 100) | **Leave as-is** — API cap |
| 67 | `Duration::days(7)` | Default search lookback | **Leave as-is** — API default |
| 165 | `LIMIT 20` | Similar events limit | **Leave as-is** — API cap |
| 163 | `INTERVAL '7 days'` | Similar events time range | **Leave as-is** |

### 7.4 routes/situations.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 56 | `.unwrap_or(5).min(20)` | Narrative limit (default 5, max 20) | **Leave as-is** |
| 78 | `.unwrap_or(50).min(200)` | Situation events limit (default 50, max 200) | **Leave as-is** |
| 98 | `.take(15)` | Max entity patterns in SQL query | **Leave as-is** |

### 7.5 routes/reports.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 41 | `.unwrap_or(20).min(100)` | Report list limit | **Leave as-is** |
| 44 | `Duration::days(7)` | Default lookback | **Leave as-is** |

### 7.6 routes/alerts.rs

| Line | Value | Controls | Recommendation |
|------|-------|----------|----------------|
| 98 | `cooldown_minutes.unwrap_or(30)` | Default alert cooldown | **Leave as-is** — user-configurable per rule |
| 99 | `max_per_hour.unwrap_or(10)` | Default max alerts per hour | **Leave as-is** — user-configurable per rule |
| 157 | `.unwrap_or(50).min(200)` | Alert history limit | **Leave as-is** |
| 160 | `Duration::hours(24)` | Default history lookback | **Leave as-is** |

---

## 8. Summary Statistics

| Category | Total Items Found | Already Configurable | Recommend Configure | Leave As-Is |
|----------|-------------------|---------------------|--------------------|-----------|
| Source poll intervals | ~23 | 23 (via DB source_config) | 0 | 0 |
| Source monitoring lists (queries, feeds, channels, countries, ASNs, prefixes) | ~25 | 0 | **25** | 0 |
| Source bounding boxes | ~10 | 0 | **10** | 0 |
| Source severity thresholds | ~8 | 0 | **8** | 0 |
| Source timing (timeouts, delays, lookback windows) | ~20 | 0 | **12** | 8 |
| Source dedup/buffer caps | ~6 | 0 | **6** | 0 |
| Source backoff parameters | ~6 | 0 | **6** | 0 |
| Intel API/model parameters | ~15 | 3 (env vars) | **10** | 2 |
| Intel budget/pricing | ~5 | 1 (daily cap env var) | **3** | 1 |
| Intel narrative thresholds | ~6 | 0 | **5** | 1 |
| Exa search limits | ~5 | 0 | **5** | 0 |
| Pipeline cluster lifecycle thresholds | ~15 | Partially (some in PipelineConfig) | **10** | 5 |
| Pipeline correlation rule parameters | ~24 | 0 | **24** | 0 |
| Pipeline alerts | ~2 | 0 | **2** | 0 |
| Server infrastructure | ~10 | 3 (env vars) | **4** | 3 |
| Entity graph thresholds | ~2 | 0 | **2** | 0 |
| Reference data (country maps, equipment codes, keywords) | ~15 | 0 | 0 | **15** |
| API response limits | ~12 | 0 | 0 | **12** |
| **TOTAL** | **~210** | **~30** | **~132** | **~47** |

---

## 9. Priority Recommendations

### Tier 1 — High-Value Configuration (do first)

These are frequently tuned and directly impact operational intelligence quality:

1. **Source monitoring lists** — `QUERIES`, `GEO_QUERIES`, `CONFLICTS`, `CHANNELS`, `FEEDS`, `SEARCH_QUERIES`, `COUNTRIES`, `MONITORED_ASNS`, `DOMAIN_PATTERNS`, `MILITARY_PREFIXES`, `HIGH_VALUE_TYPES`, `BOUNDING_BOXES`
   - **Recommendation:** A `SourcesConfig` struct (similar to `PipelineConfig`) loaded from env/JSON. Each source's list is a `Vec<String>` or `Vec<SourceEntry>` with reasonable defaults matching current values.

2. **Correlation rule parameters** — time windows, radii, minimum event counts, confidence scores for all 8 rules
   - **Recommendation:** A `RulesConfig` struct with per-rule sections, loaded from env. All rules already share a common interface via `CorrelationRule` trait.

3. **Severity thresholds** — GDELT tone thresholds, FIRMS FRP thresholds, USGS magnitude thresholds, GPSJam percentages, nuclear CPM thresholds
   - **Recommendation:** Include in the per-source config above. Each source's severity mapping becomes a configurable list of `(threshold, severity)` tuples.

4. **Intel model pricing** — `HAIKU_PRICING`, `SONNET_PRICING`, max_tokens, Sonnet budget threshold
   - **Recommendation:** An `IntelConfig` struct loaded from env. Prices change with model releases and currently require code changes.

### Tier 2 — Medium-Value Configuration

5. **Backoff parameters** — registry base/max/exponent for error and 429 backoff, stream reconnect delays
   - **Recommendation:** Centralize in a `BackoffConfig` section (the pipeline already has one — extend it to source-level).

6. **Lookback windows** — OTX 2h, OONI 2h, nuclear 24h, GFW 24h, IODA 20min, Cloudflare 7d, Telegram 12h backfill
   - **Recommendation:** Include as fields in per-source config.

7. **Dedup buffer caps** — FIRMS 50K, USGS 10K, nuclear 50K, RSS 5K
   - **Recommendation:** Include as field in per-source config.

8. **Pipeline cluster lifecycle thresholds** — gap tolerance bases, phase transition signal counts, severity escalation thresholds, conflict topic keywords
   - **Recommendation:** Extend existing `PipelineConfig` with `PhaseTransitionConfig` and `SeverityEscalationConfig` sections.

### Tier 3 — Low Priority / Leave As-Is

9. **API response limits** (20, 50, 100, 200, 500) — these are sensible server defaults
10. **Reference data** (country codes, equipment categories, language tags) — static, rarely changes
11. **Prompt engineering values** (word limits, .take() counts) — these are carefully tuned for token budget

### Suggested Architecture

```
SourcesConfig (new)
├── gdelt: { queries: [...], max_records: 250, timeout_secs: 15, tone_thresholds: [...] }
├── gdelt_geo: { queries: [...], max_points: 250, ... }
├── geoconfirmed: { conflicts: [...], page_size: 50 }
├── opensky: { bounding_boxes: [...], military_prefixes: [...] }
├── adsb: { high_value_types: [...], point_queries: [...], min_gap_ms: 1500 }
├── ais: { bounding_boxes: [...], military_mmsi_prefixes: [...] }
├── firms: { bounding_boxes: [...], frp_thresholds: [...] }
├── usgs: { bounding_boxes: [...], magnitude_thresholds: [...], depth_threshold: 5.0 }
├── notam: { critical_firs: [...], priority_qcodes: [...], conflict_qcodes: [...] }
├── shodan: { ics_ports: [...] }
├── cloudflare: { countries: [...], per_cycle: 2, date_range: "7d" }
├── ioda: { countries: [...], lookback_mins: 20 }
├── bgp: { monitored_asns: [...], dedup_window_secs: 300 }
├── otx: { search_queries: [...], lookback_hours: 2, limits: { subscribed: 50, search: 20 } }
├── certstream: { domain_patterns: [...] }
├── ooni: { countries: [...], lookback_hours: 2, limit: 100 }
├── nuclear: { regions: [...], baseline_cpm: 50.0, alert_cpm: 100.0 }
├── gfw: { bounding_boxes: [...], page_limit: 100 }
├── gpsjam: { min_threshold: 10.0, severity_thresholds: [...] }
├── telegram: { channels: [...], critical_keywords: [...], high_keywords: [...] }
├── rss: { feeds: [...], feeds_per_poll: 3, request_timeout_secs: 10 }
└── backoff: { base_secs: 30, max_secs: 300, exponent_cap: 4, rate_limit_additive: 30 }

IntelConfig (new)
├── haiku_pricing: { input: 1.0, output: 5.0, cache: 0.10 }
├── sonnet_pricing: { input: 3.0, output: 15.0, cache: 0.30 }
├── enrichment_max_tokens: 1024
├── analysis_max_tokens: 8192
├── narrative_max_tokens: 1500
├── title_max_tokens_ollama: 2048
├── title_max_tokens_claude: 40
├── sonnet_budget_threshold: 0.8
├── tempo_thresholds: { high: 20.0, elevated: 5.0 }
├── analysis_intervals: { high: 900, elevated: 3600, normal: 7200 }
├── narrative_regen: { min_events: 30, timeout_mins: 120, timeout_min_events: 10 }
├── sonnet_escalation_sources: 3
├── api_timeout_secs: 120
├── max_retries: 4
├── retry_base_ms: 500
├── ollama_timeout_secs: 120
├── ollama_gpu_concurrency: 1
└── ollama_ctx_sizes: { enrichment: 4096, narrative: 8192, analysis: 8192, merge: 2048, min: 2048 }

RulesConfig (new)
├── infra_attack: { window_secs: 300, min_shodan: 2, min_bgp: 3, min_outage: 2, confidence: 0.75 }
├── military_strike: { window_secs: 600, radius_km: 100, confidence: 0.7 }
├── confirmed_strike: { window_secs: 1800, radius_km: 50, confidence: 0.85 }
├── coordinated_shutdown: { window_secs: 900, min_bgp: 3, min_outage: 2, min_censorship: 2, confidence: 0.8 }
├── maritime_enforcement: { window_secs: 1800, radius_km: 30, confidence: 0.6 }
├── apt_staging: { window_secs: 3600, confidence: 0.65 }
├── conflict_thermal: { window_secs: 21600, radius_km: 50, min_thermal: 3, confidence: 0.8 }
└── gps_military: { window_secs: 1800, radius_km: 150, confidence: 0.7 }

PipelineConfig (extend existing)
├── ... (existing scoring, merge, cluster_caps, phase, etc.)
├── cluster_lifecycle:
│   ├── gap_tolerance_base: { critical: 12.0, high: 8.0, medium: 4.0, low: 2.0 }
│   ├── activity_factor_divisor: 5.0
│   ├── activity_factor_max: 3.0
│   ├── source_factor_divisor: 2.0
│   ├── severity_escalation: { critical_events: 20, critical_sources: 3, high_events: 10, medium_sources: 2 }
│   └── stale_severity_cap_hours: 48
├── phase_transitions:
│   ├── emerging_to_developing: { min_events: 3, min_sources: 2, or_severity: 3 }
│   ├── emerging_stale_hours: 6.0
│   ├── developing_to_active: { min_velocity_5m: 5, signal_count: 3 }
│   ├── active_rate_drop: 0.3
│   └── declining_recovery: 0.7
├── publish_channel_size: 4096
├── ingest_channel_size: 4096
├── backfill: { hours: 6, max_events: 5000 }
├── embedding_batch_size: 8
├── embedding_cache_size: 10000
├── alert_situation_dedup_mins: 30
├── alert_cleanup_hours: 2
├── conflict_topic_keywords: [...]
├── source_reliability_weights: { geoconfirmed: 0.95, acled: 0.90, ... }
└── generic_topic_prefixes: [...]

ServerConfig (extend existing)
├── db_max_connections: 20
├── camera_search_ttl_secs: 3600
├── camera_search_interval_secs: 900
├── camera_search_radius_km: 50.0
├── anomaly_z_threshold: 2.0
└── baseline_refresh_interval_secs: 3600

SearchConfig (extend existing from sr-intel)
├── max_articles_per_cluster: 10
├── exa_daily_cap: 1400
├── exa_hourly_cap: 60
├── exa_cooldown_secs: 30
└── exa_cost_per_request: 0.007
```

This architecture would bring the total externalized configuration from ~30 items to ~160+ items, covering all operationally significant magic numbers while leaving stable reference data and API response caps as compile-time constants.
