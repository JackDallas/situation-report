# Situation Monitor — 2026-03-07

Post-wipe monitoring after centroid fix (only geo-reliable event types contribute to centroids).
Checking every ~1 minute for quality: titles, centroids, severity, garbage.

---

## Check 1 -- 22:39:08 UTC
- Total: 177 situations
- Empty titles: 3
- Garbage titles (Country: prefix): 1
  - "Country:australia: Earthquake In Papua New Guinea, Forest Fires In Australia (SOUTH-AMERICA)"
- With centroids: 175/177
- High/Critical (6):
  - [HIGH] "Syria Civil War" -- lat=34.7029, lon=36.9274 (OK)
  - [HIGH] "Iran Israel Escalating Conflict" -- lat=31.4487, lon=34.4289 (OK)
  - [HIGH] "Ukraine Russian Strikes" -- lat=47.8419, lon=36.6761 (OK)
  - [CRITICAL] "Ukraine Russian Barrage" -- lat=47.8419, lon=36.6761 (OK)
  - [HIGH] "Iran-US Middle East Conflict" -- lat=51.1657, lon=10.4515 (ME topic but coords (51.2,10.5) outside ME range -- this is GERMANY)
  - [HIGH] "Middle East Drone Strikes" -- NO CENTROID
- Centroid mismatches (49):
  - "Yemen Military Asset Movements" -- ME topic but coords (15.5,44.2) outside ME range
  - "Iran Supreme Leader Health" -- ME topic but coords (15.5,44.2) outside ME range
  - "Dubai Air Traffic Disruption" -- ME topic but coords (15.5,44.2) outside ME range
  - "Argentina Earthquake" -- Americas topic but coords (7.5,22.7) outside Americas range (AFRICA)
  - "Peru Earthquake" -- Americas topic but coords (5.5,-21.9) outside Americas range (AFRICA)
  - "Brazil Economic Reforms" -- Americas topic but coords (51.2,10.5) outside Americas range (GERMANY)
  - "Greece Court Ruling" -- EU topic but coords (6.8,18.6) outside EU range (AFRICA)
  - "Venezuela Wildfires" -- Americas topic but coords (6.8,18.6) outside Americas range (AFRICA)
  - ... and 39 more
- Duplicate titles (4): "Germany Middle East Policy Shift" x3, "Russia Earthquake" x2, "Israel-Lebanon Border Shelling" x2, "Iran Nuclear Diplomacy" x2
- **Key Issues**: Many centroids appear to be cluster centroids dominated by non-geo-reliable sources (GDELT news). "Iran-US Middle East Conflict" at HIGH severity is located in Germany (51.2N, 10.5E). 49 centroid mismatches total. Yemen coords (15.5,44.2) are slightly south of the ME bounding box (20-42N) used in check -- these may be acceptable.
- **Note**: Check 1 captured pre-wipe state (177 situations). Wipe occurred between checks 2 and 3.

## Check 2 -- 22:40:29 UTC (PRE-WIPE)
- Total: 177 situations
- Empty titles: 2
- Garbage titles (Country: prefix): 1
- With centroids: 175/177
- High/Critical (5):
  - [HIGH] "Yemen Naval and Artillery Activity" -- lat=15.4805, lon=44.2239 (Yemen coords OK)
  - [HIGH] "Ukraine Russian Strikes" -- lat=47.8419, lon=36.6761 (OK)
  - [HIGH] "Ukraine Russian Barrage" -- lat=47.8419, lon=36.6761 (OK)
  - [HIGH] "Iran-US Middle East Conflict" -- lat=51.1657, lon=10.4515 (GERMANY -- BAD)
  - [HIGH] "Middle East Drone Strikes" -- NO CENTROID
- Centroid mismatches: 49 (same as check 1)
- Duplicate titles: 4

## Check 3 -- 22:41:29 UTC (WIPE OCCURRED)
- Total: 1 situation
- Empty titles: 0
- Garbage titles: 0
- With centroids: 0/1
- **WIPE DETECTED** -- situation count dropped from 177 to 1

## Check 4 -- 22:42:29 UTC (REGENERATION STARTING)
- Total: 113 situations
- Empty titles: 97 (86% -- titles still generating)
- Garbage titles: 0
- With centroids: 51/113 (45%)
- High/Critical (2):
  - [HIGH] "Iran-Israel Military Escalation" -- NO CENTROID
  - [HIGH] "Asia in Unknown Region" -- NO CENTROID
- Centroid mismatches: 0

## Check 5 -- 22:43:29 UTC
- Total: 113 situations
- Empty titles: 84 (74% -- improving)
- Garbage titles: 0
- With centroids: 51/113
- High/Critical (2):
  - [HIGH] "Iran-Israel Military Escalation" -- NO CENTROID
  - [HIGH] "Iran Regional Conflict" -- NO CENTROID
- Centroid mismatches (3):
  - "Ukraine Military Aid Dispute" -- coords (51.6,-1.4) = UK, not Ukraine
  - "Gaza Humanitarian Crisis" -- coords (51.6,-1.4) = UK, not Gaza
  - "UK Middle East Military Operations" -- coords (51.6,-1.4) = UK, topic mentions ME but UK location is reasonable

## Check 6 -- 22:44:29 UTC
- Total: 113 situations
- Empty titles: 21 (19% -- most titles now populated)
- Garbage titles: 0
- With centroids: 51/113
- High/Critical (2):
  - [HIGH] "US-Iran Military Escalation" -- NO CENTROID
  - [HIGH] "Iran Regional Conflict" -- NO CENTROID
- Centroid mismatches: 19 -- many "Middle East" topic situations clustered at (51.6,-1.4) = UK coords
- Duplicate titles: "Iran Nuclear Negotiations" x4
- **Pattern**: The (51.6,-1.4) coordinate is recurring -- likely a GDELT/news cluster centroid from UK-based news sources

## Check 7 -- 22:45:29 UTC
- Total: 113 situations
- Empty titles: 5 (4% -- titles nearly complete)
- Garbage titles: 0
- With centroids: 51/113
- High/Critical: same as check 6
- Centroid mismatches: 23
- Stable state -- no new situations being created

## Check 8-10 -- 22:46:29 to 22:48:29 UTC
- Total: 113 situations (stable)
- Empty titles: 5 (stable)
- Garbage titles: 0
- With centroids: 51/113 (stable)
- High/Critical: 2 (same, both NO CENTROID)
- Centroid mismatches: 23 (stable)
- **No changes across 3 checks**

## Check 11 -- 22:49:30 UTC (NEW SITUATIONS APPEARING)
- Total: 121 situations (+8 new)
- Empty titles: 16 (new untitled situations appearing)
- Garbage titles: 3 (new GDACS garbage titles appeared):
  - "Country:cameroon: Forest Fires In Cameroon, Forest Fires In Central African Republic (GLOBAL)"
  - "Country:afghanistan: Earthquake In Afghanistan, Earthquake In Argentina (SOUTHEAST-ASIA)"
  - "Country:australia: Earthquake In Drake Passage, Earthquake In Mid Indian Ridge (GLOBAL)"
- With centroids: 60/121
- Centroid mismatches: 20
- **New issue**: GDACS source producing garbage "Country:" prefix titles with wrong region labels (Afghanistan+Argentina tagged SOUTHEAST-ASIA)

## Check 12-14 -- 22:50:30 to 22:52:30 UTC
- Total: 127 situations (grew from 121 to 127)
- Empty titles: 21 (stable)
- Garbage titles: 3 (same GDACS garbage)
- With centroids: 65/127
- High/Critical: 2 (same, both NO CENTROID)
- Centroid mismatches: 20 (stable)

## Check 15 -- 22:53:30 UTC (FINAL)
- Total: 127 situations
- Empty titles: 20
- Garbage titles: 3
- With centroids: 65/127 (51%)
- High/Critical (2):
  - [HIGH] "US-Iran Military Escalation" -- NO CENTROID
  - [HIGH] "Iran Regional Conflict" -- NO CENTROID
- Centroid mismatches: 20
- Duplicate titles: "Iran Nuclear Negotiations" x4, "Iran Regional Conflict" x2, "Gaza Humanitarian Crisis" x2

---

## Summary -- 15-Minute Monitoring Report

### Timeline
- **22:39-22:40** (Checks 1-2): Pre-wipe state with 177 situations
- **22:41** (Check 3): Wipe detected -- dropped to 1 situation
- **22:42** (Check 4): Rapid regeneration -- 113 situations appeared, 86% without titles
- **22:42-22:45** (Checks 4-7): Title generation progressed from 86% empty to 4% empty
- **22:45-22:48** (Checks 7-10): Stable at 113 situations -- pipeline paused or caught up
- **22:49-22:53** (Checks 11-15): Growth resumed, reached 127 situations

### Quality Assessment

**Titles:**
- Post-regeneration title quality improved: empty titles dropped from 97 to 20
- 3 garbage "Country:" prefix titles from GDACS source (e.g., "Country:afghanistan: Earthquake In Afghanistan, Earthquake In Argentina (SOUTHEAST-ASIA)")
- Duplicate titles persist: "Iran Nuclear Negotiations" appears 4 times
- Some titles are nonsensical: "Asia in Unknown Region"

**Centroids:**
- Only 51% of situations have centroids (65/127) -- significant gap
- Both HIGH severity situations lack centroids entirely
- Recurring bad centroid: (51.6, -1.4) = UK coords assigned to many Middle East topic situations
- This UK coordinate appears on 20+ situations about Iran, Gaza, Ukraine, etc.
- Root cause likely: GDELT news events with UK-based source coordinates being used as situation centroids

**Severity:**
- Pre-wipe had 6 high/critical situations; post-wipe only 2 high, 0 critical
- Both high-severity situations lack centroids, making them invisible on map

### Actionable Issues

1. **GDACS garbage titles**: The "Country:" prefix format with wrong region labels needs to be cleaned in the GDACS source parser
2. **UK centroid contamination**: (51.6, -1.4) is being assigned to non-UK situations -- the centroid-only-from-geo-reliable-sources fix may not be fully working, or GDELT geo coordinates include UK publisher locations
3. **Missing centroids on 49% of situations**: Many situations (especially news-only clusters) have no geo-reliable events and thus get no centroid
4. **Duplicate situation titles**: Deduplication or merging rules not catching "Iran Nuclear Negotiations" x4
5. **High-severity situations without centroids**: These are invisible on the map despite being the most important

---

# Round 2 — Post centroid + title fixes

## Round 2 — Check 1 — 23:00:39 UTC
- Total: 1 situation
- Empty titles: 0
- Garbage titles: 1
  - [HIGH] "Balkans-caucasus-diplomacy: Annalena Baerbock, Bellingcat (NORTH-AMERICA)" — raw tag format, no centroid
- With centroids: 0/1
- UK-coord situations: 0
- High/Critical: 1 (the garbage-titled one above, no centroid)
- **Note**: Clean slate just starting. Only 1 situation created so far with 74 events. Title is garbage — looks like raw GDELT theme/entity dump rather than a proper title.

## Round 2 — Check 2 — 23:02:01 UTC
- Total: 119 situations (rapid generation)
- Empty titles: 98 (82% — titles still generating)
- Garbage titles: 2
  - "Gulf-cooperation-council-digital-surveillance: Dubai, Iran (MIDDLE-EAST)"
  - "European-population-decline: Germany (WESTERN-EUROPE)"
- With centroids: 0/119 (zero centroids so far!)
- UK-coord situations: 0
- High/Critical: 3
  - [HIGH] "Iran-Israel Military Escalation" — no centroid
  - [HIGH] "Attack in Unknown Region" — no centroid
  - [HIGH] "Climate-Driven Resource Scarcity Crisis" — no centroid
- **Note**: 119 situations created but ZERO have centroids yet. Titles still populating. Some good titles appearing: "Ukraine Kharkiv Russian Strikes", "Israel-Hezbollah Border War". No UK coords seen yet.

## Round 2 — Check 3 — 23:03:24 UTC
- Total: 119 situations (stable)
- Empty titles: 43 (36% — down from 82%, titles filling in)
- Garbage titles: 0 (the 2 from check 2 appear to have been re-titled)
- With centroids: 0/119 (still zero!)
- UK-coord situations: 0
- High/Critical: 3
  - [HIGH] "Iran-Israel Military Escalation" — no centroid
  - [HIGH] "Gulf of Aden Missile Strikes" — no centroid
  - [HIGH] "Climate-Driven Resource Scarcity Crisis" — no centroid
- **Note**: Title generation progressing well (64% now titled). Still zero centroids across all 119 situations — centroid computation may be delayed or waiting for geo-reliable events.

## Round 2 — Check 4 — 23:04:46 UTC
- Total: 119 situations (stable)
- Empty titles: 6 (5% — titles nearly complete)
- Garbage titles: 0
- With centroids: 0/119 (still zero — 3 checks with no centroids)
- UK-coord situations: 0
- High/Critical: 3 (unchanged)
  - [HIGH] "Iran-Israel Military Escalation" — no centroid
  - [HIGH] "Gulf of Aden Missile Strikes" — no centroid
  - [HIGH] "Climate-Driven Resource Scarcity Crisis" — no centroid
- **Concern**: Titles are 95% populated but zero centroids after 4 checks. In Round 1, 45% had centroids by check 4. The centroid fix may be too aggressive — filtering out all non-geo-reliable sources may leave no events to compute centroids from.

## Round 2 — Checks 6-9 — 23:08:34 to 23:11:46 UTC
- Total: 119 (stable across all 4 checks)
- Empty titles: 6 (stable)
- Garbage titles: 0
- With centroids: 0/119 (unchanged)
- UK-coord situations: 0
- High/Critical: 3 (unchanged — Iran-Israel, Gulf of Aden, Climate)
- `situation_events` table: still 0 rows at check 6
- **State is completely frozen.** No new situations, no centroids appearing, no title changes.

## Round 2 — Checks 10-15 — 23:13:01 to 23:18:08 UTC
- Total: 119 (stable across all 6 checks)
- Empty titles: 6 (stable — 113 titled)
- Garbage titles: 0
- With centroids: 0/119 (unchanged — zero throughout entire monitoring period)
- UK-coord situations: 0
- High/Critical: 3 (unchanged)
  - [HIGH] "Iran-Israel Military Escalation" — no centroid, 149 events
  - [HIGH] "Gulf of Aden Missile Strikes" — no centroid, 27 events
  - [HIGH] "Climate-Driven Resource Scarcity Crisis" — no centroid, 11 events
- `situation_events` table: still 0 rows at check 15
- Duplicate titles: 4 pairs — "Iran Regional Conflict" x2, "Germany Coalition Government Collapse" x2, "ECB Interest Rate Decision" x2, "Germany Political Leadership Transition" x2
- **State completely frozen from check 4 onward (11 consecutive identical checks).**

---

## Round 2 vs Round 1 — Comparison Summary

### What improved in Round 2:

1. **UK coordinate contamination: ELIMINATED.** The (51.6, -1.4) UK coords that appeared on 20+ non-UK situations in Round 1 were never observed in Round 2. Zero UK-coord situations across all 15 checks. This was the primary validation target and it passed.

2. **Garbage titles reduced.** Round 1 ended with 3 persistent "Country:" prefix garbage titles from GDACS. Round 2 had 2 transient garbage titles (raw GDELT theme format like "Gulf-cooperation-council-digital-surveillance: Dubai, Iran (MIDDLE-EAST)") that disappeared by check 3, and zero persistent garbage titles.

3. **Title quality improved.** Round 2 titles are cleaner and more descriptive. No "Asia in Unknown Region" type nonsense. "Attack in Unknown Region" appeared briefly at HIGH severity but was replaced by "Gulf of Aden Missile Strikes" — the system self-corrected.

4. **Fewer duplicate titles.** Round 1 had "Iran Nuclear Negotiations" x4; Round 2 has 4 duplicate pairs (all x2), a modest improvement.

### What regressed in Round 2:

1. **ZERO centroids.** This is the critical regression. Round 1 achieved 51% centroid coverage (65/127). Round 2 has 0% (0/119). The `situation_events` junction table has zero rows, meaning no events are being linked to situations. Without event linkage, the centroid computation (which now filters for geo-reliable sources) has nothing to compute from.

2. **No situation growth.** Round 1 grew from 113 to 127 situations over 15 minutes. Round 2 stayed locked at 119 from check 2 onward (13 minutes with no new situations).

3. **Fewer situations overall.** 119 vs 127 in Round 1.

### Root cause of centroid failure:

The `situation_events` table has 0 rows. Events exist in the `events` table (339 FIRMS, 27 USGS, 76 NOTAM with location data) but none are linked to situations. The centroid fix correctly filters for geo-reliable sources, but it is operating on an empty set because the event-to-situation linkage is broken. This is likely a separate bug from the centroid computation logic — the pipeline is creating situations and generating titles (via embeddings/clustering) but not populating the `situation_events` join table.

### Verdict:

The title fix and UK-coord fix both work. The centroid fix is correct in principle but is masked by a deeper issue: `situation_events` is empty, so all situations are invisible on the map. This needs investigation — the event-to-situation linkage must be restored before centroid quality can be evaluated.

## Round 2 — Check 5 — 23:06:09 UTC
- Total: 119 (stable)
- Empty titles: 6 (stable)
- Garbage titles: 0
- With centroids: 0/119 (still zero — 5 checks, ~6 minutes)
- UK-coord situations: 0
- High/Critical: 3 (unchanged)
- **No change from check 4.** Situation count and titles stable. Centroids remain at zero.
- **DB investigation**: `situation_events` table has 0 rows. No events are linked to situations, so centroid computation has nothing to work from. The events table has plenty of geo-reliable data (339 FIRMS, 27 USGS, 76 NOTAM, 20k+ aviation) but none are linked to situations.

---

# Round 3 — Two-tier centroid (geo-reliable preferred, fallback to any coords) + fixed NOTAM circles

## Round 3 — Check 1 — 23:23:58 UTC
- Total situations: 110
- Empty titles: 97 (88% — regeneration just started, titles still generating)
- Garbage titles: 0
- **Centroid coverage: 109/110 (99%)**
- UK coords on non-UK situations: 50
  - Pattern: (51.77,-1.09) assigned to 50 untitled situations — likely NOTAM Brize Norton coords bleeding into everything
- High/Critical: 2
  - [HIGH] (31.05,34.85) "Balkans-caucasus-diplomacy: Bellingcat, Bundeswehr (MIDDLE-EAST)" — garbage title, centroid in Israel region
  - [HIGH] NO CENTROID: "Attack in Unknown Region"
- Region mismatches: 2
  - Ukraine topic at (47.16,19.50) — Hungary coords
  - ME topic "UK Iran Espionage Crackdown" at (55.38,-3.44) — Scotland coords (arguably OK for UK-focused title)
- Duplicate titles: 0
- **Early assessment**: Centroid coverage is back to 99% (vs 0% in Round 2, 51% in Round 1). But 50 situations have UK NOTAM coords (51.77,-1.09) — the fallback tier may be pulling in NOTAM coords when no geo-reliable events exist. Titles still generating.

## Round 3 — Check 2 — 23:25:46 UTC
- Total: 110 (stable)
- Empty titles: 48 (44% — down from 88%, titles generating fast)
- Garbage titles: 0
- **Centroid coverage: 109/110 (99%)**
- UK coords on non-UK situations: 47
  - Examples: "German Catholic Church Abuse Scandal", "Iran Nuclear Negotiations Stalled", "Albania Police Operations", "Kinahan Cartel Operations", "Russia Olympic Ban" — all at (51.77,-1.09)
- High/Critical: 2
  - [HIGH] (31.05,34.85) "Iran-Israel Military Escalation" — Israel coords, reasonable
  - [HIGH] NO CENTROID: "Iran Drone Strikes Middle East"
- Region mismatches: 7 (Ukraine topics at UK coords, Iran topics at UK coords, Hungary coords on Ukraine topic)
- Duplicate titles: 1 ("Iran Regional Conflict" x2)

## Round 3 — Check 3 — 23:26:59 UTC
- Total: 110 (stable)
- Empty titles: 1 (titles nearly complete!)
- Garbage titles: 0
- **Centroid coverage: 109/110 (99%)**
- UK coords on non-UK situations: 42
- High/Critical: 2 (unchanged)
- Region mismatches: 11 (includes "Mexico City Protests" at Israel coords (31.05,34.85), "Germany Military Aid Ukraine" at Israel coords)
- Duplicate titles: 1

## Round 3 — Check 4 — 23:28:15 UTC
- Total: 110 (stable)
- Empty titles: 1
- Garbage titles: 0
- **Centroid coverage: 109/110 (99%)**
- UK coords on non-UK situations: 42
- State identical to Check 3 — fully stabilized at 110 situations.

## Round 3 — Check 5 — 23:30:09 UTC
- Total: 110 (stable)
- Empty titles: 1
- Garbage titles: 0
- **Centroid coverage: 109/110 (99%)**
- UK coords on non-UK situations: 42
- Top centroid clusters: (51.77,-1.09) x50, (31.05,34.85) x49, then 9 unique coords
- **Only 11 unique centroid values across 109 geolocated situations** — extreme centroid clustering.

## Round 3 — Check 6 — 23:31:19 UTC
- Total: 158 (+48 new situations!)
- Empty titles: 49 (new batch still generating titles)
- Garbage titles: 0
- **Centroid coverage: 157/158 (99%)**
- UK coords on non-UK situations: 39
- New centroid cluster appeared: (6.96,21.86) x40 — Chad/CAR region (likely FIRMS fire detections)
- Top clusters: (31.05,34.85) x48, (51.77,-1.09) x45, (6.96,21.86) x40
- High/Critical: 2 (unchanged from original batch)

## Round 3 — Check 7 — 23:32:35 UTC
- Total: 166 (+8 more)
- Empty titles: 54
- Garbage titles: 0
- **Centroid coverage: 164/166 (99%)**
- UK coords on non-UK situations: 40
- New centroid cluster: (18.87,97.33) x10 — Myanmar/Thailand (FIRMS fires)
- Top clusters: (31.05,34.85) x47, (51.77,-1.09) x46, (6.96,21.86) x45, (18.87,97.33) x10
- High/Critical: 2 (unchanged)

## Round 3 — Check 8 — 23:33:52 UTC
- Total: 182 (+16 more — steady growth)
- Empty titles: 66
- Garbage titles: 0
- **Centroid coverage: 180/182 (99%)**
- UK coords on non-UK situations: 40
- New centroid cluster: (15.48,44.22) x8 — Yemen (correct for Yemen situations)
- High/Critical now 6:
  - [HIGH] (13.49,2.18) "Sahel: Sahel (AFRICA)" — Niger coords, reasonable for Sahel
  - [HIGH] (13.49,2.18) "Earthquake: Chile/China (CENTRAL-ASIA)" — garbage GDACS title at Niger coords, WRONG
  - [HIGH] (15.48,44.22) "Yemen: Yemen (MIDDLE-EAST)" — correct
  - [HIGH] (47.80,37.09) "Ukraine Kharkiv Russian Strikes" — correct Kharkiv coords
  - [HIGH] (31.05,34.85) "Iran-Israel Military Escalation" — Israel coords, reasonable
  - [HIGH] NO CENTROID: "Iran Drone Strikes Middle East"

## Round 3 — Check 9 — 23:35:05 UTC
- Total: 184 (near stable)
- Empty titles: 69
- Garbage titles: 0
- **Centroid coverage: 182/184 (99%)**
- UK coords on non-UK situations: 40
- High/Critical: 7 — Ukraine Kharkiv elevated to CRITICAL
  - [CRITICAL] (47.80,37.09) "Ukraine Kharkiv Russian Strikes" — correct coords
  - Rest unchanged from Check 8

## Round 3 — Check 10 — 23:36:21 UTC (FINAL)
- Total: 184 (stable)
- Empty titles: 67 (36% — many new situations still waiting for titles)
- Garbage titles: 0
- **Centroid coverage: 182/184 (99%)**
- UK coords on non-UK situations: 40
- **Unique centroid values: only 25 across 182 geolocated situations**
- Top centroid clusters:
  - (31.05,34.85) x49 — Israel (GDELT news cluster)
  - (51.77,-1.09) x46 — Brize Norton UK (NOTAM fallback)
  - (6.96,21.86) x43 — Chad/CAR (FIRMS fires)
  - (18.87,97.33) x10 — Myanmar/Thailand (FIRMS fires)
  - (15.48,44.22) x8 — Yemen (GDELT/FIRMS)
- High/Critical: 7
  - [CRITICAL] (47.80,37.09) "Ukraine Kharkiv Russian Strikes" — CORRECT
  - [HIGH] (31.05,34.85) "Iran-Israel Military Escalation" — CORRECT
  - [HIGH] (15.48,44.22) "Yemen: Yemen (MIDDLE-EAST)" — CORRECT
  - [HIGH] (13.49,2.18) "Sahel: Sahel (AFRICA)" — CORRECT (Niger/Sahel region)
  - [HIGH] (13.49,2.18) "Earthquake: Chile/China (CENTRAL-ASIA)" — WRONG (garbage GDACS title, Niger coords for Chile/China earthquake)
  - [HIGH] (47.80,37.09) untitled — likely Ukraine, correct coords
  - [HIGH] NO CENTROID: "Iran Drone Strikes Middle East"
- Region mismatches: 10
  - 5 are ME/Iran/Gaza topics at UK coords (51.77,-1.09)
  - 3 are Ukraine topics at UK coords (51.77,-1.09)
  - 2 are Americas topics at Israel coords (31.05,34.85)
- Duplicate titles: 1 ("Iran Regional Conflict" x2)
- Cluster sample titles:
  - Israel cluster: "FIFA World Cup 2022", "World Cup 2026 Host Preparations", "Israel Air Force Strikes" — mixed relevance
  - UK cluster: "German Catholic Church Abuse Scandal", "Ukraine Ambassador Expelled" — mostly wrong
  - Chad cluster: "Australia-middle-east-policy: Angus Taylor..." — wrong, GDACS garbage

---

## Round 3 — Comparison Summary vs Rounds 1 and 2

### Centroid Coverage

| Metric | Round 1 | Round 2 | Round 3 |
|--------|---------|---------|---------|
| Final total situations | 127 | 119 | 184 |
| Centroid coverage | 51% (65/127) | 0% (0/119) | **99% (182/184)** |
| Unique centroid values | ~20 | 0 | 25 |
| UK coords on non-UK | 20+ | 0 | **40** |
| High/Critical count | 2 | 3 | **7** |
| High/Crit with centroids | 1 | 0 | **6/7 (86%)** |
| High/Crit centroids correct | 0/1 | N/A | **4/6 correct** |
| Garbage titles | 3 | 0 | 0 |
| Empty titles (final) | 20 (16%) | 6 (5%) | 67 (36%) |
| Duplicate title groups | 4 | 4 | 1 |

### What Round 3 Fixed

1. **Centroid coverage restored**: 99% vs 0% in Round 2. The two-tier approach (geo-reliable preferred, fallback to any coords) successfully restored map visibility.
2. **High/critical situations have centroids**: 6 of 7 high/critical situations have centroids, and 4 of those 6 are geographically correct (Ukraine at Kharkiv, Iran-Israel at Israel, Yemen at Yemen, Sahel at Niger).
3. **More situations generated**: 184 vs 119-127 in previous rounds.
4. **Garbage titles eliminated**: Zero "Country:" prefix garbage from GDACS.

### What Round 3 Did NOT Fix (Persistent Issues)

1. **UK NOTAM contamination is WORSE**: 40 non-UK situations have Brize Norton coords (51.77,-1.09) — up from 20+ in Round 1. The fallback tier is pulling NOTAM coords when no geo-reliable events exist. This means Iran, Ukraine, Germany, Russia situations all appear clustered at a UK RAF base on the map.

2. **Extreme centroid clustering**: Only 25 unique centroid values across 182 situations. The top 3 coordinates (Israel, UK NOTAM, Chad FIRMS) account for 138/182 = 76% of all geolocated situations. The map would show 3-5 giant clusters instead of a meaningful global distribution.

3. **FIRMS/GDACS centroid attractors**: The Chad cluster (6.96,21.86) x43 and Myanmar cluster (18.87,97.33) x10 are FIRMS fire detection coordinates. Situations about "Australia Middle East Policy" or "FIFA World Cup" are placed at active fire locations because those are the only geo-reliable events in the cluster.

4. **Root cause**: The fallback tier is too aggressive. When a situation has no geo-reliable events (news-only clusters), it falls back to ANY event with coordinates — and those are typically NOTAM or FIRMS events that happened to be in the same temporal/embedding cluster but have no topical relationship to the situation.

### Recommendations

1. **Filter NOTAM from centroid fallback**: NOTAM events (especially the persistent Brize Norton one) should not be used as fallback centroids. They are geographic markers for airspace notices, not topic-relevant locations.
2. **Require topical coherence for centroid**: Before using a geo event's coords as a situation centroid, verify the event is actually related to the situation topic (e.g., via embedding similarity threshold).
3. **Consider "no centroid is better than wrong centroid"**: Round 2's 0% coverage was bad, but 40 wrong centroids in Round 3 is arguably worse — it actively misleads the map user. A middle ground: only assign centroids when confidence is above a threshold.
4. **GDACS title cleanup**: Titles like "Earthquake: Earthquake In Chile, Earthquake In China (CENTRAL-ASIA)" and "Sahel: Sahel (AFRICA)" need the raw tag prefix stripped.

