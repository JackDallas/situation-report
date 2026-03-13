# Situation Clustering Audit — 2026-03-05 22:30 UTC

## Summary

**Verdict: FAILING.** Top-level situations are severely fragmented, duplicated, and noisy. The clustering is not production-quality. An analyst opening this dashboard would be confused, not informed.

| Metric | Value | Target | Status |
|--------|-------|--------|--------|
| Total situations in DB | 2,141 | — | Bloated |
| Top-level (API visible) | 20 | 8-15 | Borderline |
| Top-level (DB, no parent) | 265 | <50 | **CRITICAL** |
| FIRMS-only situations | 887 (41%) | 0 at top level | **CRITICAL** |
| Top-level FIRMS-only | 81 | 0 | **CRITICAL** |
| Duplicate titles at top level | 42x "Sub-Saharan Africa Wildfires" | 0 | **CRITICAL** |
| Centroids persisted | 44 / 2,141 (2%) | >80% active | **POOR** |
| Centroids warmed on startup | 21 | — | Low coverage |

## Critical Issues

### 1. FIRMS Wildfire Spam (P0)

FIRMS thermal_anomaly events are creating hundreds of identical single-source situations that never merge:

- **42** separate "Sub-Saharan Africa Wildfires" top-level situations
- **18** separate "Sub-Saharan Africa Wildfire Clusters"
- **9** "Oceania Wildfire Thermal Anomalies"
- **6 wildfire situations visible** in the API top-20, totaling 1,453 events fragmented across them

**Root cause**: FIRMS events are high-volume, low-entity, geographically dispersed. The clustering threshold for FIRMS is only 2, so every small cluster of hotspots becomes its own situation. These never merge because:
- Title Jaccard similarity doesn't differentiate "Sub-Saharan Africa Wildfires" from another "Sub-Saharan Africa Wildfires"
- Vector embeddings are nearly identical (same topic, same words) but geo distance blocks merge
- No dedup/consolidation pass recognizes that 42 situations with the same title should be one

**Fix needed**: Either (a) FIRMS situations should be forced into regional mega-clusters by the merge sweep, (b) FIRMS-only situations should not promote to top-level, or (c) a title-dedup pass should auto-merge same-titled situations.

### 2. Conflict Fragmentation (P1)

Related conflicts are scattered across multiple top-level situations:

**Israel/Lebanon** — 3 separate situations:
- "Israel-Lebanon Border Armed Conflict" (200 events, 4 sources)
- "Southern Lebanon Israeli-Hezbollah Armed Conflict" (8 events, 4 sources)
- "Israel Iran Regional Conflict" (46 events, 4 sources)

These should be at most 2: one Israel-Lebanon tactical conflict, one broader Iran-Israel strategic situation.

**Yemen** — 2 separate:
- "Yemen Military Asset Spots" (106 events, 2 sources)
- "Yemen Houthi Conflict Escalation" (26 events, 2 sources)

Same conflict, should be merged.

**Iran** — 2 separate:
- "Iran Iraq Proxy Conflict Escalates" (188 events, 5 sources)
- "Israel Iran Regional Conflict" (46 events, 4 sources)

These are arguably distinct (proxy vs direct), but the entity overlap (Iran) should at least link them.

### 3. Mega-Clusters Without Discrimination (P1)

Several situations are bloated catch-alls with 150-428 events:

- "Africa Wildfires and Floods Surge" (428 events) — wildfires + floods + earthquakes jammed together
- "Central Africa Wildfires and Earthquakes" (425 events) — wildfires + earthquakes, 43% title overlap with above
- "Iran Iraq Proxy Conflict Escalates" (188 events) — kitchen sink
- "Eastern Europe Military Buildup" (175 events) — vague

These mega-clusters absorb everything and become meaningless. An analyst can't act on "428 events about Africa."

### 4. Low Centroid Persistence (P1)

Only 44 of 2,141 situations have persisted centroids (2%). On restart, the merge sweep is nearly blind:
- 21 centroids warmed on latest startup (from the centroid persistence we just deployed)
- But 2,097 situations have no centroid — vector merge can't score them
- This explains the proliferation: without centroids, new events can't find their cluster

The centroid persistence code was deployed this session but hasn't had time to backfill. The pipeline only writes centroids on upsert, so old situations never get one.

### 5. Phase Distribution Skew

| Phase | Count |
|-------|-------|
| emerging | 863 |
| active | 714 |
| declining | 371 |
| developing | 193 |
| resolved | 0 |
| historical | 0 |

**Nothing ever resolves.** 863 situations stuck in "emerging" means the lifecycle FSM isn't transitioning. Either the gap/inactivity detection isn't running, or the thresholds are too generous.

### 6. Region Field Empty

Every top-level situation shows `region: ?` (null). The region field isn't being populated on situations, which means:
- Regional dedup can't work
- The UI can't group by region
- Cross-region merge thresholds fire incorrectly

## Recommendations (Priority Order)

1. **FIRMS suppression**: FIRMS-only situations should NOT be top-level. Either require multi-source or cap FIRMS situations as children of a regional wildfire parent.
2. **Title dedup merge**: If two top-level situations have >70% title Jaccard overlap AND same region, force-merge them.
3. **Centroid backfill**: One-time script to compute and persist centroids for all active situations.
4. **Lifecycle enforcement**: Situations with no new events in 6h+ should transition to declining→resolved.
5. **Region population**: Ensure `region` is set on all situations from their events' region_code.
6. **Mega-cluster splitting**: Situations above 200 events should be reviewed for subtopic splitting.
7. **Quality gate tightening**: FIRMS-only situations should require higher event counts (10+) to form.

## Raw Numbers

```
Events in last hour by source:
  thermal_anomaly (FIRMS): ~150/hr
  conflict_event: ~30/hr
  news_article: ~20/hr
  seismic: ~10/hr
  notam: ~5/hr
```

The FIRMS firehose is drowning signal in noise. Until that's fixed, the situation list will remain cluttered.
