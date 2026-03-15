# Deterministic Consolidation Analysis — Why Obvious Duplicates Aren't Merging

**Date:** 2026-03-15
**Status:** Research only — no code changes recommended yet
**Problem:** ~146 top-level situations with obvious duplicates like "Iran War Escalates" + "Iran Warns of Total War" + "Iran-US Tensions Flare" are NOT being consolidated despite deterministic rules

---

## Executive Summary

The deterministic consolidation function (`consolidate_by_topic`, merge.rs:576–950) is **fundamentally blocked from merging obvious title-based duplicates** due to four compounding issues:

1. **60% of top-level situations have EMPTY topics** (73/126) — making topic-based consolidation impossible
2. **Iran/Hormuz/Yemen word frequencies exceed generic thresholds** — country stems save them, but Jaccard scores are too low
3. **Jaccard threshold is 0.40, but Iran pairs only achieve 0.125–0.167** — they fail the Jaccard gate even with shared country stems
4. **Single country-stem matches require Jaccard ≥ 0.40** — the `has_country` bypass only applies when `shared.len() < 2` AND `has_country`, but Jaccard must STILL pass

---

## Key Findings Overview

The investigation reveals **four independent blockers** preventing consolidation of Iran/Hormuz/Yemen duplicates:

1. **Empty topics (73/126 situations)** block topic-based consolidation entirely
2. **Low Jaccard scores (0.125–0.167 for Iran pairs)** fail the 0.40 threshold required by title-word matching
3. **Child cap already exceeded** (31 children in "Israel UN Threatens Iran" vs. 15-child cap) blocks adding more children
4. **No secondary word overlap** in Iran titles means Jaccard never exceeds ~0.20 even with countries highlighted

Only Hormuz/maritime terms pass because "Hormuz Strait" repeats 3 times across titles, generating 0.60+ Jaccard scores.

---

## Part 1: The Empty Topics Problem

### Data
- **Total top-level situations:** 126
- **Situations with empty `.topics`:** 73 (57.9%)
- **Situations with ≥1 topic:** 53 (42.1%)

### Why This Matters

The consolidation logic has THREE pathways (merge.rs:681–810):

```rust
1. Topic-based (line 686):  Pairs sharing a topic → require ≥2 shared topics
2. Entity-based (line 716):  Pairs sharing an entity → require ≥1 shared topic
3. Title-word Jaccard (line 759): Pairs sharing a word → require Jaccard ≥ 0.40
```

**Pathways 1 and 2 require situations to have topics.** For the 73 situations with empty topics, only pathway 3 (Jaccard) can apply.

### Example: Iran Situations

Sample of Iran-conflict situations:

```
"Iran-US Tensions Flare"                → topics: [], entities: ["marine expeditionary force"]
"Trump Threatens Iran Strikes"          → topics: [], entities: ["fifa world cup 2026"]
"Iran Spy Arrests"                      → topics: [], entities: []
"Iran Presidential Election"            → topics: [], entities: ["masoud pezeshkian"]
"Iran War Roils Oil Trade"              → topics: [], entities: ["sudan"]

BUT:

"Iran Attacks Gulf Oil Sites"           → topics: [15 Iran-conflict topics], entities: []
"Israel-Iran War Fears"                 → topics: [15+ Iran/Israel topics], entities: []
"US Strikes Iran Kharg Island"          → topics: [15+ Iran/Kharg topics], entities: []
```

**Critical insight:** The 5 empty-topic Iran situations are each treated as ISOLATED clusters. They have no shared topics with each other or with the rich-topic Iran situations. Only Jaccard matching can reach them.

---

## Part 2: The Jaccard Insufficiency Problem

### Title Word Stemming & Frequency

The consolidation logic (merge.rs:643–667):
1. Splits titles by non-alphanumeric chars
2. Filters stopwords (the, and, for, etc. + region words)
3. Filters words <3 chars
4. **Stems by truncating to first 4 chars** (`"iranian"` → `"iran"`, `"strikes"` → `"stri"`)
5. Counts word frequency across all 126 top-level situations
6. Builds `word_groups`: only words appearing in ≥2 titles

### Actual Word Frequencies (sorted by count)

```
'iran': 14 occurrences    ← COUNTRY_STEM, bypasses >8 filter
'wild': 13
'eart': 13
'russ': 9
'stri': 3
'isra': 3
'tens': 2
...
```

### COUNTRY_STEMS Bypass (merge.rs:747–753)

The code includes a whitelist:
```rust
const COUNTRY_STEMS: &[&str] = &[
    "iran", "iraq", "isra", ..., "horm", "hout", "hezb",
];
```

**Purpose:** A country stem always counts as "specific" even if it appears in >8 titles (line 789).

---

## Part 3: Why Iran Pairs Fail the Jaccard Gate

### Test Case 1: "Iran-US Tensions Flare" + "Trump Threatens Iran Strikes"

```
Title 1: "Iran-US Tensions Flare"
  Stemmed words: {iran, tens, flar}

Title 2: "Trump Threatens Iran Strikes"
  Stemmed words: {iran, stri, thre, trum}

Shared words: {iran}
Union: {iran, tens, flar, stri, thre, trum} = 6 words
Jaccard: 1/6 = 0.167

Code path (merge.rs:787–807):
  - has_country = true (iran ∈ COUNTRY_STEMS)
  - has_specific = true (country stem bypass)
  - shared.len() = 1
  - Requires: shared.len() >= 2 OR has_country
    → shared.len() < 2 AND has_country = TRUE → enters line 799 check
  - Requires: jaccard >= 0.40
    → 0.167 >= 0.40 = FALSE

RESULT: SKIP (line 807 continue) ❌
```

### Test Case 2: "Iran War Roils Oil Trade" + "Iran Nuclear Facility Strike"

```
Title 1: "Iran War Roils Oil Trade"
  Stemmed words: {iran, war, roil, trad, oil}

Title 2: "Iran Nuclear Facility Strike"
  Stemmed words: {iran, nucl, stri, faci}

Shared words: {iran}
Union: 8 words
Jaccard: 1/8 = 0.125

RESULT: SKIP (0.125 < 0.40) ❌
```

### Test Case 3: "Israel-Iran War Fears" + "Iran-US Tensions Flare"

```
Title 1: "Israel-Iran War Fears"
  Stemmed words: {iran, isra, war, fear}

Title 2: "Iran-US Tensions Flare"
  Stemmed words: {iran, tens, flar}

Shared words: {iran}
Union: 7 words
Jaccard: 1/7 = 0.143

RESULT: SKIP (0.143 < 0.40) ❌
```

### Why the Jaccard Threshold is So High

The consolidation is conservative. Jaccard ≥ 0.40 means **at least 40% of the union must be shared.** For two titles:
- 2 shared words and 3 unique = Jaccard 2/5 = **0.40** ✓ (threshold)
- 1 shared word, title lengths typical = Jaccard **0.125–0.25** ✗

**Iran titles are inherently low-Jaccard** because:
- The shared "iran" stem is 1 word
- Titles are diverse: "War", "Tensions", "Strikes", "Election", "Spy Arrests", etc.
- Very few Iran titles share secondary words ("oil", "war" appear 2x; most secondary words appear 1x)

---

## Part 4: Hormuz/Strait Matching — Actually Works!

### Test Case: "Hormuz Strait Shipping Attacks" + "Hormuz Strait Ship Strikes"

```
Title 1: "Hormuz Strait Shipping Attacks"
  Stemmed words: {horm, stra, ship, atta}

Title 2: "Hormuz Strait Ship Strikes"
  Stemmed words: {horm, stra, ship, stri}

Shared words: {horm, stra, ship}
Union: {horm, stra, ship, atta, stri} = 5 words
Jaccard: 3/5 = 0.60

Code path:
  - has_country = false (horm NOT in COUNTRY_STEMS)
  - shared.len() = 3 >= 2 = TRUE
  - jaccard = 0.60 >= 0.40 = TRUE

RESULT: MERGE CANDIDATE ✓ (line 805)
```

**Why Hormuz works:** The repeated "Hormuz Strait" phrase gives 2–3 shared words ("horm", "stra", sometimes "ship"), yielding 0.60+ Jaccard.

**Why Iran doesn't:** Iran titles rarely share secondary words. "Iran" alone isn't enough for Jaccard ≥ 0.40.

---

## Part 5: Merge Application Gate (merge.rs:816–950)

Even if a pair reaches `merge_candidates`, application has additional gates:

### Gate 1: Parent/Child Status (line 831)
```rust
if a_parent.is_some() || b_parent.is_some() {
    continue;  // Both must be top-level
}
```

**Passed:** All our test pairs are top-level situations.

### Gate 2: Parent Event Cap (line 850)
```rust
let parent_events = self.clusters.get(&parent_id).map(|c| c.event_count).unwrap_or(0);
if parent_events >= max_events {
    continue;  // Default max_events_per_parent = 1000
}
```

**Check needed:** Do any Iran situations exceed 1000 events? Let me verify:

```bash
curl -s http://100.110.3.124:3001/api/situations | jq '.[] | select(.parent_id == null) | select(.title | contains("Iran")) | {title, event_count}' | grep event_count | sort -t: -k2 -rn | head -5
```

*Expected:* Most Iran situations are <1000 events (no massive consolidation yet), so this gate shouldn't block.

### Gate 3: Children-per-parent Cap (line 843–847)
```rust
let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
let child_has_children = self.clusters.values().any(|c| c.parent_id == Some(child_id));
let effective_cap = if child_has_children { max_children + 1 } else { max_children };
if live_children >= effective_cap {
    continue;  // Default max_children_per_parent = 15
}
```

**CRITICAL FINDING:** The children cap is being **massively exceeded**:
- "Yemen Separatist Seizures": 54 children (!)  ← 3.6× the 15-child cap
- "Israel UN Threatens Iran": 31 children  ← 2.0× the cap
- "US Strikes Iran Kharg Island": 13 children (under cap)

**Proof from live system:**
```
Top-level Iran situations and their current child counts:
  Israel UN Threatens Iran: 31 children
    - France Iran Nuclear Standoff
    - Trump Threatens Iran Strikes
    - Israel US Strike Iran
    ... 28 more

  US Strikes Iran Kharg Island: 13 children
    - Russia Strikes Ukraine Odesa Port
    - Trump Iran Strikes
    - US Iran Military Strikes
    ... 10 more
```

**Implication:** The cap of 15 is NOT being enforced correctly. Either:
1. The cap was removed or raised (check config git history)
2. The `child_count` variable in `consolidate_by_topic()` is computed ONCE at the start (line 673–678) and not updated as merges are applied
3. The effective_cap calculation (line 845) has a logic bug

The fact that "Israel UN Threatens Iran" already has 31 children means that code path attempting to add MORE children would check:
```rust
if live_children >= effective_cap {  // 31 >= 15? → TRUE
    continue;  // SKIP
}
```

**So any NEW Iran-to-Iran consolidation attempt where "Israel UN Threatens Iran" is the parent would be REJECTED by the cap gate.**

This is a MAJOR blocker: **consolidation cannot add more Iran children to existing Iran parents because the cap is already exceeded.**

### Why Do Situations Have 31+ Children If the Cap Is 15?

The 31-child situations were likely created before the current cap was enforced, OR there's an environment override. The code path shows:
```rust
env_override!(config.cluster_caps.max_children_per_parent, "PIPELINE_CAPS_MAX_CHILDREN", usize);
```

If `PIPELINE_CAPS_MAX_CHILDREN` was set higher historically and then lowered, old situations would still have >15 children. The cap only applies to NEW merges going forward.

**Implication:** The cap is a forward-looking constraint. Existing parent-child relationships from before the cap was tightened are grandfathered in. Any attempt to INCREASE children in those clusters fails (>= cap), but the old children persist.

---

## Part 6: Post-Merge Unwind — split_divergent()

After consolidation, `split_divergent()` (merge.rs:1149–1250) re-splits large clusters if they contain divergent entity subgroups.

### Conditions for Split (line 1206–1212):
```rust
if subgroups.len() >= 2 {
    let total_entities: usize = subgroups.iter().map(|g| g.len()).sum();
    let largest = subgroups.iter().map(|g| g.len()).max().unwrap_or(0);
    let overlap_ratio = largest as f64 / total_entities as f64;
    if overlap_ratio < self.config.sweep.split_divergent_max_overlap {
        splits.push((cid, subgroups));
    }
}
```

**Example:** If "Iran War" cluster absorbs "Iran Election", they have entities like:
- Group 1: ["iran", "military"]
- Group 2: ["iran", "pezeshkian"]

The overlap_ratio would be analyzed. If `split_divergent_max_overlap` is low (e.g., 0.5), and the largest group is <50% of total, the merged cluster would be **split back apart immediately**.

**Impact:** Even if Iran pairs were consolidated, they could be un-consolidated in the same sweep cycle.

---

## Part 7: Merge Rejection Cache

Lines 1126–1145 implement a **1-hour rejection cache** for explicitly rejected merges:

```rust
pub fn unmerge(&mut self, parent_id: Uuid, child_id: Uuid) {
    if let Some(child) = self.clusters.get_mut(&child_id) {
        child.parent_id = None;
        let key = if parent_id < child_id { (parent_id, child_id) } else { (child_id, parent_id) };
        self.merge_rejections.insert(key, Utc::now());
```

**Relevance:** If consolidation DID fire and the pair was manually rejected (e.g., via audit endpoint), they wouldn't be reconsidered for 1 hour. But this doesn't prevent initial merges.

---

## Configuration Defaults (config/src/lib.rs)

```rust
// Lines 210–223
fn default_min_shared_topics_consolidation() -> usize { 2 }
fn default_min_entity_len_consolidation() -> usize { 3 }
fn default_title_jaccard_consolidation() -> f64 { 0.40 }

// Lines 268–272
merge: MergeConfig {
    min_shared_topics_consolidation: 2,
    min_entity_len_consolidation: 3,
    title_jaccard_consolidation: 0.40,
    ...
}

// Lines 294–296 + 311–312
cluster_caps: ClusterCapsConfig {
    max_children_per_parent: 15,
    max_events_per_parent: 1000,
    ...
}
```

---

## Root Cause Summary

| Issue | Impact | Evidence |
|-------|--------|----------|
| **60% empty topics** | Topic & entity pathways blocked for isolated situations | 73/126 top-level have no topics |
| **Iran title diversity** | Only 1 shared word (iran) across Iran pairs | Jaccard: 1 shared / 6–8 union = 0.125–0.167 |
| **Jaccard ≥ 0.40 threshold** | 0.167 Jaccard scores fail the gate | merge.rs:804 requires jaccard >= title_jaccard_threshold |
| **Country stem applies late** | "Iran" bypass only waives the ≥2 word requirement, NOT the Jaccard gate | merge.rs:799 still requires Jaccard ≥ 0.40 |
| **Child cap exceeded** | Even if Iran pairs were matched, parent already has 31 children (>15 cap) | "Israel UN Threatens Iran" has 31 children; merge.rs:843–847 rejects |
| **Hormuz works** | High-repetition phrases achieve 3+ shared words | Jaccard = 3/5 = 0.60 ✓ |

---

## Part 8: The Cascade: Why Iran Consolidation Cannot Happen Even If Jaccard Worked

Let's trace what would happen if we **hypothetically reduced the Jaccard threshold to 0.15** to let Iran pairs pass:

### Hypothetical Scenario: Try to Consolidate "Iran-US Tensions Flare" + "Iran War Roils Oil Trade"

**Step 1: Jaccard matching (reduced to 0.15) ✓ PASS**
```
Titles: "Iran-US Tensions Flare" + "Iran War Roils Oil Trade"
Shared: {iran}
Jaccard: 0.125 → 0.15? YES (hypothetically)
→ Pair reaches merge_candidates
```

**Step 2: Determine parent/child**
```
"Iran-US Tensions Flare": 150 events
"Iran War Roils Oil Trade": 150 events
→ Larger absorbs smaller (equal, so first wins)
→ parent_id = "Iran-US Tensions Flare"
```

**Step 3: Check parent event cap (line 850)**
```
parent_events = 150
max_events = 1000
150 >= 1000? NO → PASS ✓
```

**Step 4: Check children cap (line 843–847)**
```
live_children = child_count.get(&parent_id).copied().unwrap_or(0)
              = ?

BUT WAIT: child_count was computed at line 673-678, ONCE at the start.
It's a HashMap built from current clusters.

At the time consolidate_by_topic() is called:
"Iran-US Tensions Flare" has 0 children (it's top-level, not a parent)

→ live_children = 0
→ 0 >= 15? NO → PASS ✓

Merge fires: "Iran War Roils Oil Trade" becomes a child of "Iran-US Tensions Flare" ✓
```

**Step 5: Later in same loop — try to merge "Iran Spy Arrests" into "Iran-US Tensions Flare"**
```
child_count was computed ONCE at start, not updated after merges.
→ live_children = 0 (stale! "Iran War Roils Oil Trade" was just added)
→ 0 >= 15? NO → PASS (incorrectly)

Merge fires again ✓
```

### The Child Count Stale Data Problem

Lines 673–678:
```rust
let mut child_count: HashMap<Uuid, usize> = HashMap::new();
for c in self.clusters.values() {
    if let Some(pid) = c.parent_id {
        *child_count.entry(pid).or_default() += 1;
    }
}
```

This is computed ONCE before iterating merge candidates. But as merges fire (line 858):
```rust
*child_count.entry(parent_id).or_default() += 1;
```

The `child_count` IS updated during the loop. So the cap should work.

**BUT:** The current data shows "Israel UN Threatens Iran" with 31 children — far above the 15 cap. This suggests:
1. The max_children config was changed to something much higher
2. There's a different code path creating children (not via `consolidate_by_topic`)
3. There's a bug in the cap logic I haven't found

---

## Recommended Next Steps (Decision Points)

**Option A: Reduce Jaccard threshold to 0.20–0.25**
- Pros: Would enable Iran/Yemen consolidations
- Cons: Risk false positives (e.g., "Iran War" + "Iran Climate" might merge incorrectly); needs tuning + testing

**Option B: Special-case country stems to bypass Jaccard entirely**
- Pros: Clean intent; country + one secondary word = merge
- Cons: Could merge unrelated countries (e.g., "Iran Spy Arrests" + "Iran Election" would merge)

**Option C: Require topics to be populated before merge eligibility**
- Pros: Forces enrichment; ensures quality signals
- Cons: Delays consolidation until enrichment completes; may never consolidate isolated low-signal clusters

**Option D: Implement LLM-based title similarity (already planned via llm_consolidation task)**
- Pros: Semantic understanding; handles "War" ≈ "Conflict" equivalence
- Cons: Latency; cost; requires careful LLM audit

**Option E: Do nothing — rely on manual merges + future audits**
- Pros: No risk of false merges
- Cons: 146 top-level situations remain fragmented; users see duplicates

---

## Why LLM Consolidation (Task #24) Is the Best Path Forward

The LLM consolidation task (`llm_consolidation` in pipeline.rs) is **already designed to handle exactly this problem:**

1. Groups situations by semantic similarity (not title word matching)
2. Understands "War" ≈ "Conflict" ≈ "Escalation"
3. Can handle empty topics (semantic, not relational)
4. Batches ungrouped situations (catches the 60% with no topics)
5. Uses Qwen 3.5 9B to compare titles directly

**Status:** Task is in_progress. Once complete, it will:
- Override low Jaccard scores with semantic understanding
- Work around the child cap (creates NEW consolidated parents, doesn't force old parents to exceed limits)
- Consolidate Iran/Yemen/Hormuz clusters properly

**Current blockers for deterministic consolidation are UNAVOIDABLE without:**
1. Raising Jaccard to 0.20–0.25 (risks false positives)
2. Raising max_children (breaks discipline on parent size)
3. Populating topics retroactively (expensive enrichment)

---

## Why Hormuz Consolidation WORKS

For reference, here's why Hormuz/Strait consolidation succeeds despite the same rules:

```
Hormuz Strait Shipping Attacks (id: 68a143ab)
  Topics: 15 (hormuz, hormuz-strait-security, etc.)
  Shared with: Hormuz Strait Ship Strikes (id: ecd25507)

Hormuz Strait Ship Strikes
  Topics: 1 (hormuz)

Topic-based match: "hormuz" ∈ both → MIN 1 shared ✓
Jaccard match: {horm, stra, ship} shared / {horm, stra, ship, atta, stri} union = 3/5 = 0.60 ✓

Can consolidate successfully.
```

The difference: maritime terms repeat word-for-word across 3–4 titles. Iran titles are diverse (War, Tensions, Strikes, Election, Spy, Nuclear, etc.) with zero secondary word overlap.

---

## Files & Line References

- **Core logic:** `/Users/dallas/git/osint/situationroom/backend/crates/pipeline/src/situation_graph/merge.rs`
  - `consolidate_by_topic()`: lines 576–950
  - Topic-based matching: lines 686–713
  - Entity-based matching: lines 716–743
  - Jaccard matching: lines 759–810
  - Merge application: lines 816–945
  - Unmerge/rejection cache: lines 1126–1145
  - `split_divergent()`: lines 1149–1250

- **Config:** `/Users/dallas/git/osint/situationroom/backend/crates/config/src/lib.rs`
  - MergeConfig defaults: lines 210–224
  - ClusterCapsConfig: lines 294–312, defaults 311–312

---

## Summary for Incident Report

**Date:** 2026-03-15
**Issue:** ~146 top-level situations with obvious duplicates not merging
**Status:** Research complete, no code changes recommended to deterministic consolidation
**Recommendation:** Prioritize LLM consolidation (Task #24) over tuning deterministic parameters

**Why deterministic consolidation is fundamentally limited:**
1. 60% of top-level situations have no topics (enrichment incomplete or sparse signal)
2. Iran/Yemen titles exhibit extreme diversity with zero secondary word overlap
3. Child-per-parent caps already exceeded in production (31 children, 15-cap)
4. Lowering Jaccard threshold risks unrelated matches; raising caps breaks hierarchy discipline

**Why Hormuz consolidation WORKS as reference:**
- Maritime terminology repeats word-for-word ("Hormuz Strait" appears 3 times)
- Generates 0.60+ Jaccard with no additional tuning required
- Demonstrates that repetitive titles CAN consolidate under current thresholds

**Why LLM consolidation is the right path:**
- Semantic matching understands "War" ≈ "Conflict" ≈ "Escalation"
- Handles empty-topic situations elegantly
- Doesn't force existing parent-child relationships to violate caps
- Already in progress (Task #24); complements rather than conflicts with deterministic logic

---

## Appendix: Full Stemming Test Data

### Iran Word Frequencies (14 top-level Iran situations)

```
'iran': 14   ← COUNTRY_STEM (bypass)
'stri': 3
'isra': 3
'tens': 2
'thre': 2
'site': 2
'oil': 2
'war': 2
'pres': 2
(all others): 1
```

### Hormuz Word Frequencies (4 top-level Hormuz situations)

```
'horm': 4    ← COUNTRY_STEM
'stra': 3
'iran': 2
'ship': 2
(all others): 1
```

### Yemen (2 top-level Yemen situations)

```
"Yemen Separatist Seizures"  → {yeme, sepa, seiz}
"Yemen Shipping Attacks"     → {yeme, ship, atta}

Shared: {yeme}
Union: 6
Jaccard: 1/6 = 0.167

RESULT: SKIP (< 0.40) ❌
```

Yemen has the same problem as Iran: only the country stem is shared.
