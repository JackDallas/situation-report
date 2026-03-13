# Building production-grade intelligence for sitrep.watch

**sitrep.watch can evolve from an event clustering tool into a genuine intelligence platform by layering seven tightly integrated capabilities—entity knowledge graph, situation lifecycle, temporal hierarchy, report generation, analytics, vector search, and alerting—all within a single PostgreSQL instance on a single desktop machine.** This report synthesizes research from DARPA AIDA/KAIROS, Recorded Future's Intelligence Graph, Dataminr's ReGenAI, Palantir Gotham, NATO intelligence doctrine, ICEWS/GDELT/ACLED event systems, and production open-source implementations to provide concrete schemas, algorithms, prompts, and Rust crate recommendations for each capability. The architecture deliberately avoids adding databases—PostgreSQL with TimescaleDB, PostGIS, pgvector, and ltree extensions handles everything at 720K events/day on a Ryzen 9 7950X with 64GB RAM.

---

## TOPIC 1: Entity knowledge graph in PostgreSQL with petgraph acceleration

### Data model grounded in DARPA AIDA and Recorded Future

The entity profile needs to capture what production intelligence systems track. DARPA AIDA used a hierarchical entity ontology (PER, ORG, GPE, LOC, FAC, WEA, VEH) with coarse-to-fine subtypes, encoded in RDF-based AIDA Interchange Format and linked to Wikidata via the DARPA Wikidata Overlay. Recorded Future's Intelligence Graph uses a dual-graph architecture: an **ontology graph** for stable entity facts and an **event graph** for rapidly-changing observations—scaling to **13B+ entities** with ~3M new entity nodes daily. Palantir Gotham uses a "dynamic ontology" where entity types and relationships can be defined by analysts without code changes.

For 50K–100K entities on a single machine, the recommended architecture is **PostgreSQL as source of truth** with JSONB for flexible properties, plus **petgraph (Rust)** as an in-memory read-optimized graph mirror for real-time traversal. At this scale, 50K nodes with ~500K edges consumes roughly **100MB of RAM** in petgraph's `StableGraph`—trivial for 64GB. Neo4j and TypeDB add operational overhead without the PostGIS/pgvector integration advantages. Apache AGE (Cypher-in-PostgreSQL) is worth watching but less mature.

```sql
CREATE TABLE entities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type     TEXT NOT NULL,  -- 'person','organization','location','weapon_system','military_unit','facility'
    canonical_name  TEXT NOT NULL,
    aliases         TEXT[] NOT NULL DEFAULT '{}',
    wikidata_id     TEXT,
    properties      JSONB NOT NULL DEFAULT '{}',
    location        GEOGRAPHY(Point, 4326),
    embedding       vector(1024),   -- BGE-M3 for similarity
    status          TEXT DEFAULT 'active',
    confidence      REAL NOT NULL DEFAULT 0.5,
    mention_count   INTEGER NOT NULL DEFAULT 0,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_enriched_at TIMESTAMPTZ
);
CREATE INDEX idx_entities_name_trgm ON entities USING gin(canonical_name gin_trgm_ops);
CREATE INDEX idx_entities_embedding ON entities USING hnsw(embedding vector_cosine_ops) WITH (m=16, ef_construction=200);
CREATE INDEX idx_entities_aliases ON entities USING gin(aliases);

CREATE TABLE entity_relationships (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_entity   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    target_entity   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relationship    TEXT NOT NULL, -- 'leadership','membership','alliance','rivalry','geographic_association','supply_chain','family','sponsorship'
    properties      JSONB NOT NULL DEFAULT '{}',
    confidence      REAL NOT NULL DEFAULT 0.5,
    evidence_count  INTEGER NOT NULL DEFAULT 1,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_active       BOOLEAN DEFAULT true,
    UNIQUE(source_entity, target_entity, relationship)
);

CREATE TABLE entity_state_changes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id       UUID NOT NULL REFERENCES entities(id),
    change_type     TEXT NOT NULL,
    previous_state  JSONB,
    new_state       JSONB NOT NULL,
    certainty       TEXT NOT NULL DEFAULT 'alleged', -- 'confirmed','alleged','denied','rumored'
    source_reliability CHAR(1) DEFAULT 'F', -- Admiralty A-F
    info_credibility   CHAR(1) DEFAULT '6', -- Admiralty 1-6
    triggering_event_id UUID,
    detected_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE entity_claims (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id       UUID NOT NULL REFERENCES entities(id),
    claim_type      TEXT NOT NULL,
    claim_value     JSONB NOT NULL,
    source_id       INTEGER NOT NULL,
    source_reliability CHAR(1) DEFAULT 'F',
    info_credibility   CHAR(1) DEFAULT '6',
    corroboration_count INT DEFAULT 1,
    contradiction_count INT DEFAULT 0,
    resolved        BOOLEAN DEFAULT false,
    resolution      JSONB,
    claimed_at      TIMESTAMPTZ,
    ingested_at     TIMESTAMPTZ DEFAULT now()
);
```

### Multilingual entity resolution with BGE-M3

Deduplicating names like "Hezbollah" / "حزب الله" / "Hizballah" / "Party of God" requires a multi-layered pipeline. The EU Joint Research Centre demonstrated that transliterating Arabic to Latin script, normalizing orthography, stripping vowels, then computing bigram cosine similarity achieves **0.875 similarity** between "Condoleezza Rice" and its Arabic transliteration. BGE-M3 (BAAI, 2024) is ideal for the embedding layer: **100+ languages**, 1024-dimensional dense embeddings, 8192-token context, supporting dense, sparse, and ColBERT retrieval simultaneously, running on the RTX 4090 in fp16.

The resolution pipeline should work in four layers:

- **Layer 1 — String normalization + blocking**: Unicode NFC normalization, diacritic removal, consonant-skeleton matching, trigram similarity via `pg_trgm` for candidate generation
- **Layer 2 — Embedding matching**: BGE-M3 embeddings stored in pgvector; HNSW ANN search for candidates with cosine > 0.80
- **Layer 3 — Wikidata linking**: OpenTapioca for lightweight entity linking or direct Wikidata SPARQL queries for QID resolution; QID match = definitive merge
- **Layer 4 — LLM disambiguation**: For ambiguous cases (cosine 0.85–0.95), Claude Haiku as referee with structured JSON merge/no-merge decision

The decision logic: exact Wikidata QID match → auto-merge. Cosine > 0.95 with same entity_type → auto-merge. Cosine 0.85–0.95 → LLM disambiguation queue. Below 0.85 → create new entity with async Wikidata lookup.

### Relationship extraction during enrichment

Research strongly supports using LLMs for relationship extraction. The KGGen approach (arxiv 2502.09956, Feb 2025) uses multi-stage extraction: entities first, then subject-predicate-object relations, then LLM-as-Judge clustering. For sitrep.watch, Claude Haiku should extract relationships during event enrichment using structured output:

```json
{
  "entities": [{"name": "Hassan Nasrallah", "type": "person", "wikidata_qid": "Q156629"}],
  "relationships": [{
    "source": "Hassan Nasrallah", "target": "Hezbollah",
    "type": "leadership", "properties": {"role": "Secretary-General"},
    "confidence": 0.95
  }],
  "state_changes": [{
    "entity": "Hassan Nasrallah", "attribute": "status",
    "from": "alive", "to": "killed",
    "certainty": "alleged", "modality": "realis"
  }]
}
```

The ten OSINT-relevant relationship types, drawn from DARPA AIDA's ACE/ERE frameworks: **leadership**, **membership**, **alliance**, **rivalry**, **geographic_association**, **supply_chain**, **family**, **sponsorship**, **employment**, **communication**. DARPA KAIROS's CHRONOS system demonstrated that pre-defining ~20 schema templates for common scenarios (military conflict, political transition, sanctions, humanitarian crisis) dramatically improves extraction accuracy.

### Entity state change detection uses a hybrid approach

**Tier 1 — Keyword triggers** (fast, free): ACE 2005 event ontology trigger words for LIFE:DIE ("killed", "assassinated"), JUSTICE:ARREST ("detained", "captured"), PERSONNEL:START-POSITION ("appointed", "promoted"), and similar categories. These fire at ingestion time with zero latency.

**Tier 2 — LLM classification** (accurate, ~$0.25/MTok): When a keyword trigger fires, Claude Haiku classifies the event with structured output distinguishing **realis** (actually happened) vs **irrealis** (hypothetical) vs **negated** vs **reported/alleged** vs **denied**. This distinction is critical—DARPA AIDA specifically trained modality detection models for this.

**Tier 3 — Temporal reasoning**: Track entity state over time using `entity_state_changes`. Apply simple rules: an entity can only die once; promotions are typically monotonic; location changes should be geographically plausible.

### Contradictory information uses the NATO Admiralty Code

The gold standard is **STANAG 2511's 6×6 Admiralty Code**: Source Reliability (A–F, from "completely reliable" to "cannot be judged") crossed with Information Credibility (1–6, from "confirmed by independent sources" to "truth cannot be judged"). Initialize source reliability based on type: wire services (Reuters, AP) → B; quality newspapers → B; state media (SANA, IRNA, TASS) → C; verified social media → C–D; anonymous social media → D–E; unknown → F. Track accuracy over time and adjust automatically.

When a new claim contradicts an existing claim for the same entity: compare source reliability, compare corroboration counts, compute composite confidence using `confidence = source_weight × credibility_weight × (1 - 1/2^n)` for n independent corroborations. If both claims have B+ reliability with 2+ corroborations → flag as DISPUTED for analyst review.

---

## TOPIC 2: Situation lifecycle with finite state machine and narrative regeneration

### Five-phase model inspired by Dataminr ReGenAI

Dataminr's ReGenAI (launched April 2024) creates "living document" briefs that dynamically rewrite themselves—not appending updates but **regenerating the entire narrative** with each significant development. This reduces analyst event analysis from 30+ minutes to under 90 seconds.

The recommended phase model is a finite state machine with guard conditions:

| Phase | Entry Signal | Exit Signal |
|---|---|---|
| **EMERGING** | 2+ signals in temporal/spatial window | 3+ independent sources OR event rate > threshold |
| **DEVELOPING** | Corroboration threshold met | Peak event rate reached OR scope stabilized |
| **ACTIVE** | Sustained rate above baseline for >30 min | Rate drops below 50% of peak for >2 hours |
| **DECLINING** | Rate <50% of peak for >2 hours | Rate <10% of peak, no new entities for >6 hours |
| **RESOLVED** | Explicit resolution signal OR 24h inactivity | 72h review period complete → HISTORICAL |

Critically, situations can **re-escalate**: DECLINING can transition back to ACTIVE if the event rate recovers above 70% of peak, and RESOLVED can reopen to DEVELOPING if new events appear. Every transition is logged with a metrics snapshot for audit.

### Phase transition signals and composite escalation scoring

Six signals drive transitions: **event velocity** (rolling 5min/30min/2h windows vs baseline), **source diversity** (number of independent source types), **entity state changes** (key actors changing status), **geographic spread** (bounding box expansion/contraction), **severity escalation** (max severity trend), and **temporal gaps** (duration since last significant event).

These combine into a composite escalation score using weighted signals with a **compounding multiplier** when multiple signals align simultaneously (inspired by StrikeRadar methodology). When 4+ of 6 signals are elevated above 0.6, the multiplier reaches 1.5×, reflecting the intelligence principle that converging indicators are multiplicative, not additive.

```sql
CREATE TYPE situation_phase AS ENUM ('emerging','developing','active','declining','resolved','historical');

CREATE TABLE situations (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title               TEXT NOT NULL,
    phase               situation_phase NOT NULL DEFAULT 'emerging',
    phase_changed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    situation_type      TEXT,
    severity            INTEGER NOT NULL DEFAULT 5,
    hierarchy_path      ltree,
    location            GEOGRAPHY(Geometry, 4326),
    embedding           vector(1024),
    -- Phase transition metrics (updated by background worker)
    event_count_5m      INT DEFAULT 0,
    event_count_30m     INT DEFAULT 0,
    peak_event_rate     FLOAT DEFAULT 0,
    current_event_rate  FLOAT DEFAULT 0,
    source_diversity    INT DEFAULT 0,
    max_severity        INT DEFAULT 0,
    last_significant_at TIMESTAMPTZ,
    -- Narrative
    narrative_text      TEXT,
    narrative_version   INT NOT NULL DEFAULT 0,
    properties          JSONB NOT NULL DEFAULT '{}',
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at         TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_situations_hierarchy ON situations USING gist(hierarchy_path);
CREATE INDEX idx_situations_embedding ON situations USING hnsw(embedding vector_cosine_ops);

CREATE TABLE situation_phase_transitions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    situation_id    UUID NOT NULL REFERENCES situations(id),
    from_phase      situation_phase NOT NULL,
    to_phase        situation_phase NOT NULL,
    trigger_reason  TEXT NOT NULL,
    metrics_snapshot JSONB NOT NULL,
    transitioned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE situation_narratives (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    situation_id    UUID NOT NULL REFERENCES situations(id),
    version         INT NOT NULL,
    narrative_text  TEXT NOT NULL,
    phase           situation_phase NOT NULL,
    model_used      TEXT NOT NULL,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(situation_id, version)
);
```

### Narrative generation follows Dataminr's regeneration pattern

When a significant event arrives, the system retrieves all situation events (or top-N by significance), the previous narrative, phase, and entity context from the knowledge graph, then regenerates the complete narrative via Claude Sonnet. The prompt structure enforces intelligence writing standards:

**BLUF** (2–3 sentences: what, so what, what next) → **TIMELINE** (key developments chronologically) → **CURRENT STATUS** (phase-appropriate assessment) → **KEY UNCERTAINTIES** → **INDICATORS TO WATCH**. Every narrative version is stored for diff-based catch-up generation and audit.

### "What happened while you were away" uses hybrid diff + narrative

The system takes hourly situation snapshots. When an analyst returns, it computes diffs between snapshots at `away_since` and `now()` for all followed situations, ranks by change significance (phase transitions > new situations > significant developments), and generates a prioritized briefing via Claude Sonnet. The briefing structure: Executive Summary → Critical Changes → Significant Developments → Routine Updates → Recommended Actions.

### Military doctrine integration via CCIR framework

NATO's Commander's Critical Information Requirements (CCIR) map directly to the platform: **PIRs** (Priority Intelligence Requirements) → situation watchlists and alert rules; **decision points** with thresholds → phase transition guards. A `intelligence_requirements` table tracks which intelligence questions are active, links them to situations, and tracks which events provide indicators that answer or contradict them.

---

## TOPIC 3: Multi-scale temporal hierarchy from events to campaigns

### Three-tier architecture with ltree materialized paths

The dominant OSINT event systems (GDELT, ICEWS, ACLED) use flat event models with hierarchical coding but no built-in event composition. sitrep.watch can differentiate by implementing explicit hierarchical composition. The architecture uses **five levels**: events → incidents (hours) → situations (days–weeks) → campaigns (weeks–months) → crises (months–years).

PostgreSQL's `ltree` extension provides materialized path indexing with GiST indexes for sub-millisecond tree traversal queries like `WHERE hierarchy_path <@ 'campaign.russia_ukraine'`. This replaces the need for recursive CTEs on most read operations.

```sql
CREATE EXTENSION IF NOT EXISTS ltree;

CREATE TABLE hierarchy_nodes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    level           TEXT NOT NULL CHECK (level IN ('incident','situation','campaign','crisis')),
    parent_id       UUID REFERENCES hierarchy_nodes(id),
    tree_path       ltree,
    title           TEXT NOT NULL,
    summary         TEXT,
    status          TEXT DEFAULT 'active',
    first_event_at  TIMESTAMPTZ NOT NULL,
    last_event_at   TIMESTAMPTZ NOT NULL,
    centroid        GEOGRAPHY(Point, 4326),
    primary_country CHAR(3),
    event_count     INTEGER DEFAULT 0,
    primary_actors  TEXT[],
    primary_themes  TEXT[],
    coherence_score REAL,
    created_at      TIMESTAMPTZ DEFAULT now(),
    updated_at      TIMESTAMPTZ DEFAULT now()
);
CREATE INDEX idx_hierarchy_tree ON hierarchy_nodes USING GIST(tree_path);
```

### CAMEO-compatible event coding for ecosystem interoperability

CAMEO organizes **310 event classes** into a 4-tier hierarchy: 20 root codes (2-digit, ordinal from cooperation 01–09 to conflict 10–20), ~70 secondary codes, ~200+ tertiary codes. Each code maps to a **Goldstein Scale** value (–10 most conflictual to +10 most cooperative) and a **Quad Classification** (Verbal Cooperation, Material Cooperation, Verbal Conflict, Material Conflict). sitrep.watch should store full 4-digit CAMEO codes but aggregate by 2-digit root codes at the situation level and quad class at the campaign level. This enables interoperability with GDELT, ICEWS, and Phoenix datasets.

### Grouping signals use five weighted factors

From OSINT clustering research, five signals determine when incidents roll up into situations, weighted by importance: **shared entities** (~40%), **geographic proximity** (~25%, using H3 hexagonal grids at resolution 4 for ~22km uniform cells), **temporal clustering** (~15%), **thematic similarity** (~15%, CAMEO root code overlap + embedding cosine similarity), and **causal chains** (~5%).

At the **incident level** (hours), use online incremental clustering with in-memory micro-clusters. At the **situation level** (days–weeks), run **DBSCAN** over incident feature centroids every 6 hours—DBSCAN's advantage is not requiring a pre-specified cluster count and handling noise (isolated incidents). At the **campaign level** (weeks–months), use agglomerative hierarchical clustering with entity-based merging: if two situations share >60% of their top-5 actors, consider merging.

### Avoiding mega-clusters through coherence monitoring

Long-running conflicts risk becoming mega-clusters that absorb all events in a region. The solution: **hierarchical decomposition with temporal phases**. When a situation's temporal extent exceeds 30 days or its event count exceeds a configurable threshold, auto-subdivide using change-point detection (PELT algorithm) on the daily event count time series. Track each node's coherence score; when it drops below threshold (geographic spread too wide, too many distinct actor groups), flag for decomposition. A campaign is a tree of situations with temporal phases, not a flat cluster.

---

## TOPIC 4: Intelligence products from flash alerts to deep-dive reports

### Six report types mapped to NATO standards and Bloomberg patterns

| Report Type | Model | NATO Equivalent | Bloomberg Analog | Latency |
|---|---|---|---|---|
| **Flash Alert** | Haiku | TACREP | Breaking News 3-bullet | <3s |
| **INTREP** (event-driven) | Sonnet | INTREP (J110) | News Alert | <10s |
| **INTSUM** (periodic) | Sonnet | INTSUM (J111) | Daybreak/Morning Report | <30s |
| **SUPINTREP** (deep dive) | Sonnet | SUPINTREP (J114) | BI Research | <60s |
| **Entity Profile** | Sonnet | — | Company Profile | <15s |
| **Campaign Assessment** | Sonnet | — | Sector Analysis | <60s |

The universal OSINT report structure follows DutchOSINTGuy's professional framework: Title Page → BLUF → Introduction → Methodology → Findings → Analysis → Conclusion & Recommendations → Annexes. For AI-generated reports, the system prompt enforces intelligence writing standards from the CIA/DIA Style Manual: main point up front, short paragraphs, active voice, distinguish fact from assessment, include confidence levels for all analytical judgments, cite sources for all factual claims.

### Prompting patterns prevent hallucination through grounding

The critical anti-hallucination pattern: entity relationships and facts **must come from the provided entity graph context**, never from the LLM's training data. Every prompt includes: `"Relationships and facts about entities MUST come from the provided entity_context JSON. Do not infer or fabricate entity information."` Post-generation validation in Rust checks that all entity names and relationships in output exist in input context, all source references exist in input situation data, and confidence language matches the overall confidence rating.

The prompting architecture uses **tiered models**: Claude Haiku for flash alerts, event classification, and high-volume low-complexity tasks; Claude Sonnet for INTSUM, SUPINTREP, entity profiles, and anything requiring analytical reasoning. Context assembly follows a pipeline: situation cluster → extract entity_ids → query entity graph for each entity → get 1-hop relationships → get recent situations per entity → serialize to JSON context → inject into prompt template.

### STIX-inspired structured output schema

Every report has both machine-readable JSON and rendered narrative, inspired by STIX 2.1's Report SDO. The schema includes `report_type`, `classification` (TLP markings), `confidence` (ICD 203 language: almost certainly >95%, likely 55–80%, roughly even chance, unlikely, remote), `bluf`, `sections` array with typed content, `entities_mentioned` with roles, `situation_refs`, `entity_refs`, `source_refs`, and `generation_metadata` tracking model, prompt template version, and token counts.

### Export pipeline uses typst for PDF, Tera for HTML

The recommended pipeline: Claude API → Structured JSON → three parallel outputs. For **JSON**, serve directly via API. For **HTML**, use the `tera` crate (runtime Jinja2-like templates, updatable without recompilation) with `comrak` for Markdown-to-HTML conversion, delivered via `lettre` for email. For **PDF**, embed the `typst` Rust library (Apache-2.0 license) for high-quality document rendering with proper layout, page breaks, headers/footers, and tables—far superior to `printpdf` for structured intelligence documents.

---

## TOPIC 5: Analytics powered by TimescaleDB continuous aggregates

### TimescaleDB handles the full workload without cold-path storage initially

At **720K events/day**, a 90-day window contains ~65M rows. With TimescaleDB's compression enabled after 7 days (achieving **90–95% compression**), this collapses to ~6.5GB. Cloudflare chose TimescaleDB over ClickHouse for analytics at higher volumes. Continuous aggregates pre-compute common queries so dashboards hit thousands of rows, not millions, delivering **<50ms** response times. DataFusion/Parquet should be implemented as an optional cold-path for data older than 90 days but is not required for the primary dashboard.

Storage budget with compression: raw events (90 days) ~6.5GB compressed, 5min aggregate (30 days) ~200MB, hourly aggregate (indefinite) ~50MB/year, daily aggregate (indefinite) ~5MB/year. **Total: ~7–8GB for full operational data.**

### Continuous aggregate hierarchy and additional aggregates needed

The existing 5 aggregates (5min/15min/hourly/daily/anomaly baseline) map to API endpoints. Three additional aggregates are needed:

```sql
-- Geographic rollup for heatmap API
CREATE MATERIALIZED VIEW events_geo_hourly
WITH (timescaledb.continuous) AS
SELECT time_bucket('1 hour', time) AS bucket,
    country_code, ROUND(ST_Y(location::geometry)::numeric,1) AS lat_grid,
    ROUND(ST_X(location::geometry)::numeric,1) AS lon_grid,
    COUNT(*) AS event_count, COUNT(DISTINCT source_id) AS source_count
FROM events GROUP BY 1,2,3,4;

-- Entity mention frequency
CREATE MATERIALIZED VIEW entity_mentions_hourly
WITH (timescaledb.continuous) AS
SELECT time_bucket('1 hour', m.time) AS bucket, m.entity_id,
    COUNT(*) AS mention_count, COUNT(DISTINCT e.source_id) AS source_count
FROM event_entity_mentions m JOIN events e ON m.event_id = e.id
GROUP BY 1,2;

-- Source health monitoring
CREATE MATERIALIZED VIEW source_health_15min
WITH (timescaledb.continuous) AS
SELECT time_bucket('15 minutes', time) AS bucket, source_id,
    COUNT(*) AS event_count
FROM events GROUP BY 1,2;
```

Compression configuration: segment by region/event_type, order by time DESC, compress after 7 days, retain raw data for 90 days, retain 5min aggregates for 30 days, keep hourly/daily indefinitely.

### Anomaly detection via z-score baselines

A `anomaly_baselines` table stores mean, stddev, and p95 per (region, event_type, day_of_week, hour_of_day), refreshed daily from the last 28 days of hourly aggregates. Real-time anomaly detection computes z-scores against baselines: **|z| > 3** → CRITICAL, **|z| > 2** → WARNING. Rate-of-change detection flags when a 15min bucket exceeds 2× the previous bucket with the previous bucket having >5 events. Source silence detection (event count drops to <20% of mean) catches potentially suspicious feed outages.

### Dashboard layout follows Bloomberg Terminal and Grafana SOC patterns

The recommended layout for SvelteKit:

- **Top row**: KPI stat cards with sparklines (events/hour, active anomalies, active situations, source health %, top severity gauge)
- **Row 2**: Time-series stacked area chart by severity + anomaly bands (from 5min/15min aggregates via `uPlot`)
- **Row 3 split**: MapLibre map (left) with three zoom-adaptive layers (heatmap at z0–7, clusters at z5–12, individual severity-colored markers at z10+) | Top-N tables (right) for active situations, top entities, top regions, source reliability
- **Row 4**: Situation lifecycle Gantt chart + real-time scrolling event feed via SSE

All components share filter state through Svelte stores, with map clicks filtering all other components to the selected region.

### API endpoint structure

```
GET  /api/v1/analytics/events/timeseries?resolution={5min|15min|hourly|daily}&from=&to=&regions=&group_by=
GET  /api/v1/analytics/anomalies
GET  /api/v1/analytics/geo/heatmap?from=&to=&bounds=
GET  /api/v1/analytics/entities/:id/timeline
GET  /api/v1/analytics/entities/top
GET  /api/v1/analytics/sources/health
GET  /api/v1/analytics/situations
POST /api/v1/analytics/historical  (cold-path DataFusion queries)
WS   /api/v1/ws/events             (real-time event stream)
WS   /api/v1/ws/anomalies          (real-time anomaly alerts)
```

Response time targets: continuous aggregate queries <100ms, geo heatmap <300ms, entity timeline <200ms, cold-path DataFusion <10s.

---

## TOPIC 6: Vector search combining pgvector, spatial, and graph traversal

### pgvector is sufficient—stay with it

At the scale of sitrep.watch (<10M vectors in the active window), pgvector with HNSW matches Qdrant latency at approximately **5ms p50**. The critical upgrade: pgvector 0.8.0's **iterative index scans** (`SET hnsw.iterative_scan = 'relaxed_order'`) automatically retrieve more candidates when post-filters are selective, solving the main filtered search problem. Adding a separate vector database doubles operational complexity for negligible performance gain at this scale.

Memory consideration: 1024-dim × 4 bytes × 22M events (30 days) ≈ **90GB** for full-precision vectors—tight for 64GB RAM. Use **halfvec** (2 bytes/dim) to halve index memory to ~45GB, or limit the HNSW-indexed window to 14–21 days with brute-force search for older events. The `pg_prewarm` extension keeps the HNSW index in memory.

### Three core search patterns

**"Find events similar to X"**: Embed the reference event or query text via BGE-M3, then ORDER BY cosine distance with temporal/spatial filters. The critical SQL pattern combines PostGIS spatial filtering with pgvector ordering:

```sql
SET hnsw.iterative_scan = 'relaxed_order';
SELECT e.id, e.title, (e.embedding <=> $1::vector) AS distance
FROM events e
WHERE e.time > NOW() - INTERVAL '7 days'
  AND ST_DWithin(e.location::geography, ST_MakePoint($2,$3)::geography, 100000)
ORDER BY e.embedding <=> $1::vector LIMIT 20;
```

**"Find situations involving entities related to Hezbollah"**: Two-phase graph+vector approach. Phase 1: recursive CTE traverses entity_relationships to find 1–2 hop neighbors. Phase 2: filter events by entity involvement, then ORDER BY vector similarity to the query embedding.

### Hybrid search via Reciprocal Rank Fusion

Start with PostgreSQL's native `tsvector` + GIN index (zero additional dependencies). Generate a stored tsvector column, run separate BM25 and vector searches in CTEs, combine via RRF with **0.6 semantic / 0.4 lexical weighting** (OSINT analysts search conceptually more than by exact terms). Upgrade to ParadeDB's `pg_search` extension when BM25 precision matters. Tantivy as a Rust-native BM25 engine is a viable alternative if avoiding PostgreSQL extension dependencies.

```sql
WITH
fulltext AS (
  SELECT id, ROW_NUMBER() OVER (ORDER BY ts_rank_cd(content_tsv, query) DESC) AS rank
  FROM events, websearch_to_tsquery('english', $2) query
  WHERE content_tsv @@ query LIMIT 20
),
semantic AS (
  SELECT id, ROW_NUMBER() OVER (ORDER BY embedding <=> $1::vector) AS rank
  FROM events LIMIT 20
),
rrf AS (
  SELECT id, 0.6/(60+rank) AS score FROM semantic
  UNION ALL SELECT id, 0.4/(60+rank) FROM fulltext
)
SELECT e.*, SUM(rrf.score) AS combined_score
FROM rrf JOIN events e USING (id) GROUP BY e.id ORDER BY combined_score DESC LIMIT 10;
```

---

## TOPIC 7: Four-tier alerting with fatigue prevention

### Alert architecture from rules to AI-driven signals

SOC research shows **62–83% of alerts are eventually ignored** due to volume, with false positive rates reaching 99% in some environments. sitrep.watch must design for fatigue prevention from day one.

**Tier 1 — Rule-based** (implement first): Keyword matches, entity mentions, geo-fence violations. These fire on raw events with <5s latency. Stored in an `alert_rules` table with JSONB conditions for flexibility.

**Tier 2 — Entity-driven**: "Alert when any Hezbollah leadership entity has a state change." Requires entity graph integration—when a new event mentions a watched entity or triggers a state change, evaluate against entity-monitoring rules. Implementation uses `?|` operator on JSONB `entity_ids` arrays.

**Tier 3 — Anomaly-based**: Baseline event rates per region/entity/type; alert on z-score >2. Run as a periodic batch job every 15 minutes using the anomaly_baselines table.

**Tier 4 — Semantic/AI-driven**: "Alert me when events similar to this pattern occur." Compare new event embeddings against saved reference embeddings; trigger when cosine similarity > 0.75.

### Situation-level alerting dramatically reduces noise

The default should be **situation phase change alerts** rather than raw event alerts. Alert when a situation transitions (emerging → developing → active) rather than on every individual event—this provides **10–100× noise reduction**. Escape hatch: critical events (mass casualty, WMD indicators) always alert immediately at Tier 1.

```sql
CREATE TABLE alert_rules (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    rule_type       TEXT NOT NULL CHECK (rule_type IN ('keyword','entity','geo_fence','semantic','anomaly','composite')),
    conditions      JSONB NOT NULL,
    delivery        JSONB DEFAULT '["sse"]',
    enabled         BOOLEAN DEFAULT true,
    cooldown        INTERVAL NOT NULL DEFAULT '30 minutes',
    max_per_hour    INTEGER DEFAULT 10,
    min_severity    TEXT DEFAULT 'medium',
    last_fired_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE alert_notifications (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id         UUID NOT NULL REFERENCES alert_rules(id),
    event_id        UUID,
    situation_id    UUID,
    severity        TEXT NOT NULL,
    title           TEXT NOT NULL,
    summary         TEXT,
    delivered_via   TEXT[],
    delivered_at    TIMESTAMPTZ DEFAULT now(),
    acknowledged_at TIMESTAMPTZ,
    dismissed_at    TIMESTAMPTZ
);
```

### Fatigue prevention mechanisms

Five mechanisms: **tiered severity with distinct UX** (critical = full-screen notification; medium = feed item, no interrupt; low = digest email); **cooldown windows** per rule (default 30 minutes); **deduplication** via situation-scoped fingerprinting (don't re-alert on events already part of a known active situation); **adaptive thresholds** that auto-increase severity thresholds if acknowledgment rate drops below 20%; **weekly digest** of rules with high trigger/low acknowledge ratios prompting review.

### Delivery via SSE with broadcast channels

The minimum viable delivery system uses Axum's built-in SSE support with `tokio::sync::broadcast` for fan-out to multiple clients. The pipeline: Event ingested → Alert Evaluator → Matched Rules → Dedup Check → Delivery Router → SSE/Webhook/Email/Slack. Webhook delivery uses `reqwest` with exponential backoff retry; email uses `lettre`; Slack uses webhook URL POST.

```rust
// Axum SSE endpoint pattern
async fn alert_stream(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.alert_tx.subscribe();
    let stream = async_stream::try_stream! {
        loop {
            match rx.recv().await {
                Ok(notification) => {
                    yield Event::default().event("alert").json_data(&notification).unwrap();
                },
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    yield Event::default().event("system").data(format!("Missed {n} alerts"));
                },
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
}
```

---

## How all seven systems interconnect

The entity knowledge graph is the **foundational layer** that feeds everything else. Entities extracted from events become graph nodes; relationships become edges. The situation lifecycle aggregates events by related entities and tracks phase transitions driven by entity state changes. The temporal hierarchy nests situations into campaigns using entity overlap as the primary grouping signal (~40% weight). Reports draw entity context from the graph to ground AI-generated narratives. Analytics queries the graph for entity activity timelines. Vector search combines with graph traversal for entity-scoped similarity. Alerts fire on entity state changes and situation phase transitions.

```
[SOURCES] → [EVENT INGESTION] → [EMBEDDING + NER] → [ENTITY GRAPH]
                                                          ↓
                      ┌──────────────────────────────────────────────────┐
                      ↓                    ↓              ↓              ↓
               [SITUATIONS]          [VECTOR SEARCH]  [ANALYTICS]  [ALERTING]
                      ↓                    ↓              ↓              ↓
                      └────────────────────┴──────────────┴──────────────┘
                                           ↓
                                       [REPORTS]
```

The data flow is unified through PostgreSQL. Real-time flow uses `PG LISTEN/NOTIFY` + `tokio::sync::broadcast` → Axum SSE → SvelteKit EventSource. No Redis, Elasticsearch, or Neo4j needed at this scale.

### Pipeline architecture with bounded channel backpressure

```
[27 Source Fetchers] → unbounded → [Dedup/Normalize] → bounded(1000)
  → [Embedding Worker (ort + BGE-M3 + CUDA, ~150 emb/sec)] → bounded(500)
  → [Entity Extraction (rust-bert NER fast + Claude Haiku rich)] → bounded(200)
  → [Clustering & Situation Assignment] → bounded(100)
  → [Alert Evaluation] → [SSE Broadcast Hub]
```

Embedding throughput is **18× headroom** over requirements (150 emb/sec vs 8.3 needed). Entity extraction uses a hybrid strategy: fast-path `rust-bert` local NER during embedding pass for immediate Person/Location/Organization extraction; rich-path Claude Haiku as a separate enrichment pass for relationships, event classification, and situation assignment (8 concurrent calls via `tokio::sync::Semaphore`, ~240–960 events/min). Cost optimization: use local NER for basic extraction and reserve Claude for the top **10–20%** of events by novelty score, bringing estimated daily cost to **$20–50**.

---

## Rust crate ecosystem for all seven features

| Component | Crate | Version | Purpose |
|---|---|---|---|
| **Graph** | `petgraph` | 0.8+ | In-memory StableGraph, BFS/DFS, connected components |
| **Embeddings** | `ort` | 2.x | ONNX Runtime with CUDA EP for BGE-M3 on RTX 4090 |
| **NER** | `rust-bert` | 0.23+ | Local NER pipeline (Person/Location/Organization) |
| **Full-text** | `tantivy` | 0.25+ | Rust-native BM25 search engine (2× faster than Lucene) |
| **HTTP API** | `axum` | 0.8+ | Web framework with SSE, WebSocket, middleware |
| **Database** | `sqlx` | 0.8+ | Async PostgreSQL with compile-time query checking |
| **PDF** | `typst` | 0.14+ | Embeddable document renderer with templates |
| **HTML** | `tera` + `comrak` | 1.x + 0.28+ | Runtime templating + Markdown→HTML |
| **Email** | `lettre` | 0.11+ | SMTP email delivery |
| **HTTP client** | `reqwest` | 0.12+ | Source fetching, webhook delivery |
| **Async** | `tokio` | 1.x | Runtime, channels (mpsc, broadcast), semaphores |
| **Geo** | `h3o` + `geo` | — | H3 hexagonal grid indexing + geographic computation |
| **Clustering** | `linfa` | — | DBSCAN, agglomerative clustering |
| **Serialization** | `serde` + `serde_json` | 1.x | JSON/JSONB |
| **Analytics** | `datafusion` + `arrow` + `parquet` | 44+ / 54+ | Cold-path Parquet queries |
| **Streaming** | `async-stream` + `tokio-stream` | 0.3+ / 0.1+ | SSE stream construction |
| **Text** | `unicode-normalization` | — | NFC/NFD for Arabic/Farsi |

---

## PostgreSQL performance tuning for 64GB RAM

```ini
shared_buffers = 16GB
effective_cache_size = 48GB
work_mem = 64MB
maintenance_work_mem = 2GB
wal_buffers = 64MB
huge_pages = try
max_wal_size = 8GB
max_connections = 100
max_parallel_workers_per_gather = 4
max_parallel_workers = 16
timescaledb.max_background_workers = 8
```

Memory budget: PostgreSQL shared_buffers **16GB**, OS page cache **~20GB**, Rust application **~4GB**, petgraph in-memory graph **~2–4GB**, tantivy index (mmap'd) **~2GB**, headroom/OS **~16–18GB**. GPU budget: BGE-M3 ONNX fp16 **~2GB VRAM**, local NER model **~1.5GB**, leaving **~20GB** available for future models.

---

## Implementation priority for a single developer

The ordering follows the OSINT intelligence cycle: Collection → Enrichment → Entity Extraction → Relationship Mapping → Situation Tracking → Reporting/Alerting.

**Phase 1: Foundation — "Ingest & View."** Event ingestion into hypertable, basic Axum API, SvelteKit frontend with event list and MapLibre map. Validates architecture and data flow.

**Phase 2: Embeddings & Search — "Find & Relate."** BGE-M3 via ort+CUDA, pgvector HNSW index, semantic search API, basic hybrid search with tsvector. Core differentiator that makes the data navigable.

**Phase 3: Entity Graph — "Understand Actors."** Entity extraction via Claude Haiku + rust-bert, entity resolution with BGE-M3 similarity, petgraph mirror, relationship extraction, graph traversal API. Entities are the foundation for all intelligence products.

**Phase 4: Situation Lifecycle — "Track What Matters."** Event clustering, situation creation from clusters, ltree hierarchy, phase FSM with transition guards, narrative generation via Claude Sonnet, SSE real-time updates. Transforms raw events into actionable intelligence.

**Phase 5: Alerting — "Never Miss Critical Events."** Alert rule engine (keyword → entity → geo-fence → semantic), SSE delivery, alert history and acknowledgment, deduplication, cooldowns.

**Phase 6: Reports — "Deliver Intelligence Products."** Typst PDF templates, Claude Sonnet report generation with anti-hallucination validation, INTSUM scheduling, catch-up briefings, entity profiles.

**Phase 7: Analytics — "Measure & Predict."** Activate all continuous aggregates, build dashboard API endpoints, anomaly detection baselines, trend analysis, source reliability scoring, DataFusion cold-path for historical queries.

Each phase delivers usable value independently. Phase 3 (entity graph) is the critical inflection point—it unlocks the intelligence capabilities that differentiate sitrep.watch from a mere event aggregator. The entire sequence depends on Phase 1's ingestion pipeline being solid, so investing heavily in getting the event pipeline right pays compound returns across all subsequent phases.
