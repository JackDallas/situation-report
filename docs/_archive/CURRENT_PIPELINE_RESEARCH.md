# Event clustering architectures for real-time OSINT fusion

**Your current inverted-index + weighted scoring + graph clustering architecture is fundamentally sound — the problems you're experiencing (mega-clusters, GDELT dominance, false merges, temporal insensitivity) are tuning and constraint issues, not architectural failures.** The most impactful improvements are a three-phase coarse-to-fine pipeline, streaming IDF-weighted topic scoring, event-type-adaptive distance thresholds via H3 hexagonal indexing, and exponential temporal decay with per-category half-lives. Density-based algorithms like HDBSCAN are poor fits for your 1024-dim mixed-modality streaming context. Below is a detailed treatment of each question, grounded in academic literature, production systems, and practical Rust implementation considerations.

---

## 1. Your architecture is right — density-based clustering is not the answer

The inverted-index + scoring approach directly mirrors the **blocking + matching** pattern from entity resolution, which is the established architecture for large-scale record linkage at LinkedIn, Google, and similar companies. HDBSCAN, DBSCAN, and hierarchical agglomerative clustering all have fundamental problems for your use case.

**HDBSCAN fails at 1024 dimensions.** The official HDBSCAN documentation states it works for "up to around 50 or 100 dimensional data" before performance degrades significantly. At 1024 dimensions, density estimation becomes unreliable and most points get classified as noise. The standard workaround — projecting via UMAP to ~10-50 dimensions first — adds latency and complexity. A January 2025 arXiv paper (2601.20680) systematically compared DenStream, DBSTREAM, and TextClust against batch HDBSCAN for real-time narrative monitoring and found that while DenStream was the most viable online alternative, the River ML implementation had critical scalability issues where `predict_one` recomputes DBSCAN over all micro-clusters on every call. FISHDBC (Dell'Amico, 2019) is the most promising incremental HDBSCAN variant, using HNSW graphs for approximate nearest-neighbor search and incrementally maintaining an MST — but it exists only in Python with no Rust implementation.

**Streaming density-based algorithms lack Rust ecosystem support.** No streaming clustering crate exists in Rust. DenStream, CluStream, BIRCH, FISHDBC — none have Rust implementations. The building blocks exist (`hnswlib-rs` for ANN, `petgraph` for Union-Find and graph algorithms, `linfa-clustering` for batch DBSCAN/OPTICS), but any streaming density-based approach would require ground-up implementation.

**The real problems are addressable within your current architecture.** Your mega-cluster issue stems from connected components being too aggressive — a single bridge edge from a generic topic merges two large clusters. The fixes are constraint-based:

- **Cluster size caps**: Refuse to merge clusters exceeding a size threshold (e.g., 500 events). When a merge would create an oversized cluster, require proportionally higher similarity scores.
- **Bridge resistance**: When an event connects to multiple existing clusters, require higher similarity thresholds than when adding a singleton. Inspired by the SCoDA streaming community detection algorithm, events connected to many clusters get classified as potential noise rather than creating bridges.
- **Source-level rate limiting**: Cap GDELT's contribution per time window in the blocking stage. If GDELT produces 300 of your 500 events/minute, limit its blocking key contribution to prevent it from dominating cluster centroids.
- **Entity confidence gating**: Downweight or ignore entities below a confidence threshold from the Claude Haiku extraction, preventing low-quality entity matches from causing false merges.

**The recommended enhancement** is evolving to a multi-stage blocking + weighted graph + constrained connected components architecture, adding LSH on the 1024-dim embeddings (random hyperplane LSH with ~8-16 hash functions × 4-8 tables) as a secondary candidate source alongside your inverted index. HNSW via `usearch` or `hnswlib-rs` gives O(log n) approximate nearest-neighbor search for embedding similarity. The full pipeline — inverted-index lookup + LSH + HNSW search + weighted scoring + constrained Union-Find — runs well under **10ms per event** at your throughput, comfortably sub-second.

| Rust Crate | Purpose | Maturity |
|---|---|---|
| `hnswlib-rs` | Pure Rust HNSW, supports f32/f16/bf16 | Production-quality |
| `usearch` | Rust bindings for USearch (HNSW), custom metrics | Active, well-maintained |
| `hora` | Multi-algorithm ANN (HNSW, SSG, PQIVF) | Active |
| `petgraph` | UnionFind, connected_components, MST (9.3M downloads/month) | Mature |
| `linfa-clustering` | Batch DBSCAN, OPTICS, K-Means, GMM | Most mature Rust ML framework |

---

## 2. Exponential decay with event-type-specific half-lives

Temporal decay is essential and should be multiplicative with content similarity. **Exponential decay is the correct default** because it is the only function where forward and backward decay formulations are identical (Cormode et al., "Forward Decay," DIMACS/Rutgers), making it trivially efficient in streaming — you multiply the previous sum by `exp(-λ·dt)` then add the new arrival, requiring no recomputation of historical weights.

**The formula for combined scoring:**

```
score(event, situation) = content_sim(event, centroid) × exp(-λ_type × max(0, Δt - offset)) × geo_factor
```

Where `λ_type = ln(2) / half_life` and `offset` is a grace period of full relevance before decay begins. The `Δt` should measure time since the *last event* in the situation, not since the situation's creation — this keeps actively updated situations fresh. The offset parameter (30-60 minutes for military events, 2-4 hours for broader crises) provides a "breaking news window" before decay kicks in.

**Recommended half-lives by event category**, synthesized from Hawkes process parameter estimation on conflict data, news story duration research (average story lasts **1.4 days**, larger stories 3.5-5 days per Nicholls & Bright 2018), and crisis lifecycle analysis:

| Event Category | Half-Life | Offset | Rationale |
|---|---|---|---|
| Military strike / tactical incident | 2-4 hours | 30 min | Hawkes process for Iraq insurgent data: 1/β ≈ 0.45 days |
| Military offensive / campaign | 2-3 days | 2 hours | Multi-day operations with evolving phases |
| Terrorist attack | 4-8 hours | 30 min | Rapid onset, intensive 24-hour news cycle |
| Natural disaster (acute) | 6-12 hours | 1 hour | Impact phase is hours; aftershocks follow Omori power-law |
| Natural disaster (extended) | 3-7 days | 4 hours | Hurricane/flood impact spans days to weeks |
| Cyber incident | 12-48 hours | 1 hour | Active response is hours; detection/containment takes days |
| Political crisis / protests | 1-3 days | 2 hours | COVID disorder events: exponential decay with 1/β = 0.45 days |
| Maritime / aviation incident | 4-12 hours | 30 min | Acute operational events with rapid resolution |
| Seismic event | Power-law, not exponential | — | Aftershock sequences follow Omori-Utsu law: (t+c)^(-p) with p ≈ 1 |

**For seismic events specifically**, use a power-law kernel `(Δt + c)^(-p)` with p ≈ 1.0 and c ≈ 0.01 days, matching the well-established Omori-Utsu law for aftershock decay. This captures the heavy-tailed temporal correlation that exponential decay misses.

**Multi-scale temporal architecture.** The TDT literature (DARPA/NIST, 1996-2004) established that events operate at multiple temporal scales. Implement a three-tier model: **incidents** (hours, tight half-life), **situations** (days, loose half-life), and **campaigns/crises** (weeks, very loose coupling). Incidents cluster into situations when content similarity is high and temporal overlap exists; situations link into campaigns via looser thematic connection. This mirrors the worldmonitor OSINT platform's approach of 24-hour event binning for convergence detection, 48-hour regression for trends, and 90-day baselines for anomaly detection.

**Consider Hawkes self-excitation for conflict events.** When a conflict event arrives, it temporarily boosts the activity level of nearby situations, modeling real-world cascading dynamics. Research on the Global Terrorism Database and Iraqi insurgent data shows branching ratios of **γ ≈ 0.8-0.97**, meaning each event generates nearly one follow-on event. This can be layered on top of base exponential decay: situations receiving bursts of events get dynamically extended active windows.

---

## 3. Streaming IDF and burstiness replace blocklists entirely

The blocklist approach is fundamentally brittle — it requires manual curation and can't adapt to changing event dynamics. **Streaming IDF is the single highest-impact improvement**, naturally penalizing generic topics without any manual rules. Combined with burstiness detection and structural heuristics, it creates a robust multi-signal topic quality score.

**Streaming IDF implementation.** Since your topics are discrete labels (bounded vocabulary of likely thousands, not millions), a simple `HashMap<String, f64>` with exponentially decayed counts is ideal. On each event, decay all counters and increment for observed topics:

```
topic_doc_freq[topic] = decay_factor × topic_doc_freq[topic] + 1.0
total_events = decay_factor × total_events + 1.0
IDF(topic) = ln(total_events / (1.0 + topic_doc_freq[topic]))
```

With `decay_factor = 0.9999` at 500 events/min, the effective window is ~7,000 events (~14 minutes). This is **O(1) per event per topic** and trivially implementable in Rust. A topic like "regional-middle-east" appearing in 40% of events gets IDF ≈ 0.5, while "Zaporizhzhia-nuclear-plant" appearing in 0.1% of events gets IDF ≈ 7.0 — a **14× difference in discriminative weight**, automatically.

**Burstiness via dual EWMA.** A topic that suddenly surges in frequency (going from baseline 2 events/hour to 50 events/hour) is almost certainly a specific, discriminative event — even if its absolute frequency is still moderate. Track two exponentially weighted moving averages per topic: a short-window EWMA (5-minute half-life) and a long-window EWMA (6-hour half-life). The burstiness score is simply `short_ewma / long_ewma`. This captures the temporal novelty signal that static IDF misses. The BurstSketch algorithm (Peking University, SIGMOD 2021) achieves **97% F1 with 60KB memory at 20M items/sec** — far more than needed for your throughput, but the simpler dual-EWMA approach is sufficient and more interpretable.

**Structural specificity heuristics** provide a zero-cost, stateless quality signal. Multi-token topics are more specific ("GPS-jamming-Black-Sea" > "jamming"), topics containing proper nouns are more specific ("Zaporizhzhia" > "nuclear"), and topics with known generic prefixes ("regional-*", "geopolitical-*") can be heavily penalized without full blocklisting. This runs in O(1) with no state.

**Feedback from clustering outcomes** closes the loop. After clustering, compute the entropy of each topic's distribution across clusters: `H(topic) = -Σ P(cluster|topic) × log P(cluster|topic)`. Low entropy means the topic concentrates in few clusters (discriminative); high entropy means it spreads uniformly (generic). Maintain an EMA of this quality signal per topic: `quality_ema[topic] = α × observed_quality + (1-α) × quality_ema[topic]`. Over time, the system learns that "geopolitical-conflict" always produces mega-clusters (quality → 0) while "Houthi-Red-Sea-attacks" produces tight clusters (quality → 1).

**Combine into a composite topic weight:**

```
topic_weight(t) = IDF(t)^0.4 × (0.01 + burstiness(t))^0.2 × specificity(t)^0.2 × feedback(t)^0.2
```

The IDF component dominates (0.4 weight) because it's the most reliable signal. The 0.01 floor on burstiness prevents zero-multiplication for non-bursty topics. Total memory cost is O(|topics|) — a few MB for tens of thousands of topics. Implementation priority: streaming IDF first (~1 hour to implement), structural heuristics second (~30 minutes), burstiness third (~1 hour), feedback loop fourth (~2 hours).

**From entity resolution literature**: impose a hard maximum cluster size constraint. If any topic is the sole remaining clustering key and the resulting cluster exceeds a size threshold, that topic's quality score should be degraded automatically. This provides a safety net regardless of the scoring function.

---

## 4. H3 hexagonal indexing with event-type-specific resolution

The flat 200km threshold is wrong because event types have fundamentally different spatial scales. **Replace it with a lookup table mapping event types to clustering radii and H3 resolutions**, using the `h3o` crate (pure Rust, no C dependencies, WASM-compatible, benchmarked as fast as the C reference library).

**Event-type distance thresholds**, synthesized from ACLED geo-precision standards, seismological attenuation models, signal propagation physics, and conflict analysis literature:

| Event Type | Radius (km) | H3 Resolution | Notes |
|---|---|---|---|
| Military strike / battle | 25 | Res 4 (edge ~26km) | ACLED records at specific named locations |
| Airstrike / remote violence | 50 | Res 3-4 | Multiple proximate targets per operation |
| Protest / riot | 10 | Res 5 (edge ~10km) | Urban-scale events |
| GPS jamming | 300 | Res 2 (edge ~183km) | Documented 200-500km ranges |
| FIRMS thermal / fire | 5 | Res 5-6 | Satellite pixel resolution 375m-1km |
| Seismic event | 10^(0.5×M - 0.8) | Variable by magnitude | M5→50km, M6→160km, M7→500km |
| Nuclear radiation | 500 (initial) | Res 2 | Wind-dependent plume model extends further |
| NOTAM / airspace | Defined geometry | Exact polygon | Use the NOTAM's specified circle/box |
| Cyber / BGP event | Country/ASN polygon | Res 0-1 | Not radius-based; use shared country/ASN |
| ADS-B aviation | 50 | Res 3-4 | TCAS range ~50km |
| Maritime AIS | 100 | Res 3 | Maritime chokepoints, sea lanes |
| News (geolocated) | 50 | Res 3-4 | Geocoding uncertainty buffer |
| News (country only) | Country polygon | Res 0-1 | Administrative boundary matching |

**Index every event at multiple H3 resolutions simultaneously.** For each event, compute the H3 cell at its native resolution, then compute parent cells at coarser resolutions using `h3o`'s `parent()` method — a single bitwise operation. This enables O(1) cross-type spatial joins: a military strike at resolution 4 and a GPS jamming event at resolution 2 are compared by truncating the strike's index to resolution 2. Store a `HashMap<H3Index, Vec<EventId>>` per resolution level.

**Three-tier location model for mixed-modality data:**

- **Tier 1 (Precise)**: FIRMS, ADS-B, AIS, USGS — stored as lat/lon + H3 index at native resolution with event-type-specific uncertainty radius
- **Tier 2 (Region)**: Some news, some cyber, city-level Telegram — stored as country code + admin region + centroid + H3 at resolution 1-3
- **Tier 3 (No location)**: Some Telegram, some cyber — excluded from geographic clustering but participate in temporal/semantic clustering; optionally infer location from mentioned entities, source metadata, or actor nationality

**Cyber events need special treatment.** BGP hijacks affect ASN-level or country-level infrastructure, not a geographic radius. Map ASNs to country codes using RIR data (RIPE, ARIN, APNIC). Two cyber events are "co-located" if they share any affected country or ASN. For cross-type correlation (cyber + kinetic), map the cyber event's affected countries to H3 resolution 1 cells.

**Normalize geographic distance per event-type pair:** `d_geo = min(1.0, haversine(e1, e2) / max(radius_e1, radius_e2))`. When comparing events of different types, use the larger radius as the denominator. Combine with temporal, semantic, and actor distances using event-type-pair-specific weights — for example, kinetic + kinetic pairs weight geography at 0.4, while cyber + news pairs weight it at 0.15 and boost semantic similarity to 0.35.

---

## 5. The lineage of systems solving this exact problem

Your system sits squarely in a well-studied lineage spanning DARPA programs, academic research, and commercial platforms. The most directly relevant precedents inform both what works and what remains hard.

**The Topic Detection and Tracking (TDT) program** (DARPA/NIST, 1996-2004) is the foundational literature. TDT defined five tasks including First Story Detection and Topic Detection that map directly to your "assign event to situation or create new situation" operation. The key finding: **incremental single-pass clustering using TF-IDF weighted cosine similarity**, comparing each new document against existing cluster centroids with temporal proximity boosting, was consistently the best approach. Complete/average linkage hierarchical clustering scored highest at the final TDT 2004 evaluation. Allan et al. (2000) established that "First Story Detection in TDT is hard" — distinguishing genuinely new events from minor variations remains the hardest subproblem.

**DARPA AIDA** (Active Interpretation of Disparate Alternatives) is the closest government program to your problem definition. It processes continuously streaming multi-source, multi-modal, multi-lingual data (text, speech, images, video) and maps them into a common semantic representation, then generates multiple *alternative* interpretations of events. The key insight: single-interpretation analysis loses alternatives prematurely, so multi-hypothesis tracking is critical for intelligence applications.

**DARPA ICEWS** (Integrated Crisis Early Warning System) processed **30+ million news stories yielding 20+ million unique events** from 100+ data sources using an ensemble of 75+ heterogeneous models (logistic regression, Bayesian, agent-based). It achieved >90% forecast accuracy with <20% false alarms for crisis events. ICEWS's key architectural lesson: **ensemble model aggregation via Learned Bayesian Networks significantly improves over individual models**. The CAMEO event taxonomy it uses remains the standard for political event coding.

**DARPA KAIROS** (Knowledge-directed AI Reasoning Over Schemas) represents the current generation, learning event schemas from data and applying them to discover complex events. The CHRONOS system (AAAI 2024) showed that combining symbolic reasoning with neural methods outperforms either alone — LLMs alone have reliability issues with faithfulness and explainability in this domain.

**Dataminr** is the closest commercial analog, processing **billions of public data inputs daily** across text (150 languages), images, video, sound, and sensor data (including ADS-B) to detect ~500,000 daily events. Their architecture uses 50+ proprietary LLMs, a Knowledge Graph for entity/event reasoning, and GNNs for clustering. Their ReGenAI system dynamically rewrites alerts as new data arrives — worth studying for situation evolution. **Recorded Future** uses an elegant dual-graph architecture: a slow-changing ontology graph for stable entity information and a fast-changing event graph for streaming intelligence, tracking **13B+ entities and 4,000+ threat actors**.

**Twitter's production event detection system** (Fedoryszak et al., KDD 2019) processes ~500M tweets/day at 6K tweets/second — comparable scale to your system. Their critical architectural insight: **decompose burst detection and clustering into separate components that can be scaled independently**. They use a multi-stage pipeline: entity extraction → trend detection (anomaly scoring) → entity filtering → similarity computation → Louvain community detection → cluster chain linking. This directly validates the multi-phase approach.

**The JDL Data Fusion Model** (Joint Directors of Laboratories, 1987/revised 1998-2004) provides the theoretical framework. Your "situation" concept maps directly to **JDL Level 2 (Situation Assessment)** — grouping entity-level observations into coherent situations. The literature explicitly notes that Level 2 is the **hardest and least mature** fusion level, with much higher dimensionality than entity tracking and no general metric for assessing relevance.

**Open-source resources worth examining**: `ina-foss/louvain-news-clustering` (Louvain community detection for news/Twitter events), `fedecaccia/Online-News-Clustering` (incremental TF-IDF streaming clustering), CityPulse Event-Detector (streaming event detection framework), and the multimodal cross-document event coreference system at `github.com/csu-signal/multimodal-coreference` (91.9 CoNLL F1 on ECB+).

---

## 6. Three-phase architecture delivers ~40,000× computational savings

The two-phase approach is not merely viable — it is the **established best practice** across canopy clustering, entity resolution blocking, production event detection (Twitter), and large-scale deduplication (HuggingFace/BigCode). A three-phase architecture is optimal for your system.

**The computational case is overwhelming.** With a 24-hour active window of ~720,000 events, brute-force pairwise comparison requires ~259 billion 1024-dim cosine similarity computations per day — infeasible in real-time. Three-phase blocking with HNSW reduces this to ~720K × O(log 600) ≈ **6.5 million distance computations per day**, a reduction of approximately **40,000×**. Canopy clustering (McCallum et al., KDD 2000) demonstrated **17× speedup** on 1M+ bibliographic citations. Entity resolution blocking achieves **>99% reduction ratios** while maintaining 95-99% recall of true matches (Papadakis et al., ACM Computing Surveys 2020).

**Recommended three-phase architecture:**

**Phase 0 — Intake and routing (<1ms).** Assign each event a region code via H3 cell lookup (or "GLOBAL" if no geo), a time bucket (1-hour sliding window), and a source category. Route to the appropriate partition worker. This is pure O(1) lookups — `HashMap<(H3Index, TimeBucket), PartitionId>`. Events without geographic data route to a dedicated GLOBAL partition that uses entity/semantic-only matching.

**Phase 1 — Candidate selection (<10ms).** Within the partition, search the local HNSW index for top-K (K=10) most similar cluster centroids using the 1024-dim BGE-M3 embedding. Simultaneously check entity overlap via an inverted index (`HashMap<Entity, Vec<ClusterId>>`). The union of HNSW results and entity-overlap results produces a candidate cluster list of typically 3-15 clusters. HNSW with M=16, efSearch=50 gives sub-millisecond queries on partition-sized indices.

**Phase 2 — Fine-grained scoring and decision (<50ms).** For each candidate cluster, compute the composite score incorporating embedding cosine similarity, entity Jaccard overlap weighted by entity confidence, topic compatibility weighted by streaming IDF, temporal decay with event-type-specific half-life, and geographic proximity normalized by event-type radius. If the best score exceeds a merge threshold (~0.72), assign to that cluster and incrementally update the centroid. If below a new-cluster threshold (~0.45), create a new situation. Scores in between queue for asynchronous review.

**Async background jobs (every 1-5 minutes)** handle cross-partition merge checking (comparing cluster centroids across adjacent geographic/temporal partitions to catch situations that span partition boundaries), coherence monitoring (tracking within-cluster embedding variance and triggering splits when it exceeds a threshold), mega-cluster detection and forced splitting, and HNSW index compaction with time-based eviction of events older than the active window.

**Handling events without geographic data** is the primary risk of a geo-first approach. The solution is multi-path routing: geolocated events flow through the spatial-first pipeline while non-geolocated events flow through a parallel entity/semantic-only pipeline via the GLOBAL partition. The async cross-partition merge job reconciles the two paths every few minutes, checking whether non-geolocated clusters semantically match geolocated ones. This avoids the false-split problem where a Telegram post about an event can't find its matching geolocated cluster.

**Preventing Phase 1 errors from propagating** requires overlapping partitions (canopy-style): events near H3 cell boundaries should appear in multiple partitions, ensuring that true clusters split across boundaries are eventually merged. The async cross-partition merge job provides the safety net. Additionally, maintaining a cluster coherence metric (within-cluster embedding variance) enables automatic detection and splitting of incorrectly merged clusters.

---

## Conclusion: a unified implementation roadmap

The research converges on a clear set of high-impact changes ordered by implementation effort and expected payoff. **First**, implement streaming IDF topic weighting — this single change addresses mega-clusters, replaces brittle blocklists, and takes roughly one hour to code. **Second**, add event-type-specific temporal decay with the half-life table above and an offset grace period — this fixes temporal insensitivity with minimal architectural change. **Third**, replace the flat 200km geographic threshold with the H3-indexed event-type-specific distance table using the `h3o` crate, simultaneously solving the multi-scale spatial problem and enabling efficient spatial candidate generation.

**Fourth**, and most architecturally significant, restructure the pipeline into the three-phase architecture (route → HNSW candidate selection → multi-signal scoring), adding constrained Union-Find that refuses oversized merges and requires higher similarity for bridge edges between existing clusters. This addresses GDELT volume dominance and mega-clustering structurally rather than through parameter tuning. The entity resolution blocking literature confirms this approach achieves >99% reduction in pairwise comparisons with <5% loss in recall — directly applicable math for your 500 events/minute at 720K daily events.

The overall architecture — inverted index + HNSW for candidate generation, multi-signal weighted scoring with streaming IDF and temporal decay, constrained graph clustering with size caps and coherence monitoring — is not novel in any single component but represents a well-validated synthesis of entity resolution blocking, TDT incremental clustering, and production event detection systems. The distinctive challenge of your system — 26 heterogeneous sources with mixed modalities — is best addressed not by a single clever algorithm but by the adaptive distance functions and event-type-specific parameters described above, which let the same pipeline handle military strikes at 25km resolution and BGP hijacks at country level through a unified H3-based spatial framework.
