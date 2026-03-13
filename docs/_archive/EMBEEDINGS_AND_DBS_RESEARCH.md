# sitrep.watch Infrastructure Architecture: Database, Embeddings, Local Models & Deployment

**Follow-up to: Event Clustering Architectures for Real-Time OSINT Fusion**
**Hardware baseline:** AMD Ryzen 9 7950X (16c/32t), 64GB DDR5, NVIDIA RTX 4090 (24GB VRAM, ~20GB free), 930GB NVMe, Bazzite (Fedora Atomic)
**Workload:** 720K events/day, ~500 events/min sustained, 26 heterogeneous OSINT sources, 30-day retention (21.6M vectors)

---

## 1. The 64GB memory wall

At 21.6M vectors with 30-day retention, memory is the binding constraint. Raw numbers for float32 HNSW (M=16):

| Dimensions | Vector size | Raw vectors | HNSW index | Total | Fits 64GB? |
|---|---|---|---|---|---|
| 1024 | 4,096 bytes | 82.4 GB | 93.5 GB | 175.9 GB | No |
| 768 | 3,072 bytes | 61.8 GB | 71.5 GB | 133.3 GB | No |
| 512 | 2,048 bytes | 41.2 GB | 48.5 GB | 89.7 GB | No |
| 384 | 1,536 bytes | 30.9 GB | 36.8 GB | 67.7 GB | Barely |

**Breakpoint for float32 HNSW in 64GB: ~691 dimensions.** Dimension reduction and quantization are mandatory. With int8 scalar quantization at 512 dimensions, the HNSW index drops to approximately 16GB — leaving 48GB for event payloads, geospatial indexes, application, and OS. This is the target configuration.

---

## 2. Embedding model selection

### Recommendation: BGE-M3 with Matryoshka truncation to 512 dimensions

**Why BGE-M3 over alternatives:**

BGE-M3 (568M params, MIT license) covers all seven target languages (English, Russian, Ukrainian, Arabic, Hebrew, Farsi, Chinese) with strong multilingual benchmarks (MIRACL 56.8). Its 8192-token context window safely handles the longest OSINT events — the 512-token limit of mE5-large-instruct and Nomic v2 is a real constraint when enrichment summaries push past 500 tokens. The dense+sparse+ColBERT multi-granularity output provides a natural path to hybrid retrieval where sparse vectors catch exact entity mentions (weapon types, transliterated proper nouns like "ХАМАС"/"Hamas") that dense embeddings may conflate.

| Model | Params | Dims | Context | Languages | License | RTX 4090 throughput |
|---|---|---|---|---|---|---|
| **BGE-M3** | 568M | 1024 | 8192 | All 7 target | MIT | 300–500/s |
| mE5-large-instruct | 560M | 1024 | 512 ⚠️ | Strong multilingual | MIT | 300–500/s |
| Jina v3 | 570M | 1024 | 8192 | 89 languages | CC-BY-NC ⚠️ | 300–500/s |
| Nomic v2 | 475M | 768 | 512 ⚠️ | ~100 (unvalidated) | Apache 2.0 | 400–700/s |
| **Qwen3-Embedding-0.6B** | 600M | Flexible | 32768 | 100+ languages | Apache 2.0 | 500–1000/s |

**Qwen3-Embedding-0.6B is the dark horse.** Released June 2025, it matches quality of the much larger GTE-Qwen2-7B while running at 500–1000 embeddings/sec on RTX 4090. Native Matryoshka support with flexible dimensions (32–1024) and instruction-aware architecture. The 0.6B model ranks just behind Google's Gemini-Embedding on MMTEB despite being a fraction of the size. If multilingual OSINT benchmarks confirm quality parity with BGE-M3, this becomes the default choice. Apache 2.0 license eliminates Jina v3's commercial restriction.

**Why 7B embedding models are ruled out:** GTE-Qwen2-7B requires ~14GB VRAM at FP16, leaving almost no room for batch processing. It achieves only 8–25 embeddings/sec with INT4 quantization — barely meeting the 8.3/sec sustained requirement with no headroom. The quality advantage over well-tuned 500M–600M models has narrowed significantly through 2025.

### Dimension reduction and quantization pipeline

Generate 1024-dim embeddings with BGE-M3, then Matryoshka-truncate to 512 dimensions (slice first 512 values — works because Matryoshka training front-loads information into early dimensions), then int8 scalar quantize for HNSW storage.

At 512-dim int8, each vector occupies 512 bytes. Full 30-day HNSW index requires approximately 16GB RAM. Matryoshka research consistently shows less than 1% quality degradation at half dimensions. Int8 scalar quantization retains 99%+ recall when combined with rescore multiplier of 4–5×.

**Instruction prefix for clustering quality:** Use `"Represent this conflict event for clustering by topic, location, and actors: "` on all inputs. E5 paper confirms incorporating instructions has a considerable impact on performance.

### Inference deployment

**HuggingFace Text Embeddings Inference (TEI)** as Docker sidecar on RTX 4090. TEI is written in Rust (Axum + Candle/ORT backends), supports Flash Attention and dynamic batching, exposes OpenAI-compatible `/v1/embeddings` HTTP endpoint.

```bash
docker run --gpus all -p 8080:80 \
  ghcr.io/huggingface/text-embeddings-inference:89-1.9 \
  --model-id BAAI/bge-m3
```

Tag `89-*` is optimised for Ada Lovelace (RTX 4090 architecture). Expected throughput: 500–1500 embeddings/sec for BGE-M3 at FP16 with 100–200 token inputs = 60–180× the required 8.3/sec. Model weights consume only ~2.2GB VRAM, leaving 17+ GB free for batch sizes and other models.

**Alternative: ort crate** (ONNX Runtime Rust bindings, v2.0) with CUDA or TensorRT execution providers for tighter in-process integration. Export model: `optimum-cli export onnx --model BAAI/bge-m3 --task feature-extraction --optimize O3`.

---

## 3. Reranker selection for cluster quality scoring

Rerankers significantly improve clustering quality by rescoring candidate matches from the HNSW nearest-neighbor search. The best reranker in recent benchmarks lifted Hit@1 from 62.67% to 83.00% — a transformative gain for a component that adds under 250ms of latency.

### Recommendation: Qwen3-Reranker-0.6B (primary) or jina-reranker-v3 (if latency critical)

**Top open-source rerankers ranked by quality/efficiency for local deployment:**

| Model | Params | Architecture | Hit@1 | Latency | Multilingual | License |
|---|---|---|---|---|---|---|
| **NVIDIA nemotron-rerank-1b** | 1.2B | Prompt-template | 83.0% | 243ms | Limited | Custom |
| **gte-reranker-modernbert-base** | 149M | SequenceClassification | 82.3% | 58ms | Limited | Apache 2.0 |
| **jina-reranker-v3** | 600M | Late interaction (AAAI 2026) | 81.3% | 188ms | 93 languages | CC-BY-NC ⚠️ |
| **Qwen3-Reranker-0.6B** | 600M | Causal yes/no logit | 77.7% | ~800ms | 100+ languages | Apache 2.0 |
| bge-reranker-v2-m3 | 568M | Cross-encoder | ~76% | ~200ms | Multilingual | MIT |

**Key findings from benchmarks:**

Model size does not determine reranker quality. The 149M-parameter gte-reranker-modernbert-base matches the 1.2B nemotron on Hit@1. The 4B Qwen3-Reranker finished fourth. For production systems, start with smaller models.

**For OSINT clustering specifically**, multilingual capability is essential — the reranker must handle Russian/Arabic/Chinese event pairs. This narrows the field to Qwen3-Reranker (0.6B/4B/8B, Apache 2.0, 100+ languages) and jina-reranker-v3 (600M, 93 languages, CC-BY-NC). Qwen3-Reranker-0.6B exceeds previously top-performing models across numerous retrieval tasks, and the 8B variant improves by another 3.0 points.

**Practical deployment on RTX 4090:** The Qwen3-Reranker-0.6B uses causal language modelling (yes/no logit approach), which is slower per-query than cross-encoders but highly parallelisable with vLLM. At ~1.2GB VRAM for the 0.6B model, it runs alongside the embedding model with plenty of headroom. For the async cluster quality scoring in Phase 2 of the pipeline (not latency-critical), the ~800ms per query is acceptable — you are rescoring 3–15 candidate clusters per event, not thousands.

**Where reranking fits in the clustering pipeline:** After HNSW returns top-K (K=10) candidate cluster centroids in Phase 1, the reranker rescores each (event, centroid) pair to produce a refined similarity score before the merge/split decision. This replaces or augments the cosine similarity component of the composite score function. The reranker captures cross-attention signals that cosine similarity on embeddings misses — particularly important for multilingual event pairs where the same incident is described in different languages.

---

## 4. Local enrichment models to replace Claude Haiku

The enrichment pipeline currently uses Claude Haiku API ($0.80/$4.00 per million tokens) to extract entities, topics, sentiment, and relevance scores from raw events. At 720K events/day with ~500 input + ~200 output tokens per event, this costs approximately $26/month. The question is whether a local model on the RTX 4090 can match Haiku's extraction quality for structured JSON output.

### Recommendation: Qwen3-4B (primary) or Gemma 3 4B (alternative)

**Model comparison for structured entity/topic extraction on RTX 4090:**

| Model | Params | VRAM (FP16) | VRAM (Q4) | RTX 4090 tok/s (Q4) | Multilingual | JSON output | License |
|---|---|---|---|---|---|---|---|
| **Qwen3-4B** | 4B | ~8 GB | ~3 GB | 80–120 | Excellent (100+ langs) | Strong with /no_think | Apache 2.0 |
| **Gemma 3 4B** | 4B | ~8 GB | ~2.6 GB | 80–130 | Good (140+ langs) | Good with function calling | Gemma license |
| Qwen3-1.7B | 1.7B | ~3.4 GB | ~1.5 GB | 150–200 | Good | Adequate | Apache 2.0 |
| Phi-4 14B | 14B | ~28 GB | ~9 GB | 30–50 | English-dominant ⚠️ | Strong | MIT |
| Mistral Small 3 24B | 24B | ~48 GB | ~15 GB | 30–50 | Good | Strong | Apache 2.0 |

**Why Qwen3-4B is the pick:**

Qwen3-4B performs comparably to Qwen2.5-7B across most benchmarks — effectively doubling the quality-per-parameter. Its explicit `/no_think` mode disables chain-of-thought reasoning for structured extraction tasks, dramatically reducing output tokens and latency. At Q4_K_M quantization (~3GB VRAM), it produces 80–120 tokens/sec on RTX 4090 — enough to process ~500 events/min with ~200 output tokens each if batched efficiently via vLLM or llama.cpp.

**Critical assessment: can a 4B model match Haiku for OSINT entity extraction?**

For the specific task of extracting entities, topics, sentiment, and relevance from OSINT event text: probably yes, with caveats. The extraction prompt is structured (you provide a JSON schema and the model fills it), which is where small models perform best. The main risks are:

- **Multilingual entity extraction:** Haiku handles Arabic/Russian/Chinese entity names with high reliability. Qwen3-4B has strong multilingual training but may produce more errors on transliterated names in low-resource contexts. Test thoroughly on representative samples from each language before switching.
- **Confidence calibration:** Haiku's entity confidence scores correlate well with actual correctness. A 4B model's confidence scores may be less well-calibrated, requiring the entity confidence gating threshold from the clustering pipeline to be re-tuned.
- **Rare event types:** For common event types (military, protest, disaster), quality will be comparable. For rare or novel event types, Haiku's broader training may produce better zero-shot extraction.

**Recommendation:** Start with Haiku at $26/month to establish quality baselines. Run Qwen3-4B in shadow mode (process the same events, compare outputs) for 2–4 weeks. Switch when extraction quality on your representative sample set reaches 95%+ agreement with Haiku.

### VRAM budget with all local models

| Component | VRAM | Notes |
|---|---|---|
| BGE-M3 via TEI (embeddings) | ~2.2 GB | FP16, always loaded |
| Qwen3-4B Q4_K_M (enrichment) | ~3 GB | Via vLLM or llama.cpp |
| Qwen3-Reranker-0.6B (rescoring) | ~1.2 GB | Via vLLM, loaded on-demand |
| **Total** | **~6.4 GB** | Leaves ~13.6 GB free for batching/KV cache |

All three models fit comfortably with 13+ GB headroom for batch processing and KV cache. The RTX 4090 is significantly underutilised at this workload.

---

## 5. Database architecture

### The two viable paths

**Path A: Qdrant (dedicated vector store) + PostgreSQL (relational)**

Qdrant provides filterable HNSW traversal — geo and time range filters evaluated during graph navigation, not post-processing. This is exactly what OSINT clustering needs: "find 10 nearest vectors within 50km of Odesa in last 6 hours" as a single atomic operation. Scalar quantization (int8) in RAM + original vectors on disk: 21.6M × 512-dim fits in ~12GB RAM, leaving ample room. Query latency: 20–40ms at 99% recall. Official `qdrant-client` Rust crate with gRPC via Tonic, async/await, type-safe builders. Single binary, trivial to deploy.

Add PostgreSQL (without TimescaleDB initially) for relational data: source configurations, user accounts, alert rules, situation metadata, audit logs. This keeps the operational database simple and well-understood.

**Path B: PostgreSQL + TimescaleDB + PostGIS + pgvectorscale (single-database)**

pgvectorscale changes the equation with DiskANN-inspired streaming index with Statistical Binary Quantization (SBQ). Stores graph on NVMe SSD with only ~3–5GB RAM overhead. Benchmarks at 50M vectors (768-dim): 471 QPS at 99% recall, ~31ms p50 latency. TimescaleDB hypertables partition into daily chunks, each with own DiskANN index. Recent chunks (~720K vectors, ~4.4GB) stay hot in RAM with sub-10ms latency, older chunks served from NVMe at 15–30ms. PostGIS GIST indexes give 1–15ms geospatial queries. h3-pg extension provides 99% faster H3 lookups than spatial joins via B-tree on pre-computed cells.

**Critical limitation:** Compressed TimescaleDB chunks lose vector index access entirely. Strategy: keep 2–3 days uncompressed for full vector+geo+temporal querying, compress older data. Estimated disk: 76–87GB for 30 days.

### Decision: start with Qdrant (Path A)

For a project this early, Qdrant offers simpler deployment, better tail latency for vector operations, and a Rust-native gRPC client. Add PostgreSQL later when relational query needs emerge. The single-database approach (Path B) is the long-term winner for operational maturity but adds complexity at the start.

### Analytical cold path: DataFusion over Parquet

Age events to daily Parquet files. DataFusion (pure Rust, now fastest single-node Parquet engine, outperforms DuckDB on ClickBench for partitioned data) handles aggregation over historical data for dashboards, trend analysis, and situation retrospectives. No C++ dependency, embeds directly into the Rust application.

### Hot path: in-memory HNSW

The clustering pipeline's hot path uses an in-memory HNSW via hnswlib-rs or USearch holding 24–48 hours of 512-dim int8 vectors (~4–9GB). Sub-millisecond nearest-neighbour search. New events are embedded by TEI, inserted into the in-memory index, and assigned to clusters in SituationGraph. This is separate from Qdrant — the in-memory index serves real-time clustering while Qdrant serves historical queries and dashboard lookups.

### PostgreSQL tuning for 64GB RAM

```
shared_buffers = 16GB
effective_cache_size = 48GB
maintenance_work_mem = 4GB
jit = off
random_page_cost = 1.1  # NVMe
```

---

## 6. Cloud vs local: what goes where

### The decision framework

The core question is: **does the cloud service provide capability we cannot replicate locally, or does it provide the same capability cheaper than our electricity + hardware amortisation?**

| Component | Local | Cloud | Recommendation | Rationale |
|---|---|---|---|---|
| **Embeddings (BGE-M3)** | TEI on RTX 4090 | OpenAI $65/mo, Cohere $324/mo | **Local** | Free, faster, better multilingual, no rate limits |
| **Enrichment (entity/topic extraction)** | Qwen3-4B on RTX 4090 | Haiku 3.5 $26/mo, Haiku 4.5 $33/mo | **Cloud initially, local later** | Start with Haiku for quality baseline, migrate to local once validated |
| **Reranking** | Qwen3-Reranker-0.6B on RTX 4090 | Cohere Rerank $2/1K searches | **Local** | At 720K events/day × 10 candidates each = 7.2M reranks/day = $14.4K/mo via Cohere. Absurdly expensive. |
| **Situation titles** | Qwen3-4B on RTX 4090 | Haiku 3.5 ~$0.50/mo | **Either** | Trivial volume (50–100/day), pennies either way |
| **Vector store (Qdrant)** | Self-hosted | Qdrant Cloud from €25/mo | **Local** | 21.6M vectors at 512-dim needs ~12GB RAM — trivial locally, expensive in cloud |
| **PostgreSQL** | Self-hosted on Hetzner | Managed from €20/mo | **Local on Hetzner** | Fits existing European hosting strategy |
| **Data sources (ACLED, FIRMS, GDELT, etc.)** | N/A | Various free APIs | **Cloud (free)** | These are external APIs by definition |
| **IODA / Cloudflare Radar** | N/A | APIs (pricing TBD) | **Cloud** | Critical gap — API documentation needed |
| **Shodan** | N/A | $69/mo (membership) | **Cloud** | No local alternative |

### Monthly cost projection

**Minimal viable deployment (all local except essential APIs):**

| Item | Monthly cost |
|---|---|
| Hetzner VPS (CPX31 or similar) | ~€15 |
| Domain (sitrep.watch) | ~€3 amortised |
| Shodan API membership | $69 (£55) |
| Claude Haiku enrichment (initial) | ~$26 (£21) |
| Electricity (RTX 4090, ~450W, 50% utilisation) | ~£30 |
| **Total** | **~£124/month** |

**After local enrichment migration (Qwen3-4B replaces Haiku):**

| Item | Monthly cost |
|---|---|
| Hetzner VPS | ~€15 |
| Domain | ~€3 |
| Shodan API membership | $69 (£55) |
| Electricity | ~£30 |
| **Total** | **~£98/month** |

**If everything went to cloud APIs instead:**

| Item | Monthly cost |
|---|---|
| Hetzner VPS (larger, for Qdrant/PG) | ~€40 |
| OpenAI embedding API (text-embedding-3-large) | $421 |
| Claude Haiku enrichment | $26 |
| Cohere Rerank (7.2M/day) | $14,400 ⚠️ |
| Qdrant Cloud (12GB RAM) | ~€200 |
| **Total** | **~$15,090/month** |

The reranking line item alone makes cloud-only deployment absurd. Local GPU inference is the only viable approach for this workload.

---

## 7. Deployment topology

```
┌─────────────────────────── Local Machine (64GB RAM, RTX 4090) ────────────────────────────┐
│                                                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────────┐     │
│  │   TEI        │  │ vLLM/llama   │  │   Qdrant     │  │  Rust Application          │     │
│  │  (BGE-M3)    │  │ (Qwen3-4B +  │  │  (vectors +  │  │  ┌─────────────────────┐   │     │
│  │  Port 8080   │  │  Reranker)   │  │   geo +      │  │  │ Event Ingestion     │   │     │
│  │  ~2.2GB VRAM │  │  Port 8081   │  │   temporal)  │  │  │ Clustering Pipeline │   │     │
│  │              │  │  ~4.2GB VRAM │  │  ~12GB RAM   │  │  │ SituationGraph      │   │     │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │  │ Dashboard API       │   │     │
│         │                 │                  │          │  └─────────┬───────────┘   │     │
│         └─────────────────┴──────────────────┴──────────┴───────────┘               │     │
│                                                                                      │     │
│  ┌──────────────┐  ┌──────────────┐                                                  │     │
│  │ PostgreSQL   │  │ In-memory    │  ┌────────────────────────────┐                   │     │
│  │ (relational) │  │ HNSW (hot    │  │ DataFusion (cold-path      │                   │     │
│  │              │  │  24-48hr)    │  │  analytics over Parquet)   │                   │     │
│  │  ~2GB RAM    │  │  ~4-9GB RAM  │  │  On-demand                 │                   │     │
│  └──────────────┘  └──────────────┘  └────────────────────────────┘                   │     │
│                                                                                       │     │
└───────────────────────────────────────────────────────────────────────────────────────┘     │
                                                                                              │
                         ┌──────────────────── Hetzner VPS ──────────────────┐                │
                         │  Public-facing web dashboard                      │                │
                         │  Reverse proxy (Caddy/nginx)                      │                │
                         │  Alert distribution (webhooks, email)             │                │
                         │  Optional: Qdrant replica for dashboard queries   │                │
                         └───────────────────────────────────────────────────┘                │
```

**RAM budget (64GB):**

| Component | RAM |
|---|---|
| OS + application | ~4 GB |
| PostgreSQL shared_buffers | 16 GB |
| Qdrant (12GB vectors + overhead) | ~14 GB |
| In-memory HNSW (24hr hot window) | ~4.5 GB |
| Event payloads + working memory | ~8 GB |
| OS page cache (effective_cache_size) | ~17.5 GB |
| **Total** | **~64 GB** |

Tight but workable. If RAM pressure becomes an issue, the first thing to shed is the in-memory HNSW — fall back to Qdrant-only for all vector queries at the cost of 20–40ms latency instead of sub-1ms.

---

## 8. Implementation priority order

1. **TEI sidecar with BGE-M3** (~2 hours) — Docker compose, verify embeddings flowing, measure throughput
2. **Qdrant deployment** (~2 hours) — Docker, create collection with 512-dim int8 quantization, geo and timestamp payload indexes
3. **Enrichment via Haiku API** (~3 hours) — Entity/topic/sentiment extraction with structured JSON schema, establish quality baselines
4. **Clustering pipeline hot path** (~1-2 weeks) — In-memory HNSW, three-phase architecture from Report 1, streaming IDF, temporal decay
5. **Reranker integration** (~4 hours) — Qwen3-Reranker-0.6B via vLLM, integrate into Phase 2 scoring
6. **Shadow-mode Qwen3-4B enrichment** (~1 day) — Run alongside Haiku, compare outputs, tune extraction prompt
7. **DataFusion cold path** (~2 days) — Daily Parquet export, aggregation queries for dashboard
8. **Dashboard + Hetzner deployment** (~1 week) — Public-facing web interface, alert system

---

## 9. Key risks and mitigations

**Risk: Qwen3-4B multilingual extraction quality insufficient for Arabic/Farsi.** Mitigation: keep Haiku as fallback for events in languages where local model underperforms. Route by detected language — English/Russian/Chinese to local model, Arabic/Farsi/Hebrew to Haiku. Cost impact: <$5/month for the minority-language events.

**Risk: 64GB RAM insufficient after all components loaded.** Mitigation: Qdrant supports mmap-backed storage where only the quantised vectors stay in RAM and full vectors are served from NVMe. This trades ~20ms latency for ~6GB RAM savings. Alternatively, reduce hot window from 48hr to 24hr.

**Risk: BGE-M3 Matryoshka truncation to 512-dim loses critical clustering quality.** Mitigation: benchmark clustering F1 at 1024-dim vs 512-dim on a held-out labelled set before committing. If degradation exceeds 3%, consider Qwen3-Embedding-0.6B which has native Matryoshka support designed for arbitrary dimension selection.

**Risk: Qdrant as sole vector store creates single point of failure.** Mitigation: Qdrant supports snapshot-based backup to S3-compatible storage (Hetzner Object Storage). Daily snapshots with 24hr RPO is acceptable for a monitoring dashboard. The in-memory HNSW serves as hot standby for the most recent data.
