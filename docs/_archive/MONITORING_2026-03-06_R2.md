# Monitoring Log — Round 2 (2026-03-06 13:10 UTC)

## Deployment Summary
Deploy time: 2026-03-06 13:09 UTC

### Changes Deployed (6 fixes)
1. **Severity propagation** — proportional threshold (34% of children must agree) instead of max(). Allows decrease.
2. **Orphan promotion quality gate** — orphaned children must pass standalone quality gate (min events + 2 source types)
3. **Wildfire mega-cluster fixes** — relaxed cross-region merge for natural disasters (0.65 vs 0.80), ND cap at 2 top-level
4. **Source error handling** — AuthError type, immediate disable on auth errors, exponential backoff to 30min max, max failures before park
5. **Title contamination** — strengthened is_garbage_title(), num_predict=50 cap on Ollama, coherence_min raised 0.45→0.55
6. **Topic-diversity split** — clusters with 8+ topics trigger coherence split

### Config Changes
- `severity_propagation_threshold: 0.34`
- `severity_propagation_allow_decrease: true`
- `topic_diversity_split_threshold: 8`
- `coherence_min: 0.55` (was 0.45)

---

## T+0 Baseline (13:12 UTC)
| Metric | Value |
|--------|-------|
| Total situations | 295 |
| Top-level | 55 |
| Children | 240 |
| Severity (top-level) | critical:26, high:23, medium:6 |
| Phases (all) | active:155, declining:116, developing:19, emerging:5 |
| Resolved | 0 |
| Garbage titles | 2 |
| Budget spent | $8.03/$10 |

### Immediate Effects on Startup
- 2 parents severity **lowered** (Critical→Medium) via proportional threshold
- 113 orphans detached via child cap enforcement
- 1 grandparent detached to top-level
- 77 orphans removed in first sweep
- 2 topic-diversity splits triggered (DRC Wildfires, East Africa Drought — both 15 topics)
- 7 clusters entered Declining phase

---

## Monitoring Timeline

### T+5min (13:15 UTC)
Total:321 Top:54 Sev:{crit:25, high:23, med:6} Phase:{active:177, declining:117, developing:20, emerging:7}
**BUG FOUND**: Severity oscillation — `recompute_cluster_severity` raises parent to Critical (own events), then proportional propagation lowers it. Repeats every 30s on same 3 clusters.
**HOTFIX**: Set `severity_propagation_allow_decrease: false`. Redeployed at 13:20 UTC. Oscillation stopped.

### T+15min (13:25 UTC)
Total:317 Top:57 Sev:{crit:29, high:24, med:4} Phase:{active:154, declining:125, developing:33, emerging:5}
Garbage titles: 1 (down from 2). 22 narratives generated in 5min. No resolves yet. No severity oscillation.
Orphan sweep removed 1 more. Topic pruning active (22 topics + 7 entities pruned).

### T+30min (13:40 UTC)
Total:296 Top:46 Sev:{crit:21, high:21, med:4} Phase:{active:121, declining:129, developing:40, emerging:6}
ReliefWeb 403 detected as auth error but source NOT parked (bug: poll() swallowed the error).
AIS WebSocket resetting — consecutive_failures=5, backoff=320s. Expected to park eventually.
**HOTFIX 2**: Fixed reliefweb.rs poll() to propagate AuthError instead of swallowing it. Redeployed at 13:45 UTC.

### T+60min (13:55 UTC)
Total:297 Top:57 Sev:{crit:30, high:19, med:8} Phase:{active:135, declining:124, developing:32, emerging:5, **resolved:1**}
**First resolution!** 7 Declining→Resolved transitions since startup.
ReliefWeb properly parked: "Poll auth error — source disabled until restart" — no more 403 spam.
Budget: $8.11/$10. Garbage titles: 1.

### T+120min (15:10 UTC)
_pending_

### T+180min (16:10 UTC)
_pending_

### T+240min (17:10 UTC)
_pending_

### T+360min (19:10 UTC)
_pending_

### T+480min (21:10 UTC)
_pending_

### T+600min (23:10 UTC)
_pending_

### T+720min (01:10 UTC +1)
_pending_
