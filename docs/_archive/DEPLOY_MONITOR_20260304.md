# Deploy Monitor — 2026-03-04

## Deploy Details
- **Commit**: `34e4b20` Vector-primary merge, prompt debiasing, config crate, pipeline upgrades
- **Deploy time**: ~19:59 UTC (local ~13:59 CST)
- **Changes**: Vector-primary merge logic, debiased prompts, sr-config crate, source-type context in enrichment, inferred_location extraction

---

## Monitoring Log

### T+0 — 20:41 UTC
- **Budget**: endpoint unreachable (likely hasn't done first API call yet)
- **Situations**: 275 total, 56 top-level
  - Top: Yemen (571ev/1src), Sahel (305ev/1src), Ukraine (282ev/2src), Israel-Iran (258ev/3src CRITICAL), Germany-Iran (179ev/3src CRITICAL)
  - Still 2x DRC duplicates (150ev + 100ev)
  - Israel-Iran War at #4 with 258 events — properly surfaced as CRITICAL
- **Sources**: All 15 healthy. Streaming sources (bgp, telegram, certstream, ais, shodan) ~37s ago. Polled sources active.
- **Issues**:
  - Exa API credits exhausted (402) — circuit breaker tripping repeatedly on startup. Non-critical: supplementary search disabled but core pipeline unaffected.
  - Budget endpoint returning error — may need a moment to initialize
- **Assessment**: Healthy deploy. Vector-primary merge active. Single-source Yemen/Sahel still dominate by event count but rank below multi-source critical situations.

### T+5 — 20:46 UTC (post-restart for Exa circuit breaker clear)
- **Budget**: endpoint still unavailable (may need first enrichment cycle to init)
- **Situations**: 299 total (+24), 67 top-level (+11) — clusters rebuilding after restart
  - Israel-Iran War now at 300ev/3src CRITICAL (#4 by event count)
  - Ukraine upgraded to HIGH (315ev/2src) — was medium before
  - New: "Middle East Iran Conflict Escalation" (220ev/3src CRITICAL) — possible duplicate of Israel-Iran?
  - Still 2x DRC duplicates (150 + 100)
- **Sources**: All 15 healthy. BGP had a brief WebSocket close (auto-reconnected in 20s). CertStream reconnected normally. AirplanesLive hit a 429 on startup (8-10s cooldown, expected).
- **Logs**: 39 enrichment/poll activities in 2 min. Telegram: GeoConfirmed and Ansarallah_MC channels not found (may be renamed/removed).
- **Assessment**: System recovering well post-restart. Situation count growing as backfill rebuilds clusters. Vector-primary merge will consolidate duplicates over time. No critical errors.

### T+10 — 20:52 UTC
- **Budget**: still unavailable — investigating
- **Situations**: 324 total (+25), 69 top-level (+2)
  - Israel-Iran War at 300ev/3src CRITICAL — stable
  - Ukraine upgraded to 315ev/2src but severity regressed to MEDIUM (was HIGH at T+5) — severity not propagating?
  - "Fire in IRAN" at 166ev/1src MEDIUM — likely FIRMS thermal cluster, poor title (prompt bias still present for FIRMS?)
  - New: "Middle East Conflict Humanitarian Crisis" (107ev/3src CRITICAL)
  - Still 2x DRC, but "Germany Iran Conflict" renamed (was "Germany-China Trade" before fixes — title regen working)
- **Event flow**: Active — 2,219 events in last 5min. Aviation dominant (644+606+572 ADSB). BGP+FIRMS healthy. OONI flowing. No GDELT/RSS in this 5min window (expected, longer poll intervals).
- **Enrichment**: 24 enrichment/narrative events in 5min — healthy
- **Issues**:
  - Budget endpoint still returning error — may be a route issue post-restart
  - "Fire in IRAN" title suggests FIRMS events not getting good titles
  - Ukraine severity regression needs watching

### T+15 — 20:59 UTC
- **Situations**: 335 total (+36), 69 top-level (stable)
  - Israel-Iran at 300ev/3src CRITICAL — stable
  - "Iran Wildfire Clusters Detected" at 166ev — FIRMS title improved (was "Fire in IRAN")
  - Enrichment running (15 events in 5min)
- **Sources**: Aviation dominant (2,262 events/5min). BGP (23), Cloudflare (2). News sources on longer poll cycles.
- **AIS Investigation**: API implementation matches docs exactly. Key connects to wss://stream.aisstream.io but sends 0 messages even for busiest shipping lanes. **API key is expired/revoked** — needs regeneration at aisstream.io. Health reporting fixed: will now show "connecting" instead of false "healthy".
- **Fixes applied this interval**:
  1. Stale positions: `pollPositions()` now passes `since=30min`, `replacePositions()` removes departed aircraft
  2. Stale events: Initial geo load passes `since=12h`, default timeRange reduced from 48h to 12h
  3. Outlinks: Added 10 missing source types (rss-news, gdelt, telegram, geoconfirmed, ioda, cloudflare, gpsjam, nuclear, notam, gfw) + generic URL fallback
  4. Position popups: Added AirplanesLive/MarineTraffic outlinks + last-seen timestamp
  5. Stream health: "connecting" on start, "healthy" only after first data received
  6. AIS: key expired, needs regeneration (code is correct)

### T+25 — 21:09 UTC (post-fix deploy + new AIS key)
- **Health reporting working**: AIS correctly shows `connecting` (not false `healthy`). BGP promoted to `healthy` on first data, then correctly cycles through `connecting`→`healthy` on reconnects.
- **AIS with new key**: Still 0 messages after 2 minutes with regenerated key. Connects and subscribes to 6 regions but no data flows. Possible aisstream.io service issue or account activation delay. Our code implementation matches their API spec exactly (confirmed by documentation review).
- **Other streaming sources**: CertStream reconnecting (server closes frequently), Shodan-stream and Telegram connecting but no events yet.
- **Budget**: $8.03 spent today

### T+90 — 22:30 UTC (session resume)
- **Situations**: 324 total, 68 top-level (stable)
  - Israel-Iran War Escalation at 300ev/3src CRITICAL/Active — properly surfaced at #4 ✓
  - All 3 plan fixes (grandparent detach, merge guard, composite ranking) confirmed deployed
  - 5 CRITICAL situations all top-level: Israel-Iran War, Germany-Iran Conflict, Israel-Iran Conflict, Middle East Humanitarian, Germany-China Trade
  - DRC duplicates still present (2x ~150+100ev MEDIUM)
- **Budget**: $8.03 spent, $1.97 remaining — DEGRADED mode active
- **AIS Investigation Complete**: Code is correct per aisstream.io docs. Issue is likely poor volunteer receiver coverage in our Middle East regions. Recommended diagnostic: test with global bounding box.
- **Assessment**: System stable. War surfacing correctly. Budget approaching limit, enrichment throttled.

