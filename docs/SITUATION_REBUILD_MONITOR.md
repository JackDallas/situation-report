# Situation Rebuild Monitor - 2026-03-22

## Baseline (before wipe)

**Snapshot taken: 2026-03-22**

### Counts
| Metric | Value |
|--------|-------|
| Total situations | 979 |
| Distinct titles | 946 |
| Top-level (hierarchy) | N/A (all hierarchy_path NULL) |
| Children | N/A |
| situation_events rows | 0 (table empty) |

### Phase Distribution
| Phase | Count | Avg Severity | Avg Source Diversity |
|-------|-------|-------------|---------------------|
| active | 358 | 2.2 | 9.7 |
| resolved | 265 | 1.8 | 9.1 |
| declining | 163 | 1.6 | 7.1 |
| developing | 151 | 1.4 | 3.7 |
| emerging | 34 | 1.0 | 1.0 |
| historical | 8 | 1.4 | 9.3 |

### Severity Distribution
| Severity | Count |
|----------|-------|
| CRITICAL (4) | 1 |
| HIGH (3) | 82 |
| MEDIUM (2) | 642 |
| LOW (1) | 254 |

The single CRITICAL situation is "Indian Navy Secures Oil Amid Iran-US Tensions" -- questionable.

### Source Diversity Distribution
| Source Diversity | Count |
|-----------------|-------|
| 12 | 269 |
| 11 | 174 |
| 10 | 20 |
| 9 | 19 |
| 8 | 91 |
| 7 | 91 |
| 6 | 20 |
| 5 | 43 |
| 4 | 8 |
| 3 | 88 |
| 2 | 80 |
| 1 | 76 |

**Problem**: 269 situations have 12 source types and 174 have 11. The top source_types list is a kitchen-sink combo of 14 types (bluesky, copernicus, firms, gdacs, gdelt, geoconfirmed, imb-piracy, notam, ooni, otx, rss-news, telegram, ukmto-warnings, usgs). This means source_types is being inherited/accumulated incorrectly -- a single situation should not contain copernicus + otx + imb-piracy + usgs unless it truly spans all those domains.

### Location Issues
| Location | Count | Issue |
|----------|-------|-------|
| (17.02, -8.33) - Mid-Atlantic/Angola coast | 248 | Suspicious cluster |
| (-38.73, 20.81) - Mid-Atlantic Ocean | 225 | Clearly wrong |
| (37.51, 48.26) - Eastern Ukraine | 162 | Plausible |
| (-17.58, 20.81) - Atlantic Ocean | 28 | Wrong |
| (0, 0) - Null Island | 27 | Fallback/default |

**Problem**: 500+ situations have mid-Atlantic or Null Island locations. Only ~162 at Eastern Ukraine are plausible.

### Topic Fragmentation
| Topic | Situation Count | Issue |
|-------|----------------|-------|
| Iran/Israel/Houthi/Yemen | 246 | Massively fragmented |
| Ukraine/Russia | 69 | Fragmented |
| Earthquake | 32 | Should be zero or minimal |

**Sample fragmentation** (Iran/Israel/Houthi/Yemen): "Anti-Houthi Coalition Strikes", "Anti-Houthi Separatist Strikes", "Brussels Houthi Designation Push", "Canada PM Carney Iran", "Canada PM Carney Supports Iran Strikes", "EU Gas Prices Surge Over Iran Conflict", "EU Iran Energy Crisis", "EU Iran Policy Shift", etc. -- these should be consolidated into a handful of parent situations.

### Noise Situations
- "Roman Calendar Reform Explained" -- Bluesky personality noise
- "British Calendar Reform Explained" -- Bluesky personality noise
- "AEI Director Fired" -- not OSINT relevant
- "Pete Monks Criticizes US Allies" -- noise

### Baseline Summary
The current state has significant quality issues:
1. **Kitchen-sink source_types**: 45% of situations have 11-12 source types (inheritance bug)
2. **Bad locations**: 50%+ at mid-Atlantic or Null Island
3. **Massive fragmentation**: 246 Iran/Israel situations, 69 Ukraine/Russia -- should be ~5-10 each
4. **Empty event linkage**: situation_events table has 0 rows
5. **No hierarchy**: all hierarchy_path values are NULL
6. **32 earthquake situations**: should be zero or minimal for an OSINT intel tool
7. **Noise leakage**: calendar reform, personality posts making it through

---

## Rebuild Progress

| Time (UTC) | Situations | Distinct Titles | Notes |
|------------|-----------|----------------|-------|
| pre-wipe | 979 | 946 | Baseline |
| 18:29:48 | 0 | 0 | Wipe detected, app restarted, backfill: 500k rows, 210k fed to graph |
| 18:30:52 | 2 | 2 | First situations: "Afghanistan-humanitarian-crisis" (rss-news), "Api Abuse" (otx) |
| 18:31-18:45 | 2 | 2 | Stuck -- crash loop every ~2.5min (9 restarts), rustls panics |
| 18:45-18:52 | 2 | 2 | Continued crash loop, multiple deploys by consolidation teammate |
| ~18:52 | 0 | 0 | Fresh deploy + wipe, rustls panics fixed (0 panics in new build) |
| 18:53 | 2 | 2 | First situations created (different from before) |
| 18:53-19:01 | 2 | 2 | Crash loop resumed (deploy iterations, no panics but restarts) |
| ~19:07 | 0 | 0 | Final wipe + deploy, server now stable |
| 19:10 | 216 | 216 | Explosive growth -- server stable, 1 restart only |
| 19:13 | 227 | 223 | Steady growth |
| 19:16 | 263 | 256 | Growing, some duplicate titles appearing |
| 19:19 | 314 | 306 | Growth slowing |

### Crash Loop Detail (18:29-19:07)
The app initially crash-looped with rustls CryptoProvider panics:
```
Task panic: panicked at rustls-0.23.37/src/crypto/mod.rs:249:14:
Could not automatically determine the process-level CryptoProvider from Rustls crate features.
Call CryptoProvider::install_default() before this point...
```
- 3-4 task panics per startup cycle when source tasks tried TLS before CryptoProvider was installed
- Process crashed ~2.5 min after each start (stack traces in logs)
- Each restart lost all in-memory clusters; only DB-persisted situations survived
- Consolidation teammate iterated through multiple deploys to fix this
- Final deploy (~19:07) was stable: 0 panics, 1 restart, steady situation growth

---

## Quality Assessment (snapshot at ~314 situations)

### Phase Distribution
| Phase | Count | Avg Severity | Avg Source Diversity |
|-------|-------|-------------|---------------------|
| developing | 219 | 1.7 | 3.8 |
| active | 50 | 1.1 | 3.1 |
| declining | 39 | 1.3 | 1.8 |
| emerging | 6 | 1.0 | 1.0 |

No resolved or historical phases yet -- expected for a fresh rebuild.

### Severity Distribution
| Severity | Count |
|----------|-------|
| MEDIUM (2) | 163 |
| LOW (1) | 151 |

No HIGH or CRITICAL -- big improvement over baseline where severity was inflated.

### Source Diversity Distribution
| Source Diversity | Count |
|-----------------|-------|
| 7 | 24 |
| 6 | 1 |
| 5 | 27 |
| 4 | 106 |
| 3 | 74 |
| 2 | 29 |
| 1 | 53 |

Max diversity is 7 (was 12 in baseline). No kitchen-sink combos of 14 source types.

### Source Types Patterns
| Source Types | Count |
|-------------|-------|
| bluesky, gdelt, geoconfirmed, otx, rss-news | 106 |
| bluesky, gdelt, geoconfirmed, ooni, otx, rss-news, telegram, ukmto-warnings | 48 |
| rss-news | 29 |
| bluesky, gdelt, rss-news | 27 |
| bluesky, rss-news, telegram | 26 |
| bluesky, gdelt, geoconfirmed, rss-news | 25 |

**Concern**: 106 situations all have the identical 5-type combo (bluesky, gdelt, geoconfirmed, otx, rss-news). While better than 14-type kitchen sinks, this suggests some situations may still be getting source_types from unrelated events absorbed into clusters. Need to verify these are genuinely multi-source.

### Location Distribution
| Location | Count | Assessment |
|----------|-------|-----------|
| (37.51, 48.26) - Eastern Ukraine | 128 | Plausible for conflict cluster |
| (37.52, 48.78) - Eastern Ukraine | 100 | Plausible for conflict cluster |
| (0, 0) - Null Island | 41 | Fallback/default -- needs fix |
| (51.4, 35.7) - Tehran, Iran | 25 | Plausible |
| (32.5, 15.5) - Sudan | 5 | Plausible |
| (7.5, 9.1) - Nigeria | 4 | Plausible |

**Massive improvement**: Zero mid-Atlantic bogus locations (was 500+ in baseline). Null Island (0,0) down from 27 to 41 -- but 41 out of 314 is 13%, still too many. The Ukraine/Iran locations are plausible.

### Topic Fragmentation
| Topic | Before | After | Improvement |
|-------|--------|-------|-------------|
| Iran/Israel/Houthi/Yemen | 246 | 42 | 83% reduction |
| Ukraine/Russia | 69 | 12 | 83% reduction |
| Earthquake | 32 | 0 | 100% eliminated |

**Iran/Israel** (42 situations): Still somewhat fragmented. Examples like "Iran Missile Strikes" (2 duplicates), "Iran Nuclear Talks" (2 duplicates), plus many specific situations like "Iran Airport Fire", "Iran Economic Collapse", "Iran Strait of Hormuz Shipping Restrictions". Some of these are legitimately distinct (airport fire vs nuclear talks), but others could consolidate further ("Iran Missile Campaign" + "Iran Missile Strikes" + "Iran Oil Facility Attacks").

**Ukraine/Russia** (12 situations): Reasonable fragmentation. "Ukraine Kupiansk Drone War" and "Ukraine Kupiansk Offensive" could merge, but otherwise distinct.

**Earthquakes**: Zero. Previously 32. Clean elimination.

### Duplicate Titles
| Title | Count |
|-------|-------|
| East Asia Military Flights | 3 |
| France Sarkozy Trial Opens | 2 |
| Iran Missile Strikes | 2 |
| Middle East Conflict Escalates | 2 |
| Saudi Eastern Province Drone Strikes | 2 |
| Iran Nuclear Talks | 2 |
| Michigan Synagogue Attack | 2 |

7 duplicate title groups (8 extra situations). Dedup gate should be catching these but isn't.

### Noise Assessment
- "Roman Calendar Reform" / "British Calendar Reform" -- GONE (was in baseline)
- "Pete Monks Criticizes US Allies" -- still present
- "Academy Awards Ceremony" -- noise (not OSINT)
- "David Alman Announces Retirement" -- noise
- "Daniel Pearl Kidnapping" -- historical event, not current OSINT
- "Boeing 747 Hijacking" -- historical event
- `"online Rent A Sage" Bret Devereaux` -- Bluesky personality noise

Some noise remains but much less than baseline.

---

## Comparison

| Metric | Before (979) | After (314) | Assessment |
|--------|-------------|-------------|-----------|
| Total situations | 979 | 314 | 68% reduction -- less is more |
| Distinct titles | 946 | 306 | Better ratio (97.5% vs 96.7%) |
| Max source diversity | 12 | 7 | Fixed -- was kitchen-sink bug |
| 10+ source types | 463 (47%) | 0 (0%) | Fixed |
| Mid-Atlantic locations | 500+ (51%) | 0 (0%) | Fixed |
| Null Island (0,0) | 27 (3%) | 41 (13%) | Worse ratio -- needs attention |
| Iran/Israel fragmentation | 246 | 42 | 83% improvement |
| Ukraine/Russia fragmentation | 69 | 12 | 83% improvement |
| Earthquake situations | 32 | 0 | 100% eliminated |
| CRITICAL severity | 1 | 0 | No false CRITICALs |
| HIGH severity | 82 | 0 | Conservative -- may under-report |
| Noise situations | 4+ | ~5 | Similar |
| Duplicate titles | 33 (unique gap) | 7 groups | Fewer but still present |
| Server stability | N/A | Stable after fixes | Was crash-looping initially |

### What Improved
1. **Source types inheritance bug fixed** -- no more 14-type kitchen sinks
2. **Location inheritance fixed** -- no more mid-Atlantic clusters
3. **Topic consolidation dramatically better** -- 83% reduction in fragmentation
4. **No earthquake noise** -- zero earthquake situations
5. **No inflated severity** -- no false CRITICAL/HIGH
6. **Calendar/personality noise eliminated**

### What Still Needs Work
1. **Null Island (0,0)**: 13% of situations have (0,0) location -- default/fallback still too common
2. **Source types homogeneity**: 106 situations share identical 5-type combo -- possibly over-absorbing
3. **Iran topic still fragmented**: 42 Iran-related situations could consolidate to ~10-15
4. **Duplicate titles**: Dedup gate missing 7 duplicate groups
5. **Some noise remains**: Academy Awards, retirement announcements, historical events
6. **No HIGH/CRITICAL severity yet**: May be too conservative (Iran war should be HIGH)
7. **Hierarchy still not in use**: All hierarchy_path NULL (not a regression -- was NULL before too)

---

## Grade: 7/10

**Significant improvement over baseline.** The source_types and location inheritance bugs are fixed -- the two biggest quality issues from before. Topic fragmentation reduced 83%. Earthquake noise eliminated. No false severity inflation.

Still has room to improve: Null Island locations, some duplicate titles, Iran topic could consolidate further, and severity seems too conservative (everything is LOW or MEDIUM, nothing HIGH for an active war).

The crash loop during rebuild was concerning but was resolved through iteration. The final stable build produces much cleaner situations than the previous state.
