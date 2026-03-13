# Pipeline Health & Quality Backlog

Live monitoring document tracking issues, observations, and improvements needed.

## Deployment: 2026-03-03 19:20 UTC (Pipeline Overhaul v1)

### Changes Deployed
- PipelineConfig: 80+ magic numbers externalized to sr-config crate
- Smooth size penalty: `-ln(1 + count/50)` replaces cliff penalties
- Sweep system: 4-pass periodic cleanup (metadata prune, shed oversized, orphan removal, coherence audit)
- Title stability: 30-min minimum between AI title regenerations
- Quality gate fix: children need 3+ events (was 1)
- MAX_CHILDREN enforcement in merge loop
- Source fixes: AirplanesLive rate-limit cooldown, CertStream keepalive, GDELT error visibility, GeoConfirmed dedup, RSS two-buffer rotation
- Pipeline hardening: embedding worker restart loop, Semaphore(32) bounded spawning

---

## Active Issues

### P0 — Bad Ollama Titles (15 clusters)
**Observed**: T+18min post-deploy
**Symptom**: 15 child clusters titled "No relevant information provided to generate a title", "No location or conflict context provided", etc.
**Root cause**: These are ADS-B flight position clusters (adsb-fi, adsb-lol, airplaneslive + telegram). Flight position events have no textual content (just lat/lon/altitude/heading/callsign). When Ollama is asked to generate a title from these, it correctly reports there's nothing to title.
**Impact**: Visible in API, may show in UI child lists. All are children (parent=Y), so they're behind parent accordions in AlertsPanel.
**Fix needed**: Title generation for ADS-B-heavy clusters should:
1. Skip LLM entirely if >80% of events are flight positions
2. Generate formulaic title from callsigns + region (e.g., "Military Flight BAT61 Activity — Eastern Mediterranean")
3. Fall back to parent title + " — Flight Track Cluster"

### P0 — Overloaded Parent Clusters (inherited)
**Observed**: T+18min post-deploy
**Symptom**:
- "Eastern Europe Military Strikes and Drone Attacks": 23 children, 4,113 child events
- "US Military Aircraft Intercepted Over Unknown Region": 17 children, 2,923 child events
- "US Israel Iran Conflict and Lebanon Hostilities": 43 children, but only 166 total child events (many near-empty)
**Root cause**: Pre-existing clusters from before MAX_CHILDREN fix. The config default is `max_children_per_parent: 20` but these were already built. The 43-child "US Israel Iran" parent accumulated tiny children before the quality gate was raised to 3.
**Impact**: Bloated parent situations, UI accordion shows too many children, some children have near-identical topics.
**Fix needed**:
1. Add a sweep pass: `sweep_excess_children()` — if parent has > max_children, merge smallest children back into parent or dissolve them
2. Consider a one-time migration sweep on startup to enforce new limits on legacy clusters
3. The "43 children" case suggests the quality gate wasn't filtering at ingest time — verify the child quality gate applies during `merge_overlapping()` not just `active_clusters()`

### P1 — ADS-B Mega-Clustering
**Observed**: T+18min post-deploy
**Symptom**: ADS-B flight positions create enormous clusters (200+ events). Each ADS-B source (3 sources) polls every 90s and generates position events that cluster together because they share entities (aircraft callsigns) and regions.
**Root cause**: Flight positions are high-volume, share callsigns (strong entity match = high score), and are geographically concentrated. The scoring system treats entity matches the same regardless of event type.
**Impact**: Situations dominated by flight positions rather than meaningful intelligence events. Dilutes the signal.
**Fix needed**:
1. Event type weighting in `score_candidate()` — flight_position events should score lower for clustering (they're tracking data, not intelligence)
2. Consider separate tracking layer: flight positions should live in `positions` table/pipeline, not situations
3. The `is_high_volume()` flag on EventType should influence clustering threshold — high-volume types need higher score to join a cluster
4. Alternatively, the summarizer could absorb flight_positions like it does for other high-volume types (30s summaries)

### P1 — Exa Search Rate-Limit Burst on Startup
**Observed**: T+0 (startup)
**Symptom**: ~50 supplementary search requests fired simultaneously, hitting Exa's 10 req/s limit. All returned 429.
**Root cause**: On startup, all clusters with `needs_search` flag get search requests queued simultaneously. The `SearchRateLimiter` has a cooldown_secs=30 between requests for the same cluster, but nothing throttles the total request rate across all clusters.
**Fix needed**:
1. Add global rate limiter: max 8 requests/second across all Exa searches
2. Stagger startup searches: add jitter/delay when processing backfilled clusters
3. Consider a startup-mode where searches are deferred until T+5min after startup

### P1 — Zero Enrichments After Startup
**Observed**: T+43min
**Symptom**: 0 events enriched in 10 minutes. Ollama is generating narratives and titles but not enriching individual events.
**Root cause**: Need to investigate. Possible causes:
1. Enrichment only runs on news_article/conflict_event types (not ADS-B/BGP), and no news events flowed in the period
2. RSS only returned 2 events in 10min — may all have pre-existing enrichment (dedup check)
3. Ollama queue congested with narrative/title generation, enrichments queued behind
**Impact**: New events missing entity extraction, topic extraction, state change detection, translation
**Investigation**: Check `event_has_enrichment()` dedup and enrichment queue depth

### P2 — CertStream Reconnection Frequency
**Observed**: T+18min, T+23min
**Symptom**: "CertStream closed by server, reconnecting" every ~60s
**Root cause**: The 30s ping keepalive fix was deployed, but the server may still be closing the connection. The reconnection is clean (backoff resets to 1s) so it's not death-spiraling, but it means CertStream is producing no data.
**Impact**: CertStream events (cert_issued) are useful for domain monitoring but zero data is flowing from this source.
**Investigation needed**: Check if CertStream.io service is actually down, or if the WebSocket handshake/URL has changed.

### P2 — GDELT Transient Failures
**Observed**: T+0, T+23min
**Symptom**: GDELT requests failing with network errors, retrying once, then failing again. Error now correctly propagated to registry (fix working).
**Root cause**: GDELT API appears to have intermittent availability issues. The fix makes failures visible but doesn't solve the underlying flakiness.
**Impact**: GDELT news articles not flowing during outage periods. Registry backoff will retry.
**Future**: Consider adding a CDN/cache layer or alternative news source as fallback.

### P2 — Duplicate Topic Titles in Related Situations
**Observed**: T+18min
**Symptom**: Three separate "Eastern Europe Military Activity and Strikes" situations, all critical, 117-195 events. These should have been merged.
**Root cause**: Pre-existing clusters that were too large to merge (combined size would exceed old thresholds). With smooth penalty, they could potentially merge now, but the merge conditions require entity/topic overlap that may not be sufficient.
**Impact**: User sees redundant situations with near-identical titles.
**Fix needed**: Merge sweep should be more aggressive with title-similar clusters. Consider adding a title Jaccard check to merge conditions: if titles are >0.7 similar AND shared region, merge regardless of entity overlap.

---

## Sweep System Observations

### Sweep 1 (T+2min): pruned 9 topics, 0 entities, 0 shed, 17 orphans removed
- Orphan cleanup working well — 17 near-empty children removed
- Topic pruning active but conservative (only 9 topics across all clusters)

### Sweep 2 (T+20min): pruned 10 topics, 0 entities, shed 1,093 events, 0 orphans
- Event shedding activated — 1,093 events trimmed from oversized clusters
- This reduced many 400+ event clusters down to 200 (shed_threshold default)
- No entity pruning yet — entities may be more stable than topics

### Sweep 3 (T+26min): pruned 14 topics, 0 entities, shed 1,155 events, 0 orphans
- Shedding continuing — another 1,155 events trimmed
- Topics pruned increasing (14 vs 10 vs 9) — system finding more stale topics

### Sweep 4 (T+28min): pruned 22 topics, 0 entities, shed 648 events, 0 orphans
- Shed count decreasing (648 vs 1,155) — clusters converging toward shed_threshold

### Sweep 5 (T+30min): pruned 24 topics, 0 entities, shed 846 events, 0 orphans
- Topic pruning accelerating (24 topics) — good sign of metadata hygiene

### Narratives: 28 generated in 5 minutes
- Ollama generating narratives at ~5/min — GPU is well-utilized
- Good titles coming through: "Epstein Investigation Involves Clinton and Mandelson", "Berlinale Film Festival Gaza Conflict Debate"

### Sweep 6 (T+34min): pruned 31 topics, 0 entities, shed 649 events, 0 orphans
- Topic pruning peaked at 31 — system finding and removing stale metadata aggressively
- Shed stabilizing around 650-850 per sweep (new ADS-B events accumulate between sweeps)

### Sweep 7 (T+36min): pruned 22 topics, 0 entities, shed 731 events, 0 orphans
- Steady-state pattern emerging: ~700 events shed per 2-min sweep cycle
- This is the ADS-B throughput being trimmed back each cycle — expected behavior

### Sweep 8-12 (T+34-43min): Steady state pattern
- Topic pruning accelerating: 18 → 19 → 20 → 21 → 27 → 41 topics/sweep
- **Entity pruning activated at T+36min**: 2 entities pruned, then 5 — system finding and removing contamination
- Event shedding: 870 → 998 → 353 → 949 → 487 → 744 per cycle (ADS-B accumulation between sweeps)
- 1 more orphan removed at T+32min
- System self-correcting as expected — entity pruning kicking in after topic pruning stabilized

### Observation: Entity pruning now active
- Started pruning entities at sweep 10 (2 entities) and sweep 12 (5 entities)
- Indicates entity contamination being cleaned up from pre-existing clusters

---

## Source Health Snapshot

| Source | Status | Notes |
|--------|--------|-------|
| ADS-B (3x) | Healthy | ~3,200 events/5min combined. AirplanesLive handling 429s properly |
| FIRMS | Healthy | 230 events/10min |
| BGP | Healthy | 230 events/5min |
| OpenSky | Healthy | 80 events/5min |
| Shodan | Healthy | 16 ICS scans on startup |
| NOTAM | Healthy | 6 airspace notices |
| Cloudflare | Healthy | 2 radar events |
| RSS News | Healthy | Events in DB, polling normally |
| Telegram | Healthy | 395 recent events on startup, streaming |
| GeoConfirmed | Healthy | 706 total events in DB |
| GDELT | Degraded | Transient API failures, in backoff |
| GDELT Geo | Degraded | Same as GDELT |
| CertStream | Degraded | Reconnecting every 60s, no data |
| OONI | Degraded | IR endpoint failing |
| OTX | Degraded | Response parsing failures |
| GPSJam | Idle | Last success 13:27 UTC |
| Nuclear | Idle | Polling but no events (expected) |
| GFW | Idle | Polling, no recent events |
| AIS | Healthy | Streaming, connected to 6 regions |

---

## Quality Metrics to Track

- [ ] Number of situations with bad/placeholder titles (target: 0)
- [ ] Max children per parent (target: <=20)
- [ ] Percentage of situations with narratives (target: >70% for severity >= medium)
- [ ] Percentage of situations with centroids (target: >90%)
- [ ] Average intra-cluster coherence score (need to expose this metric)
- [ ] Enrichment coverage: % of news events with enrichment data
- [ ] Title regeneration frequency (should be <1/hour per cluster after stability fix)

---

## Future Overhaul Ideas

### Tier 1 — Next Sprint
1. **ADS-B/Position Event Separation**: Flight positions should not cluster into situations. Move to separate tracking pipeline or require minimum non-position events (e.g., 3+ news/conflict/telegram) before a cluster becomes a situation.
2. **Formulaic Title Fallback**: When Ollama produces garbage titles, use template: `"{primary_entity} Activity — {region}"` instead.
3. **Startup Search Staggering**: Rate-limit Exa searches to 5/s with jitter on startup.
4. **Excess Children Sweep**: Dissolve or merge children when parent exceeds max_children.

### Tier 2 — Near-Term
5. **Coherence-Based Split**: The sweep's coherence audit needs embeddings cached for existing clusters. Currently only works for clusters created after startup. Need to pre-compute embeddings for top-N situations on startup.
6. **Merge by Title Similarity**: Add title Jaccard to merge conditions for deduplicating near-identical situations.
7. **Source Reliability Scoring**: Track per-source uptime/error rates, surface in UI, auto-disable consistently failing sources.
8. **Narrative Quality Gate**: Don't display narratives that are <200 chars or contain "no information available" patterns.

### Tier 3 — Architectural
9. **Event Type Weighting**: Different event types should have different base scores for clustering. News articles and conflict events are high-value signals; flight positions and BGP anomalies are low-value individually.
10. **Situation Lifecycle Automation**: Auto-resolve situations that haven't received new events in 24h. Auto-merge situations that share >3 entities and same region.
11. **Real-time Coherence Monitoring**: Expose intra-cluster similarity as a metric. Alert when coherence drops below threshold.
12. **Multi-tenant Pipeline Config**: Allow per-region or per-domain config overrides (e.g., Middle East clusters need different thresholds than cyber monitoring).
