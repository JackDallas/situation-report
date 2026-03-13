# Situation Report - Data Growth Projection

**Date:** 2026-03-04
**Database:** TimescaleDB + PostGIS on <YOUR_HOST>
**Current DB Size:** 3,019 MB (~2.95 GB)

---

## 1. Current State Snapshot

### Database Total Size

| Metric            | Value    |
|-------------------|----------|
| Total DB size     | 3,019 MB |
| Events hypertable | 2,857 MB |
| Position history  | 43 MB    |
| Indexes (all)     | 1,326 MB |
| latest_positions  | 57 MB    |
| Continuous aggs   | ~4 MB    |
| Other tables      | ~58 MB   |

### Row Counts (Top Tables)

| Table                    | Rows      |
|--------------------------|-----------|
| events (total)           | 2,167,446 |
| position_history         | 244,887   |
| latest_positions         | 24,364    |
| situation_search_history | 5,649     |
| entities                 | 1,935     |
| entity_relationships     | 2,460     |
| event_entities           | 551       |
| situations               | 7         |

### Events by Source Type (All Time)

| Source        | Count     | Avg Row Size | Daily Rate (est) |
|---------------|-----------|--------------|------------------|
| ais           | 1,162,495 | 627 bytes    | ~13.9M/day*      |
| airplaneslive | 259,424   | 714 bytes    | ~146K/day        |
| adsb-fi       | 258,715   | 697 bytes    | ~145K/day        |
| adsb-lol      | 234,764   | 700 bytes    | ~135K/day        |
| bgp           | 163,458   | 559 bytes    | ~91K/day         |
| opensky       | 41,151    | 510 bytes    | ~21K/day         |
| firms         | 26,884    | 3,087 bytes  | ~11K/day         |
| shodan        | 11,485    | 1,942 bytes  | ~5.7K/day        |
| rss-news      | 7,685     | 5,657 bytes  | ~4.4K/day        |
| telegram      | 1,663     | 3,263 bytes  | ~676/day         |
| gdelt         | 768       | 6,141 bytes  | legacy, inactive |
| ooni          | 612       | 3,530 bytes  | ~292/day         |
| geoconfirmed  | 518       | 4,647 bytes  | ~1/day           |
| notam         | 353       | 4,553 bytes  | ~245/day         |
| cloudflare    | 108       | 4,471 bytes  | ~68/day          |
| otx           | 66        | 4,861 bytes  | ~40/day          |
| usgs          | 1         | 703 bytes    | ~1/day           |

*AIS started at 21:37 UTC on March 4 and has been running for ~2 hours. The 13.9M/day rate is extrapolated from ~580K/hour peak sustained rate. See AIS analysis below.

---

## 2. Hourly Ingestion Rates (Last 24 Hours)

### Non-AIS Sources (Stable Baseline)

Based on March 3 (full day, no AIS): **443,931 events/day = ~18,497/hour average**

Hourly range on March 4 (non-AIS only):
- **Low (off-peak):** ~10,000/hour (UTC 03:00-06:00)
- **Average:** ~22,000/hour
- **Peak:** ~38,000/hour (UTC 17:00-18:00)

### AIS Source (NEW - Started 2026-03-04 ~21:37 UTC)

| Hour (UTC)  | AIS Events |
|-------------|------------|
| 21:00-22:00 | 240,141    |
| 22:00-23:00 | 562,365    |
| 23:00-00:00 | 360,240*   |

*Partial hour at time of query.

AIS average sustained rate: **~387,000/hour** (based on full hours observed).

### Combined Rate With AIS

| Scenario       | Events/Hour | Events/Day  |
|----------------|-------------|-------------|
| Non-AIS only   | ~18,500     | ~444,000    |
| AIS only       | ~387,000    | ~9,288,000  |
| **Combined**   | **~405,500**| **~9,732,000** |

AIS represents **95.4%** of total event volume.

---

## 3. Position History

| Metric                  | Value    |
|-------------------------|----------|
| Current rows            | 244,887  |
| Retention policy        | 24 hours |
| Compression policy      | 2 hours  |
| Size (current)          | 43 MB    |
| Sources tracked         | 5 (adsb-fi, adsb-lol, ais, airplaneslive, opensky) |
| Latest positions        | 24,364 entities |
| Rate (observed hour)    | ~240,000/hour |

Position history is self-limiting due to the 24-hour retention policy. Maximum steady-state size: **~500 MB** (24h x 240K/hr x ~90 bytes/row average compressed).

---

## 4. Compression Analysis

### Current Compression State (Events Hypertable)

| State        | Chunks | Total Size |
|--------------|--------|------------|
| Uncompressed | 36     | 2,797 MB   |
| Compressed   | 111    | 66 MB      |
| **Total**    | **147**| **2,863 MB** |

**Compression ratio: ~42:1** on older chunks (66 MB compressed from estimated ~2.8 GB uncompressed historical data).

### Compression Policy
- Events: compress after **7 days**
- Position history: compress after **2 hours**

### Today's Chunk Breakdown (_hyper_1_1118_chunk, March 4)

| Component  | Size     |
|------------|----------|
| Table data | 1,186 MB |
| Indexes    | 950 MB   |
| **Total**  | **2,135 MB** |

This single chunk contains ~1.73M rows (March 4) and is **74.5%** of the entire events hypertable.

### Retention Policies

| Hypertable       | Retention | Schedule |
|------------------|-----------|----------|
| events           | 180 days  | Daily    |
| events_hourly    | 2 years   | Daily    |
| events_daily     | 5 years   | Daily    |
| position_history | 24 hours  | Daily    |

---

## 5. Growth Projections

### Scenario A: Without AIS (Pre-March 4 Baseline)

Based on March 3 data: 443,931 events/day, ~642 MB/day uncompressed (based on observed chunk size).

However, March 3 was the first day with all non-AIS sources fully active. Prior days had far fewer events (the DB has only ~1,005K non-AIS events accumulated over ~1 year). The realistic daily data generation rate for non-AIS sources is **~640 MB/day uncompressed** before compression kicks in at 7 days.

| Timeframe | Raw Data Added | After Compression (7d+) | Cumulative DB Size |
|-----------|----------------|-------------------------|--------------------|
| 1 day     | ~640 MB        | n/a (still uncompressed)| ~3.7 GB           |
| 1 week    | ~4.5 GB        | ~4.5 GB (none compressed yet) | ~7.5 GB      |
| 1 month   | ~19.2 GB       | ~5.1 GB*               | ~8.1 GB           |
| 1 year    | ~234 GB        | ~11.5 GB**             | ~14.5 GB          |

*30 days: 7 days uncompressed (4.5 GB) + 23 days compressed at ~42:1 (~0.6 GB)
**365 days: 7 days uncompressed (4.5 GB) + 173 days compressed (rolling 180-day retention) at ~42:1 (~7 GB)

### Scenario B: With AIS (Current Configuration)

AIS adds ~9.3M events/day at 627 bytes average. Combined with non-AIS sources:

**Daily raw data estimate:**
- AIS: 9.3M rows x 627 bytes = ~5.5 GB/day (table data)
- Non-AIS: 444K rows x ~750 bytes avg = ~0.3 GB/day (table data)
- Indexes: ~80% of table data = ~4.6 GB/day
- **Total daily uncompressed: ~10.4 GB/day**

This matches observed behavior: today's single chunk is 2.1 GB for ~1.73M rows accumulated in ~24 hours, which extrapolates to ~2.8 GB/day at the full 24-hour AIS rate. But AIS only ran for ~2 hours today. A full day with AIS would yield far more data.

**Corrected full-day estimate with AIS running 24h:**
- Today's chunk at 2.1 GB covers: ~444K non-AIS (24h) + ~1.16M AIS (2h) = ~1.6M rows
- A full day: ~444K non-AIS + ~9.3M AIS = ~9.7M rows
- Scaling: 9.7M / 1.6M x 2.1 GB = **~12.7 GB/day uncompressed (data + indexes)**

| Timeframe | Raw Data Added | After Compression | Cumulative DB Size |
|-----------|----------------|-------------------|--------------------|
| 1 day     | ~12.7 GB       | n/a               | ~15.7 GB          |
| 1 week    | ~89 GB         | ~89 GB (none compressed yet) | ~92 GB   |
| 1 month   | ~381 GB        | ~100 GB*          | ~103 GB           |
| 6 months  | ~2.3 TB        | ~180 GB**         | ~183 GB           |
| 1 year    | ~4.6 TB        | ~180 GB***        | ~183 GB           |

*30 days: 7 days uncompressed (89 GB) + 23 days compressed at ~42:1 (~11 GB)
**180 days: 7 days uncompressed (89 GB) + 173 days compressed at ~42:1 (~91 GB)
***365 days: Same as 180 days due to retention policy dropping chunks older than 180 days

### Steady-State Size (With Current Retention)

The 180-day retention policy means the DB reaches a **steady state** at ~6 months:

| Component                        | Steady-State Size |
|----------------------------------|-------------------|
| 7 days uncompressed events       | ~89 GB            |
| 173 days compressed events       | ~91 GB            |
| Position history (24h retention) | ~0.5 GB           |
| Indexes (uncompressed week)      | ~44 GB            |
| Continuous aggregates            | ~0.5 GB           |
| Other tables                     | ~0.2 GB           |
| **Total Steady State**           | **~180-225 GB**   |

---

## 6. 1TB NVMe Fill Estimate

| Scenario                              | Time to 1 TB  |
|---------------------------------------|---------------|
| With AIS, no retention changes        | **Never** (steady state ~225 GB) |
| With AIS, if retention increased to 1y| ~12-14 months |
| With AIS, if compression disabled     | ~79 days       |
| With AIS, if retention disabled       | ~79 days       |
| Without AIS, no retention changes     | **Never** (steady state ~15 GB)  |

**The current retention + compression policies keep the database well within 1 TB.** The 180-day event retention is the critical safety valve. Without it, data would exceed 1 TB in about 2.5 months at the current AIS-inclusive rate.

---

## 7. Top Space Consumers (Index Analysis)

| Index                          | Size   | Purpose              |
|--------------------------------|--------|----------------------|
| idx_events_payload (today)     | 397 MB | JSONB payload GIN    |
| idx_events_location (today)    | 121 MB | PostGIS spatial      |
| idx_events_source_time (today) | 97 MB  | Source+time lookup   |
| idx_events_region_time (today) | 85 MB  | Region+time lookup   |
| idx_events_entity_time (today) | 83 MB  | Entity+time lookup   |
| events_event_time_idx (today)  | 66 MB  | Hypertable partition  |
| idx_events_dedup (today)       | 41 MB  | Deduplication        |
| idx_latest_pos_source          | 34 MB  | Position lookups     |
| idx_events_embedding (today)   | 27 MB  | pgvector HNSW        |
| idx_events_tsv (today)         | 14 MB  | Full-text search     |

**Indexes are 44.5% of the current DB size** (950 MB indexes on today's 1,186 MB data chunk). The JSONB payload GIN index alone is 397 MB -- larger than any other single index.

---

## 8. Recommendations

### Immediate (This Week)

1. **Reduce compression delay from 7 days to 2 days for events.** The current 7-day uncompressed window means up to 89 GB of live data with AIS. Reducing to 2 days cuts this to ~25 GB.
   ```sql
   SELECT alter_job(1003, config => '{"hypertable_id": 1, "compress_after": "2 days"}');
   ```

2. **Monitor AIS volume.** AIS is brand new (started 2 hours ago) and dominates 95% of event volume. Verify this rate is intentional. If AIS is storing raw vessel positions as events, consider whether `latest_positions` + `position_history` already covers this use case, and AIS events could be filtered to only store significant state changes (port arrival, zone entry, speed anomalies).

3. **Evaluate the payload GIN index.** At 397 MB for one day's chunk and growing linearly with AIS volume, this is the single most expensive index. If JSONB payload queries are rare for AIS events, consider a partial index excluding high-volume source types:
   ```sql
   CREATE INDEX idx_events_payload_filtered ON events USING gin(payload)
   WHERE source_type NOT IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'bgp');
   ```

### Short-Term (This Month)

4. **Add AIS-specific retention.** AIS raw events are low-value after correlation. Consider a separate 7-day retention for AIS while keeping 180 days for intel sources. This requires a custom retention job since TimescaleDB retention is per-hypertable:
   ```sql
   -- Custom job to delete old AIS events
   CREATE OR REPLACE FUNCTION delete_old_ais_events(job_id INT, config JSONB)
   RETURNS VOID AS $$
     DELETE FROM events WHERE source_type = 'ais' AND event_time < NOW() - INTERVAL '7 days';
   $$ LANGUAGE SQL;
   ```

5. **Consider partitioning by source volume tier.** The extreme skew (AIS = 95% of rows) means one source dominates chunk sizes, compression ratios, and index bloat. A separate hypertable for high-volume tracking data (AIS, ADSB) vs. intel events would allow different storage strategies.

### Long-Term (This Quarter)

6. **Implement tiered storage.** Move compressed chunks older than 30 days to cheaper storage (e.g., S3 via TimescaleDB tiered storage or pg_tier extension).

7. **Tune continuous aggregates for AIS.** Pre-aggregate AIS data into hourly vessel-count-per-region summaries. Drop raw AIS events after aggregation (reduce retention to 24-48h).

8. **Consider downsampling tracking sources.** Aviation sources (airplaneslive + adsb-fi + adsb-lol) produce 3x redundant data for the same aircraft. Deduplication at ingest time could reduce ~430K/day to ~150K/day.

---

## 9. Summary

| Metric                     | Value                |
|----------------------------|----------------------|
| Current DB size            | 3.0 GB               |
| Daily growth (with AIS)    | ~12.7 GB/day uncompressed |
| Daily growth (without AIS) | ~0.64 GB/day uncompressed |
| Compression ratio          | ~42:1                |
| Steady-state (180d retain) | ~180-225 GB with AIS |
| 1 TB NVMe headroom         | 4-5x above steady state |
| #1 risk                    | AIS event volume (95% of all data) |
| #1 recommendation          | Shorten compression delay to 2 days |

The database is currently healthy at 3 GB but growing rapidly since AIS was enabled. With the existing 180-day retention and compression policies, the steady state will be approximately **180-225 GB** -- well within the 1 TB NVMe capacity. The primary concern is the **7-day uncompressed window**, which will hold ~89 GB of live data at full AIS rate. Reducing this to 2 days is the highest-impact change.
