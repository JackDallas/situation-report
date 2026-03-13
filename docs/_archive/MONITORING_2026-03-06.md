# Situation Clustering Monitoring — 2026-03-06

## Deploy Timeline

| Time (UTC) | Event |
|------------|-------|
| 00:50 | Deployed v2 with 6 clustering fixes + tighter quality gate |
| 00:52 | Container started, 48 centroids warmed (up from 21) |
| 00:53 | First sweep: 19 "Sub-Saharan Africa Wildfires" detached to top-level |
| 01:00 | Phase transitions observed (Emerging→Developing, Developing→Active) |

## Fixes Deployed

1. **ThermalAnomaly removed from is_important_category()** — FIRMS goes through noise buffer
2. **Single-source telemetry gate** — FIRMS/aviation-only clusters never surface as standalone top-level
3. **Title-dedup merge for empty regions** — `both_regions_empty` allows merge when neither has region
4. **Child region_codes propagated to parent** — fixes region-based merge logic
5. **Gap tolerance ceiling (24h) + single-source fast-track (6h)** — prevents infinite inflation
6. **Standalone quality gate** — requires title + 2+ source diversity + not telemetry-only

## Monitoring Checks

### T+10min (01:00 UTC) — First Check

| Metric | Before | After | Target | Status |
|--------|--------|-------|--------|--------|
| Top-level situations | 20 (fragmented) | 11 | 8-15 | **PASSING** |
| Total API clusters | 20 | 99 | — | OK (children now visible) |
| Duplicate titles at top level | 42 | 0 | 0 | **PASSING** |
| FIRMS-only top-level | 6 | 0 | 0 | **PASSING** |
| Single-source top-level | 2 | 0 | 0 | **PASSING** |
| Wildfire at top level | 6 | 4 | 0-2 | Improved |
| Resolved situations | 0 | 0 | >0 after 6h | Expected (too early) |
| Phase transitions | none observed | 3 | — | **ACTIVE** |

**Top-level situations:**
1. Central Africa Wildfires and Earthquakes (427 evts, 5src, 11ch) — critical active
2. Iran Iraq Proxy Conflict Escalates (193 evts, 5src, 11ch) — critical active
3. Eastern Europe Military Buildup (185 evts, 4src, 3ch) — critical active
4. North Africa Fire Clusters (152 evts, 5src, 4ch) — critical active
5. Sahel Military Buildup (150 evts, 4src, 4ch) — critical active
6. Africa Drought and Wildfires (150 evts, 2src, 7ch) — high declining
7. Global Wildfires and Storms Surge (125 evts, 6src, 7ch) — medium declining
8. Yemen Military Asset Spots (107 evts, 2src, 7ch) — medium developing
9. DRC Military Buildup (106 evts, 2src, 6ch) — medium developing
10. Myanmar Conflict and Military Movements (102 evts, 2src, 3ch) — medium developing
11. North America Earthquake Swarm (60 evts, 2src, 5ch) — critical developing

**Assessment**: Massive improvement. 11 clean top-level situations, zero duplicates, zero single-source.

**Remaining issues:**
- 4 wildfire situations still at top level (multi-source, legitimate but cluttered)
- "Central Africa Wildfires and Earthquakes" is a 427-event mega-cluster (mixed topics)
- "Global Wildfires and Storms Surge" is vague/cross-region
- No resolved situations yet (expected, need 6h+ for lifecycle)
- Missing important conflicts: Israel-Lebanon, Ukraine, Yemen-Houthi now only as children

## Issues to Investigate

### 1. Wildfire Mega-Clusters (P1)
4 wildfire-related situations at top-level is still too many. They're multi-source (FIRMS + GDELT + GeoConfirmed + RSS + Telegram) so they pass the quality gate. Need to consider:
- Should wildfires be capped at 1-2 top-level mega-clusters?
- Could the merge sweep consolidate all wildfire situations into one "Global Wildfire Activity" parent?
- Title similarity between "Central Africa Wildfires" and "Africa Drought and Wildfires" should trigger merge

### 2. Important Conflicts Buried as Children (P1)
Pre-deploy, Israel-Lebanon and Ukraine were top-level. Now they might be children of the big parents. Need to verify:
- Is "Israel-Lebanon Border Armed Conflict" a child of "Iran Iraq Proxy Conflict"?
- Is Ukraine conflict visible? "Eastern Europe Military Buildup" sounds like it might contain Ukraine

### 3. Mega-Cluster Quality (P2)
"Central Africa Wildfires and Earthquakes" with 427 events is too broad. Contains wildfires + earthquakes + floods in one bucket. The coherence splitting should trigger if mean pairwise cosine < 0.45.

### 4. Severity Inflation (P2)
Every 30s, parent severity gets raised to match highest child. This means most top-level situations become "critical" even if the overall situation is not critical. 5 of 11 are "critical active."

### 5. Phase Resolution Timeline (Monitor)
No situations have resolved yet. Expected timeline:
- Single-source clusters: should resolve after 6h of inactivity
- Multi-source: after 24h max (was 86h before)
- Check again at T+6h (~07:00 UTC)

### 6. ReliefWeb API 403 (P2)
ReliefWeb now requires an approved appname. Need to register at https://apidoc.reliefweb.int/

### 7. Severity Inflation from Child Propagation (P2)
Every sweep cycle raises parent severity to match highest child. Most top-level become "critical"
even when the overall cluster is moderate. 7 of 13 top-level are critical active.

---

### T+30min (01:23 UTC)
- Top-level: 12 → matches target range
- **Ukraine Military Buildup and Conflict** surfaced as #2 (198 evts, 36 children) — major consolidation
- 28 merges in 10min — aggressive merge activity
- 50 title/narrative operations — AI title gen working
- Emerging dropped to 5 (from 17 at T+10)

### T+45min (01:39 UTC)
- Top-level: 13 — "Central African Republic Wildfires" appeared (335 evts, firms+gdacs)
- Ukraine grew to 50 children, Iran to 29
- Merge rate slowing (12 in 20min)

### T+65min (01:59 UTC)
- Stable at 13 top-level, 155 total
- DB still shows 863 emerging (historical, not in-memory)

### T+85min (02:19 UTC)
- Stable at 13, active clusters 94
- API phase distribution settling: active=94, developing=30, declining=16, emerging=5

### T+110min (02:47 UTC)
- Stable at 13 top-level, 147 total
- 17 merges in 30min (slowing — stabilizing)
- 0 phase transitions (steady state)
- No new errors beyond known AIS/ReliefWeb issues
- Myanmar developing → high severity

**Status: STABLE. System has converged to 13 top-level situations.**

### T+280min (05:39 UTC)
- 13 top-level, declining=14
- Several smaller orphan situations surfaced (Colombia, Madagascar, Australia, Greece)
- Venezuela declined, DRC declined
- Still 0 resolved

### T+350min (06:50 UTC)
- 19 top-level — spike from parent eviction orphaning children
- "Central Africa Drought Crisis", "Venezuela Drought", "East Africa Drought" appeared
- Still 0 resolved

### T+380min — BUGFIX: Decayed Peak Rate (07:29 UTC)
**Bug found**: `compute_gap_tolerance()` used raw `cluster.peak_event_rate` (all-time peak, never decays) instead of decayed peak. A cluster with a one-time burst would have permanently inflated gap tolerance.

**Fix**: Added decay calculation using `peak_decay_half_life_mins` (30min half-life). After 7h of inactivity, decay = 0.5^14 ≈ 0.00006, so peak effectively reaches 0 and gap_tolerance drops to base value.

**Deployed**: 07:29 UTC

**Result**: Immediate wave of phase transitions — 14 Active→Declining transitions at 07:38 with gap_tolerance=2.6h (was previously 10-15h for the same clusters). 42 declining situations now.

### T+450min (08:35 UTC)
- 13 top-level, 41 declining
- Still 0 resolved — expected: resolve threshold = gap_tolerance * 1.5 ≈ 7.8h
- First resolutions predicted ~12:50 UTC

### T+490min (09:17 UTC)
- 14 top-level, 25 declining, 55 active
- Active count recovered from 50→55 (new events forming new situations)
- "Somalia Agricultural Monitoring Systems" (3 evts) appeared — low-quality, should be gated
- Still awaiting first resolutions

### T+520min (09:25 UTC)
- 14 top-level, 93 total (active=43, declining=25, developing=22, emerging=3)
- 25 phase transitions total since deploy (6 Dev→Declining, 15 Active→Declining, 4 Emerging→Developing)
- 0 resolved — math verified: medium severity + 4+ sources → resolve_threshold=7.8h
- Oldest declining cluster (Australia Wildfires, last_updated 03:59) resolves at ~11:47 UTC
- Batch of 9 declining clusters (last_updated 05:00) resolve at ~12:48 UTC
- Budget: $4.69 / $10 — healthy
- Known source errors: AIS key invalid (backoff), ReliefWeb 403 (needs appname), 2 RSS feeds 403
- Severity inflation still happening every 30s (parent severity raised to match child — spammy log)

### T+555min (10:00 UTC)
- 15 top-level (+1 low/emerging), 145 total, declining=53, active=64, developing=23, emerging=5
- Declining doubled 25→53 — many children clusters decaying now
- Active grew 43→64 — new events creating new active child situations
- "Climate-intervention-private-sector in Unknown Region" appeared (2 evts, low emerging)
- DRC and Yemen both at 250 events now
- Still 0 resolved — predicted first at ~11:47 UTC (Australia Wildfires)
- AIS key error every 3min (backoff not increasing — fixed at 160s)

### T+615min (11:00 UTC)
- 14 top-level, 109 total (active=39, declining=46, developing=20, emerging=4)
- Second wave Active→Declining: 13 at 10:13 + 7 at 10:41 (rate decline trigger)
- Still 0 resolved — oldest declining clusters from 07:36 (only 4h ago)
- Previously older clusters (03:59-05:00) were evicted from memory before resolving

### T+660min (11:52 UTC) — BUGFIX: Prune vs Resolve Race
**Bug found**: `prune_stale()` uses max_age=6h, but resolve_threshold for medium/multi-source=7.8h. Declining clusters were being evicted from memory before reaching their resolve threshold. They could never resolve!

**Fix**: Skip pruning for Declining-phase clusters in `prune_stale_with_cache()`.

**Deployed**: 11:51 UTC

**Result**: **17 Declining→Resolved transitions on first sweep!** Examples:
- "Resolved: no activity for >10.4h (threshold=7.8h)"
- "Resolved: no activity for >9.5h (threshold=6.0h)"
- "Resolved: no activity for >12.2h (threshold=6.0h)"

Post-deploy: 37 top-level (25 declining), 184 total, resolved=17. Declining top-level will continue resolving over next 2-4h. All resolved are currently children (not top-level).

### T+720min (12:50 UTC) — FINAL CHECK
- 36 top-level, 264 total (active=147, declining=92, developing=20, emerging=5)
- 0 resolved in current memory (17 resolved earlier, then pruned — correct behavior)
- 36 total phase transitions logged since deploy
- Budget: $7.28 / $10 — healthy
- 10 active/developing core top-level situations (the rest are declining and will resolve)
- Broken AI titles: "I need more information..." and empty titles appearing at top-level
- New issue: orphan children promoting as top-level declining (13-18evts each)

## Final 12h Summary

| Metric | Before (00:50) | After (12:50) | Target |
|--------|----------------|---------------|--------|
| Top-level situations | 20 (fragmented) | 10 active + 26 declining | 8-15 active |
| Duplicate titles | 42 | 0 | 0 |
| FIRMS-only top-level | 6 | 0 | 0 |
| Single-source top-level | 2 | 0 | 0 |
| Resolved situations | 0 ever | 17 confirmed | >0 |
| Phase transitions | 0 ever | 36 logged | active |
| Max gap tolerance | 86h | 24h cap | <24h |

**Bugs found & fixed during monitoring (3):**
1. Quality gate too loose (T+10min) — tightened standalone: telemetry-only blocked, AI title required, 2+ source diversity
2. Decayed peak rate (T+380min) — raw all-time peak never decayed, inflating gap tolerance permanently
3. Prune vs Resolve race (T+660min) — prune_stale max_age=6h < resolve_threshold=7.8h, declining clusters evicted before resolving

## Remaining Issues for Investigation

### 8. "Global Wildfires and Political Scandals" Title Contamination (P2)
200-event mega-cluster with contaminated AI title mixing wildfires and political scandals. The coherence splitting should catch this, but the mixed topics may not trigger if cosine similarity is above threshold.

### 9. Orphan Promotion Cascading (P1)
When parent clusters are evicted from the 500-cluster in-memory graph, their children become orphans and get promoted to top-level. This causes top-level count spikes (12→19). Need:
- Larger in-memory graph window (1000+ clusters?)
- Or: when a parent is evicted, assign children to nearest sibling parent
- Or: resolved parents should stay in graph until children also resolve

### 10. Somalia Low-Quality Cluster (P2)
"Somalia Agricultural Monitoring Systems" with only 3 events and medium developing phase surfaced at top-level. It has 2 source types so it passes the diversity gate. May need a higher min_events_standalone threshold (current=8, this has only 3 — it might have children).

### 11. Duplicate Yemen Situations (P2)
"Yemen Military Asset Movements" appears twice (200 evts developing + 97 evts emerging). Title-dedup merge should catch this but may not be triggering because regions are different or title Jaccard is below threshold.
