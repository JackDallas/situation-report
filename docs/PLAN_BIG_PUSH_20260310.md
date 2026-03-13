# Big Push Plan — March 10, 2026

Comprehensive plan based on 10 agent deep dives covering pipeline stability, AI costs,
data quality, source health, UI bugs, and new capabilities.

## Phase 0: Emergency Fixes (unblock the pipeline)

These are the reason the dashboard shows 3 situations during an active war.

### 0.1 Topic Diversity Split Threshold
**File**: `config/src/lib.rs` line 695
**Change**: `topic_diversity_split_threshold: 8` → `25`
**Why**: Threshold of 8 is below the topic prune threshold (15) and max (30).
Every meaningful cluster hits 8 topics within 3-4 events, gets split into
empty-metadata children that fail quality gates. Death spiral.
**Also**: Add env var override `PIPELINE_SWEEP_TOPIC_DIVERSITY_THRESHOLD`

### 0.2 Ollama Health Check — Stop Blocking Pipeline
**File**: `pipeline/src/pipeline.rs` (health check in tokio::select! loop)
**Change**: Move `health_check()` + `warm_model()` to `tokio::spawn()` background task.
Use `Arc<AtomicU32>` for failure counter shared with spawned task.
**Why**: Currently blocks the entire pipeline loop for 20-60s during model re-warm,
causing "Pipeline lagged, skipped N events" on every health check failure.
**Also**: Increase health check timeout 5s→15s, interval 5min→10min.
Make health check use `/api/tags` instead of `/api/generate` to avoid GPU contention.

### 0.3 Ollama Docker Config
**File**: `docker-compose.yml`
**Changes**:
- Add `OLLAMA_FLASH_ATTENTION: "1"` (reduces VRAM ~20-30%)
- Add `OLLAMA_NUM_PARALLEL: "1"` (explicit single-request mode)
**Why**: GPU memory pressure between BGE-M3 ONNX + Qwen 3.5 9B causes model unloads.

### 0.4 Deploy Script Fix
**File**: `deploy.sh`
**Change**: Add `--exclude='replay-data'` to rsync (2.7GB stalling Tailscale deploys)

### 0.5 Disable Dead/Broken Sources
- CertStream: `enabled: false` (80 fails, marginal OSINT value, internal retry bypasses health tracking)
- Shodan Stream: `enabled: false` if no paid streaming plan (80 fails)
- Rename "UKMTO" → "ASAM (Historical)" (source is dead since June 2024, misleading name)

---

## Phase 1: Data Quality & UI Fixes

### 1.1 Fix Aircraft Trails (3 bugs, 1 unified fix)
**File**: `frontend/src/lib/stores/map.svelte.ts`
**Change**: Remove `this.appendTrail(nextHistory, entry)` call from `replacePositions()` (line 284).
Trails should only come from `loadEntityTrail()` API calls.
**Fixes**: Ghost replay paths, backwards trails, click-to-hide behavior.

**File**: `frontend/src/lib/components/panels/PositionDetailPane.svelte`
**Change**: Remove "Load 2h trail" button (lines 46, 61-69, 467-480). Redundant since
MapPanel line 1016 already auto-loads trail on aircraft click.

### 1.2 Thermal Scaling (FRP-based)
**Backend**: Promote `frp` from payload to GeoJSON feature property for `thermal_anomaly` events.
**Frontend** (`MapPanel.svelte` thermal-dots layer):
- Color: log-scale gradient — yellow (#fbbf24) → orange (#f97316) → red (#ef4444) → bright red (#dc2626)
- Size: multiply base radius by 1.0x (0-5MW) / 1.3x (5-25MW) / 1.8x (25-100MW) / 3.5x (100+MW)
- Opacity: modulate base opacity by FRP intensity (0.5 → 0.95)
- Formula: `normalized = log10(frp + 1) / log10(600)`

### 1.3 Fix events_24h Bug
**File**: `server/src/routes/sources.rs`
**Change**: Compute `total_events_24h` live in the API query via JOIN against events table.
The column in `source_health` is never written to — it's been 0 since day one.

### 1.4 Position Staleness
**Backend**: Add periodic cleanup job: `DELETE FROM latest_positions WHERE last_seen < NOW() - INTERVAL '1 hour'`
**Frontend** (`sse.ts`): Tighten `since` window from 30min to 10min for aircraft.
**Frontend** (`map.svelte.ts`): Filter or dim aircraft with null heading + null speed (stationary/stale).

### 1.5 Aircraft Identification (modes.csv)
**Data**: Load Bellingcat's modes.csv (495K ICAO hex → category/military/owner) into HashMap at startup.
**Integration point**: During ADS-B source ingestion (adsb.rs), enrich each event with:
- `tags: ["military:true", "category:fighter"]`
- payload: `{ "operator": "Royal Air Force", "registration": "ZZ504" }`
**Fixes**: UK/France dual-tagging bug (hex 43c61d correctly maps to UK/RAF, not French)
**Update**: Periodic refresh from tar1090-db GitHub repo.
**Cleanup**: ~38K rows with misaligned columns need a sanitization pass on load.

---

## Phase 2: AI Provider Migration (Gemini-only)

Goal: Eliminate Anthropic API dependency. Single API key: `GEMINI_API_KEY`.
Hard budget: **$30/calendar month** (invoice billing). Target: $0/day (Ollama) to ~$1/day (Gemini fallback).

### 2.1 Stabilize Ollama (Phase 0.2 + 0.3)
Most important — if Ollama works reliably, cloud API cost is $0.

### 2.2 Add Gemini Client (Native REST API)
**File**: New `intel/src/gemini.rs` (~200 lines, reqwest + serde)
**API**: Native Gemini REST — `POST generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
**Auth**: `x-goog-api-key` header. Single env var `GEMINI_API_KEY` (stored in 1Password: OSINT/GCPVertex/credential).
**Models**:
- **`gemini-2.5-flash-lite`** — enrichment, titles (replacing Haiku). $0.10/M in, $0.40/M out.
- **`gemini-2.5-flash`** — narratives, analysis (replacing Sonnet). $0.30/M in, $2.50/M out.
**Why native over OpenAI-compat shim**:
- `responseSchema` — full JSON schema enforcement at decode level (critical for structured enrichment output)
- Implicit caching — 90% input cost reduction on repeated system prompts (>1024 tokens, automatic)
- `countTokens` endpoint — free, 3000 RPM, accurate budget tracking
- Safety settings — per-request control
- Better structured output support than the shim's limited `json_schema`
**Estimated cost**: ~$0.80/day (Flash-Lite enrichment/titles ~$0.20 + Flash narratives ~$0.60). Well within $30/mo.

### 2.3 Monthly Budget Enforcement
**File**: `intel/src/budget.rs` (extend existing BudgetManager)
**Changes**:
- New `budget_monthly` table or column tracking cumulative Gemini spend per calendar month
- Hard cap at $30/month — once hit, Gemini calls return `Err(BudgetExhausted)`, system runs Ollama-only
- Daily budget stays ($10/day) as inner guard; monthly cap is the hard outer limit
- On first of each month, monthly counter resets
- API endpoint: `GET /api/intel/budget` extended with `gemini_spent_month_usd`, `gemini_month_limit_usd`
- Dashboard: show monthly spend alongside daily

### 2.4 Update Fallback Chain
**Current**: Ollama → Claude (Haiku/Sonnet)
**New**: Ollama → Gemini 2.5 Flash-Lite (enrichment/titles) → Gemini 2.5 Flash (narratives/analysis)
Claude removed from fallback chain entirely. No ANTHROPIC_API_KEY needed.
**Context caching**: System prompts for enrichment (~1200 tokens) will automatically benefit from
implicit caching (90% input discount). No explicit cache management needed — Google detects shared
prefixes across requests.

### 2.5 Remove Claude Hard Dependency
Make ANTHROPIC_API_KEY fully optional. System runs with Ollama + Gemini only.
Remove `sr-intel` Claude client from default feature set (keep behind `claude` feature flag for emergency use).

### 2.6 Token Counting for Budget Accuracy
Use Gemini's free `countTokens` endpoint (3000 RPM) to track actual token usage per call.
Budget manager records exact cost per request instead of estimating from string length.
Monthly budget dashboard: `gemini_spent_month_usd` / `gemini_month_limit_usd` ($30).

---

## Phase 3: Pipeline Quality

### 3.1 Make Merge Thresholds Configurable
Move ~25 hardcoded numbers from merge.rs to MergeConfig:
- Title identity merge (0.60), zero-entity guards (0.75/0.40)
- Low-content guards (0.80/0.40), regional absorb sizes (20/50)
- Heuristic thresholds (0.50, 2, 1), grandparent guard (3)
- Merge rejection expiry (1h), topical orphan Jaccard (0.15)

### 3.2 Make Severity Thresholds Configurable
New SeverityConfig struct for the recompute_cluster_severity function:
- Critical: events >= 20, sources >= 3 (currently hardcoded)
- High: events >= 10
- Medium: sources >= 2, events >= 5

### 3.3 Fix Split Child Metadata
**File**: `merge.rs` split_by_coherence()
**Change**: Populate child cluster's topics/entities from its split events (currently empty).
Reference: split_divergent() already does this correctly. split_by_coherence() should follow
the same pattern — iterate child's event_ids, extract topics/entities from source events.

### 3.4 DTO Quality Gate Improvements
- Make ND caps adaptive: `max(2, total_top_level / 6)` instead of fixed 2/3
- Make "incoherent" threshold (12 topics) configurable
- Make Medium standalone penalty (+8 events) configurable

### 3.5 Source Health Improvements
- On startup, check DB `consecutive_failures` — skip sources with >50 fails + auth error
- Add SSE event for source health transitions (healthy → error)
- Fix CertStream's internal retry bypassing registry health tracking

---

## Phase 4: New Capabilities

### 4.1 Military Aircraft Surge Correlation Rule
**Requires**: modes.csv integration (Phase 1.5)
**Rule**: Alert when count of distinct military hex codes in a region within 6h exceeds
rolling 7-day baseline by 2+ standard deviations.
**Categories of interest**: fighter, tanker, reconnaissance, electronic_warfare, UAV

### 4.2 Transit Pattern Detection
**Inspired by**: Turnstone's dual-ROI pattern
**Rule**: Define monitored corridors (pairs of bounding boxes with max transit time).
Alert when military aircraft transit between endpoints.
**Examples**: Incirlik → Baghdad, Ramstein → Rzeszow, Akrotiri → Syria

### 4.3 IMB Piracy Source
**Data**: ICC-CCS piracy map WordPress REST API at icc-ccs.org
**Events**: Geolocated piracy incidents (Attack, Boarding, Hijack)
**Replaces**: Dead NGA ASAM source for maritime security alerts

### 4.4 Historical ADS-B Baseline
Compute rolling baselines per region: avg military aircraft/day, unique hex codes/day.
Alert on >2 sigma deviations. Uses the same modes.csv enrichment from Phase 1.5.

---

## Phase 5: Research / Future (no immediate implementation)

### 5.1 Adaptive Merge Thresholds
Percentile-based: track running histogram of candidate cosine similarities,
set threshold at 85th percentile. Ref: ARD-Stream algorithm.

### 5.2 Per-Cluster Adaptive Radii
FISHDBC/DenStream-style: each cluster maintains its own merge radius based on
internal density. Tight clusters (single event) have tight radius, broad phenomena
(migrant crisis) have loose radius.

### 5.3 LLM-Enhanced Cluster Centroids
Use narrative text as cluster embedding representative instead of EWMA centroid
of individual event embeddings. More stable coherence measurements.

### 5.4 llama-cpp-rs Integration
Replace Ollama Docker container with direct llama.cpp Rust bindings.
Eliminates HTTP overhead, model lifecycle management, Docker boundary.
GBNF grammar support for structured JSON. Medium-term project.

### 5.5 Source Failure Alerting
Webhook/ntfy.sh notifications when sources transition to error state.
Frontend toast/banner for degraded source health.

---

## Execution Order

| Order | Item | Effort | Impact |
|-------|------|--------|--------|
| 1 | 0.1 Topic diversity threshold | 5 min | Critical — unblocks pipeline |
| 2 | 0.2 Ollama health check background | 1 hr | Critical — stops event skipping |
| 3 | 0.3 Ollama docker config | 5 min | High — reduces GPU pressure |
| 4 | 0.4 Deploy script fix | 5 min | High — unblocks Tailscale deploys |
| 5 | 0.5 Disable dead sources | 10 min | Medium — reduces noise |
| 6 | 1.1 Fix trails | 30 min | High — visible UI fix |
| 7 | 1.2 Thermal scaling | 1 hr | Medium — better visual intel |
| 8 | 1.3 Fix events_24h | 30 min | Medium — source monitoring |
| 9 | 1.4 Position staleness | 30 min | Medium — fixes plane hovering |
| 10 | 1.5 Aircraft identification | 2 hr | High — fixes tagging, enables Phase 4 |
| 11 | 2.2-2.5 Gemini migration + monthly budget | 4 hr | High — eliminates Anthropic dependency, $30/mo cap |
| 12 | 3.1-3.2 Configurable thresholds | 2 hr | Medium — enables tuning |
| 13 | 3.3 Split child metadata | 1 hr | Medium — prevents invisible fragments |
| 14 | 4.1-4.2 Military correlation rules | 4 hr | High — new capability |
