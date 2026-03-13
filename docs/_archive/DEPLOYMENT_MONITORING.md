# Deployment Monitoring Log — 2026-03-02

Fresh deploy at ~22:06 UTC. Clean DB, all volumes wiped.

## Check 1 (T+2 min, ~22:08 UTC)
- Budget: $0.00 — Qwen handling all narratives
- 18 situations (3 top-level, 15 children), 1,392 events
- All medium severity, 17 emerging / 1 developing
- All single-source (geoconfirmed only)
- Largest cluster: 286 events "Myanmar Missile Strikes" — backfill artifact, size penalty holding
- No errors

## Check 2 (T+7 min, ~22:13 UTC)
- Budget: $0.00 still
- Stable at 18 situations, 1,392 events
- 21 narratives generated in 5min window (all Qwen)
- CertStream reconnecting frequently (upstream flaky, ~5x in 5min)
- AirplanesLive 429 rate-limited, backoff working (40s)
- GDELT GEO had 1 request failure

## Check 3 (T+12 min, ~22:18 UTC)
- Budget: $0.42 (Haiku enrichment kicked in, 204k tokens, 0 Sonnet)
- 182 situations (5 top-level, 177 children), 2,160 events
- Severity diversified: 133 high, 18 medium, 31 low
- Phases working: 48 emerging, 56 developing, 78 declining
- Source types in situations: geoconfirmed, notam, rss-news
- Still 0 multi-source situations — no cross-source linking yet
- 22 narratives in 5min, all Qwen
- AirplanesLive stuck at 160s backoff (5 consecutive 429s)
- 23/24 sources healthy

## Check 4 (T+17 min, ~22:23 UTC)
- Budget: $0.65 (Haiku 311k tokens, 0 Sonnet)
- 269 situations (5 top-level, 264 children), 2,731 events
- Severity: 197 high, 34 medium, 38 low
- Phases: 48 emerging, 60 developing, 161 declining
- Source types: geoconfirmed, notam, rss-news
- Still 0 multi-source
- 134 singleton RSS news clusters with empty titles — half of all situations
- 28 narratives in 5min
- Ollama merge audit failed 1x
- AirplanesLive: NEVER successfully polled — 429 from first attempt. Works from other IPs (200). Desktop IP likely rate-limited from previous deployments.

## Check 5 (T+22 min, ~22:28 UTC)
- Budget: $2.03 (Haiku 897k, Sonnet 24k tokens) — first Sonnet spend!
- 514 situations (8 top-level, 506 children, 126 singletons), 4,909 events
- Severity: 274 high, 186 medium, 54 low
- Phases: 61 emerging, 62 developing, 391 declining — lifecycle working
- Source types: gdelt, geoconfirmed, notam, rss-news — GDELT appeared!
- **154 multi-source situations** — cross-source linking finally working
- Merge audit active: 5 bad merges rejected (South Korea baseball, etc.)
- NEW mega-cluster: 490 events "African-diplomatic-response: Alexey Kochnev..."
- Entity contamination: "Alexey Kochnev, Andrey Manturov" appearing in unrelated titles
- Ollama merge audits failing — Ollama may be down
- RSS feeds 403: breakingdefense, warontherocks

## Check 6 (T+27 min, ~22:33 UTC)
- Budget: $2.24 (+$0.21 since fix) — Ollama handling ALL narratives now
- Sonnet tokens unchanged at 23,820 — no new Sonnet spend, fix confirmed
- 643 situations (9 top-level, 634 children, 131 singletons), 6,781 events
- Severity: 349 high, 233 medium, 61 low
- Phases: 61 emerging, 61 developing, 521 declining
- Source types: gdelt, geoconfirmed, notam, rss-news
- 249 multi-source situations (up from 154) — cross-source linking accelerating
- Merge audits working: correctly rejecting Taiwan, ransomware, AI agent merges
- NEW mega-cluster: 798 events "Iran and US Military Conflict" — size penalty not gating GDELT bulk
- AirplanesLive: 9 consecutive 429s, 160s backoff

## Check 8 (T+46 min, ~22:53 UTC)
- Runtime: 46 min
- Budget: $2.60 (unchanged — Qwen handling narratives)
- 792 situations (17 top, 775 children, 143 singletons), 13,044 events
- Severity: 349 high, 368 medium, 75 low
- Phases: 83 emerging, 77 developing, 632 declining
- Largest: 1,533 "Sahel Military Vehicle Movements" (still growing)
- 256 multi-source
- **Periodic analysis BROKEN**: 3/3 attempts failed with JSON truncation (EOF while parsing). 68k Sonnet tokens wasted, zero intel briefs generated. Output exceeds 4096 max_tokens with 792 situations.
- **Ollama confirmed fixed**: model warm, 100% GPU, 24h keepalive

## Check 9 (T+55 min, ~23:01 UTC)
- Runtime: 55 min. Budget: $2.80. 878 situations, 14,870 events, 261 multi-source
- Total narratives: 263, total polls: 47
- Iran title drifted again: "...Escalates Across Middle East" (was "Nuclear Tensions")
- Top 5: Sahel (1,938), Iran (806), Syria (500), Ukraine (467), Myanmar (413)

## Check 10 (T+60 min, ~23:06 UTC) — 1 HOUR MARK
- Runtime: 1 hour. Budget: $2.80 (FLAT — no new spend)
- 993 situations (19 top, 974 children, 139 singletons), 16,872 events
- Severity: 350 high, 571 medium, 72 low
- Phases: 82 emerging, 77 developing, 834 declining
- Total narratives: 301, total polls: 51
- Sahel mega-cluster at 2,424 events and climbing ~100/min
- Multi-source plateaued at 261
- AirplanesLive: 15 consecutive 429s, still dead

### 1-Hour Summary
- **Budget**: $2.80 total, stabilized. Projected ~$3-4/day at current rate. Sustainable.
- **Growth**: 993 situations, 16.9k events from 4 source types (gdelt, geoconfirmed, notam, rss-news)
- **Missing sources**: ACLED, FIRMS, cyber (shodan/bgp/cert), maritime (AIS/GFW), aviation (airplaneslive) — all polling but not creating situations
- **Mega-clusters**: Size penalty ineffective. Sahel (2,424), Iran (812), Syria (500) all far above 80-event target cap
- **Analysis**: Broken — 3/3 Sonnet analysis attempts failed (JSON truncation). No intel brief available.
- **Quality**: Topic gravity well causing 15% noise in Iran situation. Title drift on parents.
- **Ollama**: Fixed and stable. All narratives via Qwen.

### Iran Deep Dive (378 sub-situations analyzed)
- Quality: ~65-70% relevant. 303 Iran-related, 58 completely unrelated (15%)
- Root cause: **topic gravity well** — 364/378 children share exact same 3 topics
- Merge condition `topic_jaccard >= 0.5 && shared_topics >= 3` fires at 1.0 because 3/3 match
- Parent absorbs all child entities (98 total including "cologne carnival"), broadening gravity well
- 92% of children linked through topic overlap ONLY — no entity or geo overlap
- Fix priority: require entity overlap for news merges, apply topic IDF to merge scoring

## Check 7 (T+39 min, ~22:45 UTC)
- Budget: $2.60 (+$0.36), Haiku 1.11M, Sonnet 68k tokens
- 718 situations (17 top-level, 701 children, 142 singletons), 12,293 events
- Severity: 344 high, 298 medium, 76 low
- Phases: 84 emerging, 73 developing, 561 declining
- Source types: gdelt, geoconfirmed, notam, rss-news (unchanged)
- 256 multi-source (+7)
- NEW mega-cluster: 1,128 events "Sahel Region Military Vehicle Movements"
- Iran at 800, Syria at 500 — size penalty not gating bulk ingest
- 28 narratives/5min, all Qwen
- AirplanesLive: 11 consecutive 429s

## Issues Noted

### FIR Ambiguity (NOTAM vs Forestry)
- "FIR" (Flight Information Region) being misinterpreted in enrichment as fire/forestry
- Multiple situations created about trees/forests from NOTAM FIR data
- RSS news may also be pulling forestry articles matching "FIR" keyword
- **Fix ideas**:
  - Add source_type context to enrichment prompt so Haiku knows NOTAM FIR = aviation
  - RSS feed negative keyword filtering for forestry noise
  - Entity disambiguation by source context

### No Cross-Source Linking
- After 12 minutes, still 0 multi-source situations
- May be normal early — ACLED/GDELT have longer poll intervals
- Correlation rules need overlapping entities/regions from multiple sources to fire
- Watch for this to improve as more source types contribute events

### AirplanesLive Blocked
- 429'd from very first poll at startup — zero successful polls ever
- API works fine from other IPs (200 from local machine)
- Desktop server IP likely rate-limited from previous deployment sessions
- Backoff capped at 160s, keeps retrying — may eventually recover
- Consider: longer base interval, or check if API requires key/registration now

### RSS News Singleton Explosion
- 134 situations with 1 event each and empty titles (as of T+17)
- RSS news articles creating individual clusters instead of merging
- May indicate: entity overlap too low for merge, or enrichment not extracting enough linking entities
- Watch: do these eventually merge as more events arrive, or do they stay as singletons?

### Ollama Model Unloading (Fixed)
- Model was in "Stopping..." state — keepalive not being respected despite env var
- Caused narrative/audit requests to timeout → fall through to Sonnet ($2+ burned)
- Fixed: `docker restart ollama` + `ollama run qwen3.5:9b --keepalive 24h`
- Model now loaded: 100% GPU, 32k context, 24h keepalive
- **BACKLOG**: Add Ollama health check in pipeline — if Ollama returns error/timeout 3x consecutively, log a warning and attempt `docker restart ollama` or re-warm the model via a lightweight ping request before each batch. Consider adding a `/api/health` endpoint that checks Ollama reachability.

### Entity Contamination in Titles
- "Alexey Kochnev, Andrey Manturov (Montenegro)" appearing in unrelated situation titles
- African diplomacy, China-EU trade situations all show these names
- Likely cause: these entities are high-frequency in GDELT data and getting merged into many clusters
- The title generator is then including them as prominent entities
- Fix: title generation should weight entity relevance, not just frequency

### 490-Event Mega-Cluster
- "African-diplomatic-response: Alexey Kochnev..." absorbed 490 events
- Size penalty cap (80) not preventing this — may be from rapid batch ingest
- Related to entity contamination: overly broad entity match pulling in unrelated events

### RSS Feed 403 Errors
- breakingdefense.com and warontherocks.com returning 403 Forbidden
- These sites likely block non-browser user agents
- Fix: add browser-like User-Agent header to RSS fetcher, or remove these feeds

### GDELT Mega-Cluster (798 events)
- "Iran and United States Military Conflict Escalates" absorbed 798 events
- GDELT bulk ingestion bypasses size penalty — events arrive faster than penalty can gate
- Size penalty only applies per-ingest scoring, but batch ingest scores all events before any are added
- **Fix needed**: enforce hard cap during ingest — reject events into a cluster once it exceeds threshold, force new cluster creation

### CertStream Instability
- Upstream server (certstream.calidog.io) closing WebSocket every ~60s
- Auto-reconnect working but log noise
- Not a code issue — upstream service quality

---

## Backlog Items

1. **Ollama auto-recovery**: Add health check in pipeline — if Ollama returns error/timeout 3x consecutively, attempt to re-warm model via lightweight ping. Consider `/api/health` endpoint that checks Ollama reachability.

2. **Visual update indicators**: Add visual indicators on the map and situation list when situations are updated. Map markers should flash/pulse when new events arrive. Situation list items should highlight briefly on update (e.g., brief glow or "NEW" badge that fades).

3. **FIR disambiguation**: Add source_type context to enrichment prompt so Haiku knows NOTAM "FIR" = Flight Information Region, not fire/forestry.

4. **GDELT bulk ingest size cap**: Enforce hard maximum cluster size during ingest to prevent mega-clusters from GDELT batch ingestion.

5. **AirplanesLive rate limit recovery**: Investigate persistent 429s from desktop IP. Consider longer base poll interval or API key registration.

6. **RSS feed User-Agent**: breakingdefense.com and warontherocks.com returning 403 — add browser-like User-Agent header.

7. **Entity contamination in titles**: "Alexey Kochnev, Andrey Manturov" appearing in unrelated situation titles. Title generator should weight entity relevance by TF-IDF, not raw frequency.

8. **UI: Move domain tabs into news feed**: Move Kinetic/Cyber/Track/Intel tabs to be sub-tabs within the News feed tab instead of separate bottom-half tabs. Make the situation feed (alerts panel) full height in the left sidebar.

9. **UI: Sub-situations tree view**: Sub-situations list should be at bottom of right panel, collapsible, tree structure grouped by topic/region instead of flat list.

10. **Map markers missing for GDELT/RSS situations** (ROOT CAUSE FOUND):
    - Bug 1: Centroid fallback calls `region_center("ME")` but expects `"middle-east"`. Doesn't try `country_center()`.
    - Bug 2: GDELT/RSS hardcode `latitude: None, longitude: None`. GDELT has `sourcecountry` — use `country_center()`.
    - Frontend: `news_article`/`geo_news` in `hiddenEventTypes` by default.

11. **KIA/critical events not surfacing** (CRITICAL): "Sixth American service member killed in Iran operation" should be top criticality but isn't. Problems:
    - State change detection may not trigger for RSS headlines (enrichment may not extract "killed" as state_change)
    - Even if detected, the event is buried in a 800-event mega-cluster where severity signal is diluted
    - Need: keyword-triggered critical severity escalation for KIA/killed/death + military/US context
    - Need: alert banner for critical state changes regardless of cluster size

12. **Periodic analysis JSON truncation** (BROKEN): Sonnet analysis runs every 15min (HIGH tempo) but ALL 3 attempts have failed with "EOF while parsing" — JSON output truncated at ~4096 tokens. With 792 situations, the output is too large.
    - Fix: increase max_tokens for analysis, OR limit input to top-N situations by severity, OR chunk the analysis
    - Impact: zero intel briefs generated, right panel "situation report" view is empty, 68k Sonnet tokens wasted

13. **Visual update indicators**: Add pulse/flash on map markers and situation list items when updated.

14. **Title stability for parent situations** (IMPORTANT): Parent titles drifting when children merge in — "Iran war" becomes "African nations respond to Iran conflict" because absorbed child entities dominate title regen. Fixes:
    - Lock/freeze titles for parent situations above N children or N events
    - Or weight "founding" entities (from first N events) much higher than absorbed child entities in title prompt
    - Or add semantic similarity check: reject new title if cosine_sim < 0.6 with previous title
    - The title should represent the situation's core identity, not its latest child

15. **Situation report in right panel**: Periodic Sonnet analysis (`/api/intel/latest`) should show in right panel when no situation is selected. Currently broken because analysis JSON is truncating (item 12).

16. **Post-enrichment severity escalation** (CRITICAL): RSS events all arrive as `low` severity. Enrichment extracts state changes (killed, wounded) but severity is never updated. "Sixth American service member killed in Iran" stays `low`. Need a post-enrichment hook that escalates severity based on state_changes (killed → critical, wounded → high) and entity context (US military → boost).

~~17. Debug inspection API endpoint~~ — Removed: direct psql via `/inspect` skill covers this need without exposing internals on the network.

18. **Phase transition too aggressive** (IMPORTANT): Active Iran war showing as "declining" phase. Phase logic likely triggers on event rate dropping between GDELT poll bursts. Fix: phase transitions should consider severity and state changes, not just event velocity. A situation with critical severity and recent state changes (killed) should never be "declining" regardless of event rate. Also consider minimum dwell time in each phase before allowing transition.

8. **UI: Move domain tabs into news feed**: Move Kinetic/Cyber/Track/Intel tabs to be sub-tabs within the News feed tab instead of separate bottom-half tabs. Make the situation feed (alerts panel) full height in the left sidebar.

9. **UI: Sub-situations tree view**: Sub-situations list (e.g. "Sub-situations (371)") should be:
   - At the bottom of the right side panel (situation drawer)
   - Collapsible (collapsed by default when count is high)
   - Tree structure instead of flat list — group by topic/region/severity to make large counts consumable

10. **Map markers missing for GDELT/RSS situations** (ROOT CAUSE FOUND):
    - **Bug 1**: Centroid fallback in `situation_graph.rs:1501` calls `region_center("ME")` but function expects `"middle-east"`. Also doesn't try `country_center()`. Fix: normalize region codes and chain both lookups.
    - **Bug 2**: GDELT (`gdelt.rs:217`) and RSS (`rss_news.rs:339`) hardcode `latitude: None, longitude: None`. GDELT has `sourcecountry` field — use `country_center()` for approximate geo.
    - **Frontend**: `news_article` and `geo_news` in `hiddenEventTypes` by default in map store.
    - **Result**: 800-event Iran situation has null centroid, zero map markers, can't fly-to.
