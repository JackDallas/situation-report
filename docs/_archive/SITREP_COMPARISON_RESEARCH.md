Situation Report — Prior Art & Techniques Analysis

 Context

 This document maps the Situation Report's architecture against prior art in OSINT, military C2, SIEM, geospatial intelligence, news monitoring, and adjacent real-time data domains
 (finance, IoT, NDR, supply chain, autonomous vehicles, weather systems). The goal is to identify what's novel, what's validated by industry practice, and what techniques from other
 domains could strengthen the system.

 ---
 1. What Situation Report Actually Is

 A multi-domain real-time intelligence fusion platform that:
 - Ingests 23 heterogeneous sources (conflict, aviation, maritime, cyber, seismic, nuclear, news, GPS)
 - Clusters events into situations using a multi-signal scoring engine (streaming IDF, burst detection, temporal decay, geo-proximity, entity overlap, topic similarity, vector
 embeddings, title Jaccard)
 - Detects cross-source patterns via 8 correlation rules (military strikes, infrastructure attacks, coordinated shutdowns, etc.)
 - Enriches with LLMs (Ollama local GPU first, Claude API fallback) — single-call translate + summarize + entity/relationship/state extraction
 - Generates Dataminr-style regenerating narratives (BLUF/TIMELINE/STATUS/UNCERTAINTIES/INDICATORS)
 - Visualizes on a real-time map with dead-reckoning position interpolation, situation hierarchy, and domain-tabbed analysis panels
 - Manages AI budget with daily caps and graceful degradation chain

 ---
 2. Landscape Positioning

 Open Source OSINT Tools

 ┌────────────────┬────────────────┬───────────────────────┬─────────────────────────────────────────────┐
 │      Tool      │     Focus      │   Correlation Model   │             vs. Situation Report              │
 ├────────────────┼────────────────┼───────────────────────┼─────────────────────────────────────────────┤
 │ SpiderFoot     │ Recon/enum     │ Module-based, batch   │ Single-domain, not real-time                │
 ├────────────────┼────────────────┼───────────────────────┼─────────────────────────────────────────────┤
 │ Maltego        │ Link analysis  │ Manual transforms     │ Interactive, not automated                  │
 ├────────────────┼────────────────┼───────────────────────┼─────────────────────────────────────────────┤
 │ OpenCTI        │ CTI sharing    │ STIX 2.1 graph, dedup │ Cyber-only, recently added AI ("Ariane")    │
 ├────────────────┼────────────────┼───────────────────────┼─────────────────────────────────────────────┤
 │ MISP           │ Threat sharing │ Attribute matching    │ Federated sharing focus, simple correlation │
 ├────────────────┼────────────────┼───────────────────────┼─────────────────────────────────────────────┤
 │ TheHive/Cortex │ IR response    │ Analyzer enrichment   │ Case-driven (reactive), not stream-driven   │
 └────────────────┴────────────────┴───────────────────────┴─────────────────────────────────────────────┘

 Gap identified: No open-source tool fuses conflict + aviation + maritime + cyber + seismic + nuclear + news into one real-time picture. OpenCTI is cyber-only; GDELT is news-only;
 SpiderFoot is recon-only.

 Commercial Intelligence Platforms

 ┌────────────────────────────────────┬───────────────────────────────────────────────────────┬──────────────────────────────────────────────────┐
 │              Platform              │                   Closest Parallel                    │                  Key Difference                  │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Palantir Gotham/Foundry            │ Entity ontology + analyst workspace                   │ Analyst-driven mapping vs. automated correlation │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Recorded Future Intelligence Graph │ Entity extraction + knowledge graph from 1M+ sources  │ Custom NLP models, vastly larger corpus          │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Primer Command                     │ AI-driven situational awareness from multiple sources │ DoD contracts, proprietary models                │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Dataminr                           │ ReGenAI narrative regeneration                        │ 12+ years of proprietary training data           │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Anduril Lattice                    │ Real-time sensor fusion into unified track picture    │ Hardware sensor fusion, edge compute             │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Silobreaker                        │ AI-native intelligence with PIRs                      │ "Priority Intelligence Requirements" concept     │
 ├────────────────────────────────────┼───────────────────────────────────────────────────────┼──────────────────────────────────────────────────┤
 │ Nuculair                           │ 312 sources, Neo4j graph, 2.3M events/day             │ Closest independent project in scope             │
 └────────────────────────────────────┴───────────────────────────────────────────────────────┴──────────────────────────────────────────────────┘

 SIEM / Event Correlation

 ┌─────────────┬──────────────────────────────────────────────────────────────────────┬────────────────────────────────────────────────────────────┐
 │   System    │                              Technique                               │                         Relevance                          │
 ├─────────────┼──────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────┤
 │ Splunk ITSI │ Episode rules grouping related events with severity calc + lifecycle │ Direct analog to situation clustering + SituationPhase FSM │
 ├─────────────┼──────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────┤
 │ ArcSight    │ CEP temporal patterns (A→B within N minutes)                         │ Pipeline's 8 rules are imperative CEP patterns             │
 ├─────────────┼──────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────┤
 │ Elastic     │ ML anomaly detection baseline + deviation                            │ Could complement rule-based detection                      │
 └─────────────┴──────────────────────────────────────────────────────────────────────┴────────────────────────────────────────────────────────────┘

 Key difference: SIEMs correlate within one domain (security) using field-match rules. Situation Report correlates across domains using geo + time + entity + topic + embeddings — a
 much richer signal space.

 ---
 3. Techniques Validated by Industry Practice

 The following current architectural choices are confirmed as best practices across multiple domains:

 ┌────────────────────────────────────┬────────────────────────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────┐
 │             Technique              │                        Where Validated                         │                      Situation Report Implementation                      │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Dual EWMA burst detection          │ Finance (PEWMA), social media event detection                  │ 5min short / 6hr long, ratio>2.0 triggers bonus                         │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Sliding window correlation         │ SIEM (Splunk episodes), CEP engines                            │ 6hr CorrelationWindow with multi-index                                  │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ 4-layer entity resolution          │ Senzing (entity-centric matching), OpenCTI                     │ Exact → fuzzy → Wikidata QID → create new                               │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Knowledge graph fusion             │ Amazon Fuse Platform, Palantir ontology                        │ petgraph StableGraph + BFS neighborhood                                 │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Embedding-based similarity         │ MIT/Stanford time-aware doc embeddings                         │ BGE-M3 1024-dim with cosine similarity gates                            │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Single-call LLM enrichment         │ LLM-augmented analytics best practice                          │ Haiku: translate + summarize + extract in one call                      │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Local model first, cloud fallback  │ Budget-aware tiered inference                                  │ Qwen3.5-9B (Ollama) → Claude Haiku → skip                               │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Prompt caching                     │ Anthropic recommended practice                                 │ ~95% cache hit rate on system prompts                                   │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Progressive refinement             │ ShakeAlert seismic (act on 3 stations, update later)           │ Ingest fast → correlate → enrich important only                         │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ FSM situation lifecycle            │ Splunk episode lifecycle, military OODA loop                   │ SituationPhase:                                                         │
 │                                    │                                                                │ emerging→developing→active→declining→resolved→historical                │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Exponential backoff                │ Universal distributed systems pattern                          │ Per-source backoff, reset on success                                    │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Dead reckoning interpolation       │ Autonomous vehicles, AIS vessel tracking                       │ Geodetic extrapolation between 30s polls                                │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Per-event-type temporal decay      │ MIT/Stanford (2024): different event types need different      │ Conflict=4h, nuclear=48h, default=12h                                   │
 │                                    │ decay                                                          │                                                                         │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Importance filtering               │ Planet Labs "tip and cue", ShakeAlert progressive alerts       │ Only important events → SSE; high-volume summarized                     │
 ├────────────────────────────────────┼────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤
 │ Dual rhythm (continuous +          │ Supply chain (Resilinc), military intel cycle                  │ Real-time SSE events + periodic Sonnet analysis                         │
 │ periodic)                          │                                                                │                                                                         │
 └────────────────────────────────────┴────────────────────────────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────┘

 ---
 4. What's Genuinely Novel

 These aspects of the Situation Report don't have clear precedent in the surveyed landscape:

 4a. Multi-Domain Fusion Scope (Open Source)

 No open-source tool combines conflict, aviation, maritime, cyber, seismic, nuclear, news, and GPS into a single correlation engine. Commercial tools (Palantir, Recorded Future) do
 this but are proprietary and analyst-driven rather than automated.

 4b. Multi-Signal Cluster Scoring

 The score_candidate() function combines 8+ scoring dimensions simultaneously:
 - Streaming IDF (entity + topic)
 - Burst detection bonus
 - Temporal decay (per event-type)
 - Geographic proximity (graduated, per event-type radii)
 - Entity overlap (IDF-weighted)
 - Topic overlap (IDF-weighted + burst)
 - Vector similarity (hard reject gate at <0.40)
 - Title Jaccard
 - Size penalty (escalating)
 - Cross-source bonus / single-source penalty

 Most systems use 2-3 of these. The combination of all 8+ with adaptive thresholds is unusually comprehensive.

 4c. Budget-Aware AI with Degradation Chain

 No other surveyed system dynamically adjusts AI capability based on spend: Sonnet → Qwen → skip enrichment. Commercial tools either have unlimited budgets or fixed tiers.

 4d. Streaming IDF with Exponential Decay

 Standard TF-IDF is batch. The pipeline's streaming variant with decay_factor=0.9999 handles concept drift — rare entities score higher, but old terms decay. Academic literature
 validates per-topic decay but the specific implementation pattern (min total=100 startup floor, periodic cleanup at 1000 events) appears original.

 4e. Self-Hosted Frontier LLM Enrichment

 Commercial tools train custom models. Using Claude/Ollama for structured extraction (entities, relationships, state changes, sentiment) in a single call gives near-frontier
 analytical capability without ML infrastructure. The tiered local→cloud approach with budget management is a novel operational pattern.

 ---
 5. Techniques from Adjacent Domains Worth Adopting

 Tier 1: High Impact, Moderate Effort

 5.1 — Dual Scoring: Severity + Certainty (from Vectra AI NDR)
 Every situation gets both a threat/severity score AND a certainty/confidence score. High severity + low certainty = investigate. High severity + high certainty = act now. Prevents
 both alert fatigue and suppression of important weak signals. Currently the pipeline has severity but not confidence.

 5.2 — Source Reliability Weighting (from IoT Sensor Fusion, military intelligence)
 Weight source contributions in cluster scoring by historical reliability. GeoConfirmed (human-verified) should score higher than raw GDELT (automated NLP). Could be tracked
 per-source in source_health table — accuracy rate over time.

 5.3 — Continuous Anomaly Scoring (from dxFeed Grenadier, financial surveillance)
 Output normalized anomaly scores rather than binary alerts. Every source stream gets a continuous "how unusual is this rate/content?" score. Aggregate across sources for a composite
  anomaly index. Currently burst detection is binary (ratio>2.0 = bonus); continuous scoring would be more nuanced.

 Tier 2: High Impact, Higher Effort

 5.4 — Multi-Window Burst Detection (from Elastic Burst Detection, Zhu & Shasha)
 Monitor multiple sliding window sizes simultaneously (1min, 5min, 30min, 2hr, 12hr). Current dual-EWMA covers 5min and 6hr but misses medium-term patterns. The key insight: you
 don't know the right window size a priori.

 5.5 — Session Windows for Situation Lifecycle (from Apache Flink)
 Dynamic-size windows that end after an inactivity gap proportional to prior activity. A hot situation stays "active" with a short gap tolerance; a quiet situation times out faster.
 Could replace or augment the current fixed-timer phase transitions.

 5.6 — Impact Propagation Through Entity Graph (from Supply Chain: Resilinc, Everstream)
 When a disruption hits a node (e.g., power infrastructure attacked), automatically trace downstream impact through the entity graph → hospitals, military bases, airports,
 communication nodes. The graph structure already exists; this adds forward-looking impact assessment.

 Tier 3: Research / Experimental

 5.7 — GNN+RNN Situation Scoring (from Darktrace DIGEST)
 Model situations as graphs (entities=nodes, interactions=edges). GNN extracts structural features, RNN captures temporal evolution, combined model outputs severity score. Would
 require training data but could replace hand-tuned scoring weights.

 5.8 — Change-Point Detection (from Finance: CUSUM, Shiryaev-Roberts)
 Sequential stopping rules that detect distributional shifts with provable false-alarm properties. More rigorous than EWMA for detecting regime changes (e.g., onset of a new crisis).

 5.9 — Causal Rule Mining (from Temporal Knowledge Graphs: ONSEP framework)
 Automatically discover correlation rules from historical data rather than hand-coding them. E.g., discover that "NOTAM + military flight + seismic" patterns precede strikes, without
  a human writing the rule.

 5.10 — Latency-Aware Fusion (from Autonomous Vehicles: Latency-Aware EKF)
 Explicitly model per-source reporting delay in the correlation engine. GDELT ~15min delay, FIRMS ~3hr, real-time streams ~seconds. Out-of-sequence measurements handled via state
 reprocessing rather than simple time windows.

 ---
 6. Comparative Architecture Map

                     SITUATION REPORT vs. LANDSCAPE

 INGEST          [23 sources]     vs.  Recorded Future [1M+ sources]
                                       Nuculair [312 sources]
                                       OpenCTI [300+ connectors]

 CORRELATE       [8 rules + multi-    vs.  Splunk [correlation searches]
                  signal scoring]          ArcSight [CEP engine]
                                          Palantir [analyst-driven ontology]

 ENRICH          [Ollama/Claude        vs.  Dataminr [proprietary LLMs]
                  single-call]             Recorded Future [custom NLP]
                                          OpenCTI Ariane [document AI]

 CLUSTER         [SituationGraph       vs.  EventRegistry [NLP similarity]
                  streaming IDF +          GDELT [mention frequency]
                  8 signals]               Splunk ITSI [episode rules]

 ENTITY GRAPH    [4-layer resolution   vs.  Senzing [entity-centric]
                  + petgraph]              Palantir [defense ontology]
                                          Neo4j (Nuculair) [47M edges]

 NARRATIVE       [Dataminr ReGenAI     vs.  Dataminr [12yr training data]
                  pattern, BLUF format]    Primer [DoD narrative gen]

 VISUALIZE       [MapLibre + dead      vs.  Palantir [full analyst workspace]
                  reckoning + SSE]         Anduril Lattice [3D sensor fusion]
                                          Planet Labs [satellite imagery]

 BUDGET          [Daily caps +         vs.  Nothing comparable found
                  degradation chain]       (commercial tools have fixed tiers)

 ---
 7. Key Academic Frameworks Mapping

 Endsley's Situation Awareness Model

 - Level 1 (Perception): 23 data sources — strong
 - Level 2 (Comprehension): Correlation + entity graph + AI enrichment + clustering — strong
 - Level 3 (Projection): Narratives + alerts — present but weakest area (describes more than predicts)

 JDL Data Fusion Model

 - Level 0 (Source preprocessing): Unicode sanitization, rate limiting, health tracking — covered
 - Level 1 (Object assessment): Entity resolution, position tracking — covered
 - Level 2 (Situation assessment): SituationGraph clustering, correlation rules — covered
 - Level 3 (Impact assessment): Narrative generation, severity scoring — partially covered
 - Level 4 (Process refinement): Feedback loops, adaptive improvement — gap (no analyst feedback loop)

 Hierarchical Correlation (Maosa et al., 2023)

 - Level 1 (Alert aggregation): Raw events → pipeline importance filter
 - Level 2 (Cross-source correlation): 8 correlation rules → Incidents
 - Level 3 (Scenario reconstruction): SituationGraph → Situations with narratives

 All three academic frameworks validate the current architecture while highlighting Level 3 projection / Level 4 feedback as growth areas.

 ---
 8. Summary

 The Situation Report is a genuinely novel open-source system. It occupies an unserved niche: automated multi-domain intelligence fusion with LLM enrichment, budget-aware AI, and
 streaming cluster scoring — all self-hosted. The closest commercial analogs (Recorded Future, Palantir, Dataminr) cost 6-7 figures annually and are analyst-driven rather than fully
 automated.

 The multi-signal scoring engine (8+ dimensions in score_candidate()) is more sophisticated than most SIEM correlation and approaches the complexity of financial surveillance
 algorithms, while the budget-aware AI degradation chain appears to be without precedent.

 The most impactful techniques to adopt from adjacent domains would be:
 1. Severity + certainty dual scoring (from Vectra AI) — prevents alert fatigue
 2. Source reliability weighting (from sensor fusion) — not all sources are equal
 3. Continuous anomaly scoring (from finance) — beyond binary burst detection
