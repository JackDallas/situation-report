# Situation Report -- Storage & Compression Optimization

**Date:** 2026-03-04
**Database:** TimescaleDB (pg17) + PostGIS on <YOUR_HOST>
**Current DB Size:** 3.0 GB | **Projected Steady-State:** 180-225 GB
**Volume Driver:** AIS vessel tracking (95% of events, ~9.3M/day)

---

## Executive Summary

The Situation Report database is healthy at 3 GB but growing at ~12.7 GB/day uncompressed with AIS enabled. Current policies (7-day compression delay, 180-day retention) produce a steady-state of ~180-225 GB on a 1 TB NVMe -- safe but with thin margins after accounting for WAL, temp files, and OS overhead.

This report evaluates eight optimization strategies that, combined, can:

- **Reduce the uncompressed hot window from ~89 GB to ~13 GB** (compression delay reduction)
- **Cut steady-state by 50-60%** (AIS-specific retention + partial indexes)
- **Enable 3+ years of intel data retention** (tiered storage on the 4TB USB drive)
- **Provide disaster recovery** (automated backups to the external drive)

Recommended implementation order is provided at the end, prioritized by impact-to-effort ratio.

---

## Table of Contents

1. [Compression Delay Optimization](#1-compression-delay-optimization)
2. [AIS-Specific Retention](#2-ais-specific-retention)
3. [Partial GIN Index](#3-partial-gin-index)
4. [4TB USB-C Drive Integration](#4-4tb-usb-c-drive-integration)
5. [TimescaleDB Tiered Storage](#5-timescaledb-tiered-storage)
6. [Continuous Aggregate Optimization](#6-continuous-aggregate-optimization)
7. [Source Deduplication](#7-source-deduplication)
8. [Backup Strategy](#8-backup-strategy)
9. [Recommended Implementation Order](#9-recommended-implementation-order)

---

## 1. Compression Delay Optimization

### Current State

- Compression policy: **7 days** (set in migration `002_timescaledb_postgis.sql`)
- Chunk interval: **1 day**
- Compression ratio: **~42:1** on existing compressed chunks
- Uncompressed window at full AIS rate: **~89 GB** (7 days x 12.7 GB/day)

### Recommendation: Reduce to 2 Days

Changing the compression delay from 7 days to 2 days shrinks the uncompressed hot window from ~89 GB to ~25 GB -- a **~64 GB reduction** in peak uncompressed data.

### Can We Go to 1 Day?

Yes, with caveats. TimescaleDB chunks are 1-day intervals, and compression operates at the chunk level. A 1-day delay means yesterday's chunk gets compressed today. This works when:

- Data arrives in temporal order (true for AIS -- events use `Utc::now()`)
- You do not need to UPDATE or backfill data older than 24 hours
- Your enrichment pipeline finishes within 24 hours of ingestion

**Risk with 1 day:** The enrichment pipeline (`enrich.rs`) attaches `payload.enrichment` to events via UPDATE. If enrichment is delayed or backed up beyond 24 hours, UPDATEs hit compressed chunks, which in TimescaleDB requires decompressing the affected segment, performing the update, and leaving an uncompressed "overflow" portion. This is functional (TimescaleDB supports DML on compressed chunks since 2.11+) but introduces:

- **Write amplification:** Each UPDATE decompresses a segment (~1000 rows), modifies one row, and the segment remains partially uncompressed until recompression
- **Recompression overhead:** The compression policy re-compresses these modified chunks, but it adds CPU work
- **Query planning:** Chunks with mixed compressed/uncompressed segments can be slower to query

**Verdict:** 2 days is the sweet spot. It gives the enrichment pipeline ample time to complete (it typically finishes within minutes), provides a buffer for container restarts, and still achieves most of the space savings. Going to 1 day saves only ~12.7 GB more but introduces fragility.

### Per-Source-Type Compression Delays

TimescaleDB's `add_compression_policy` applies to the entire hypertable -- you cannot set different compression delays per `source_type` within a single hypertable. To achieve different compression speeds:

- **Option A (Recommended):** Use 2 days globally. This works for all sources because the enrichment pipeline only touches news/intel events (not AIS/ADSB), and those events are a tiny fraction.
- **Option B (Advanced):** Separate hypertables for tracking vs. intel events. This would require application-level changes to route inserts and is not recommended at this stage.

### SQL Commands

```sql
-- Step 1: Find the current compression job ID
SELECT j.job_id, j.schedule_interval, config
FROM timescaledb_information.jobs j
WHERE j.proc_name = 'policy_compression'
  AND j.hypertable_name = 'events';

-- Step 2: Update the compression delay (replace JOB_ID with actual ID)
-- Method A: alter_job directly
SELECT alter_job(<JOB_ID>,
    config => jsonb_build_object(
        'hypertable_id', (config->>'hypertable_id')::int,
        'compress_after', '2 days'
    )
) FROM timescaledb_information.jobs
WHERE job_id = <JOB_ID>;

-- Method B: Remove and re-add (cleaner)
SELECT remove_compression_policy('events');
SELECT add_compression_policy('events', INTERVAL '2 days');

-- Step 3: Manually compress existing chunks older than 2 days that are still uncompressed
SELECT compress_chunk(c, if_not_compressed => true)
FROM show_chunks('events', older_than => INTERVAL '2 days') c;

-- Step 4: Verify
SELECT chunk_name,
       range_start,
       range_end,
       is_compressed,
       before_compression_total_bytes,
       after_compression_total_bytes
FROM timescaledb_information.chunks
WHERE hypertable_name = 'events'
ORDER BY range_start DESC;
```

### Tradeoffs

| Factor | 7-day delay | 2-day delay | 1-day delay |
|--------|-------------|-------------|-------------|
| Uncompressed window (with AIS) | ~89 GB | ~25 GB | ~13 GB |
| Enrichment UPDATE safety | Comfortable | Comfortable | Risky for delayed enrichment |
| Query speed on recent data | Fast (row-based) | Fast (2 days row-based) | Minimal row-based window |
| Compression CPU overhead | Low (weekly batch) | Low (daily batch) | Low (daily batch) |
| Backfill/correction window | 7 days | 2 days | 1 day |

### Estimated Space Savings

**Immediate: ~64 GB reduction** in peak uncompressed data at steady state.

---

## 2. AIS-Specific Retention

### Problem

AIS raw events are 95% of volume (~9.3M rows/day, 627 bytes avg) but have low intelligence value after the correlation window (6 hours) and position tracking (24 hours). The current 180-day retention keeps all sources equally, meaning 95% of stored data is low-value vessel positions.

### Recommendation: 7-Day Retention for Tracking Sources

Keep raw AIS, ADSB, and OpenSky events for only 7 days. Intel-relevant sources (news, conflict, cyber, etc.) keep the full 180-day retention. The continuous aggregates (`events_hourly`, `events_daily`) already capture counts and unique entities, so historical trend analysis is preserved.

### Why Not Chunk-Level Retention?

TimescaleDB's built-in `add_retention_policy` drops entire chunks by time range -- it cannot selectively retain rows within a chunk based on `source_type`. Since all source types share the same hypertable with 1-day chunks, the built-in policy is all-or-nothing.

A custom job is required for row-level selective retention.

### SQL: Custom Retention Job

```sql
-- Create the selective retention function
CREATE OR REPLACE FUNCTION selective_retention(job_id INT, config JSONB)
RETURNS VOID AS $$
DECLARE
    tracking_retention INTERVAL;
    rows_deleted BIGINT;
BEGIN
    tracking_retention := (config->>'tracking_retention')::interval;

    -- Delete old tracking events from UNCOMPRESSED chunks only.
    -- Compressed chunks cannot have individual rows deleted efficiently;
    -- they rely on the global 180-day drop_chunks policy.
    --
    -- For uncompressed chunks (last 2 days with the new compression delay),
    -- this is a no-op since they are newer than 7 days.
    --
    -- For the gap between 2 days and 7 days, chunks are compressed.
    -- We must decompress, delete, and recompress -- which is expensive.
    -- Instead, we use a smarter approach: delete from chunks that are
    -- between tracking_retention and the global retention.

    -- Delete tracking-source rows older than the tracking retention period.
    -- TimescaleDB supports DELETE on compressed chunks (decompresses affected segments).
    DELETE FROM events
    WHERE source_type IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky')
      AND event_time < NOW() - tracking_retention;

    GET DIAGNOSTICS rows_deleted = ROW_COUNT;
    RAISE NOTICE 'selective_retention: deleted % tracking rows older than %',
        rows_deleted, tracking_retention;
END;
$$ LANGUAGE plpgsql;

-- Register the job to run daily at 03:00 UTC
SELECT add_job(
    'selective_retention',
    '1 day',
    config => '{"tracking_retention": "7 days"}'::jsonb,
    initial_start => '2026-03-05 03:00:00+00'::timestamptz
);
```

### Important: DELETE on Compressed Chunks

Deleting rows from compressed chunks in TimescaleDB works but has overhead:

1. The affected compressed segments are marked for decompression
2. On next recompression pass, they get cleaned up
3. Space is not immediately reclaimed -- it is reclaimed on recompression

For maximum efficiency, run `recompress_chunk()` after the deletion job, or let the compression policy handle it naturally on its next run.

### Alternative: Separate Hypertable for Tracking Data

A more architecturally clean approach is to route AIS/ADSB events to a separate hypertable with its own compression and retention policies. This avoids the expensive DELETE-on-compressed-chunks problem entirely:

```sql
-- Create a dedicated tracking hypertable
CREATE TABLE tracking_events (LIKE events INCLUDING ALL);
SELECT create_hypertable('tracking_events', 'event_time',
    chunk_time_interval => INTERVAL '1 day');

-- Set aggressive policies
ALTER TABLE tracking_events SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'source_type, region_code',
    timescaledb.compress_orderby = 'event_time DESC'
);
SELECT add_compression_policy('tracking_events', INTERVAL '2 hours');
SELECT add_retention_policy('tracking_events', INTERVAL '7 days');
```

**Tradeoff:** This requires application code changes to route inserts (`persist_event` in `registry.rs` would need to check `source_type` and pick the right table). Queries that span both tables need UNION. This is a larger refactor best done when there is a clear need.

### Tradeoffs

| Approach | Space Savings | Code Changes | Complexity |
|----------|---------------|--------------|------------|
| Custom DELETE job | High | None (SQL only) | Medium (DELETE on compressed chunks) |
| Separate hypertable | High | Significant (Rust + SQL) | High initially, cleaner long-term |
| Status quo (180d for all) | None | None | Low |

### Estimated Space Savings

At steady state with 180-day global retention:
- AIS compressed data (173 days): ~85 GB
- With 7-day AIS retention: ~2 GB (7 days of AIS, compressed after 2 days)
- **Savings: ~83 GB**, reducing steady state from ~180-225 GB to ~95-140 GB

---

## 3. Partial GIN Index

### Problem

The `idx_events_payload` GIN index on the JSONB `payload` column is the single most expensive index, consuming **397 MB per day** on today's chunk alone. AIS events dominate the index but their payloads are rarely queried via GIN operations (the `?`, `@>`, `?|`, `?&` operators).

AIS payloads contain structured numeric data (mmsi, speed, heading, course) that is never queried through GIN -- the pipeline accesses these fields directly from the JSONB in application code after fetching by entity_id or event_time.

### Recommendation: Filtered GIN Index Excluding High-Volume Sources

```sql
-- Step 1: Create the filtered index (CONCURRENTLY to avoid locking)
CREATE INDEX CONCURRENTLY idx_events_payload_filtered
ON events USING GIN (payload)
WHERE source_type NOT IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky', 'bgp');

-- Step 2: Verify the new index works for your queries
-- Run EXPLAIN ANALYZE on typical payload queries to confirm the planner uses it:
EXPLAIN ANALYZE
SELECT * FROM events
WHERE payload ? 'enrichment'
  AND source_type = 'rss-news'
  AND event_time > NOW() - INTERVAL '1 day';

-- Step 3: Drop the old full index (only after verifying step 2)
DROP INDEX CONCURRENTLY idx_events_payload;
```

### What Queries Would Break?

The filtered index is ONLY used when the query's WHERE clause matches (or is a subset of) the index predicate. These queries would NOT use the new partial index:

1. **Queries filtering payload on AIS events:**
   ```sql
   -- This query would fall back to sequential scan on the chunk:
   SELECT * FROM events
   WHERE source_type = 'ais'
     AND payload @> '{"military": true}';
   ```
   **Impact:** Low. The application does not query AIS payloads via GIN. Military AIS filtering is done at ingest time (`is_military_mmsi()` in `ais.rs`), not via DB queries.

2. **Queries on payload without a source_type filter:**
   ```sql
   -- No source_type in WHERE = planner cannot prove it matches the partial index:
   SELECT * FROM events WHERE payload @> '{"enrichment": {"relevance": "high"}}';
   ```
   **Impact:** Medium. The `event_has_enrichment()` query in `queries.rs` uses `payload ? 'enrichment'` but also filters by `source_type` and `source_id`, so it would still use the partial index. However, any ad-hoc queries without source_type would degrade.

3. **Queries on BGP/OpenSky payload fields:**
   ```sql
   SELECT * FROM events
   WHERE source_type = 'bgp' AND payload @> '{"type": "leak"}';
   ```
   **Impact:** Low. BGP events are few (~91K/day) and queries on them use the `idx_events_source_time` index first, then filter payload in the application.

### Tradeoffs

| Factor | Full GIN index | Partial GIN index |
|--------|---------------|-------------------|
| Index size per day | ~397 MB | ~15-20 MB (estimated) |
| Covers all source_types | Yes | No (excluded: ais, adsb-*, opensky, bgp) |
| Requires source_type in WHERE | No | Yes (for planner to use it) |
| Maintenance cost (INSERT) | High (every row) | Low (only ~5% of rows) |

### Estimated Space Savings

Per day: ~397 MB -> ~20 MB = **~377 MB/day saved**.
Over the 2-day uncompressed window: **~754 MB saved**.
Over the 7-day uncompressed window (if still on 7-day compression): **~2.6 GB saved**.

Note: Compressed chunks do not maintain B-tree or GIN indexes (TimescaleDB drops them on compression), so the savings apply only to the uncompressed window. Still significant because the uncompressed window is the most space-constrained period.

---

## 4. 4TB USB-C Drive Integration

### Use Cases

1. **Tiered storage:** Move compressed (cold) chunks to the USB drive
2. **Backups:** Store pg_dump / base backups on the USB drive
3. **WAL archiving:** Archive WAL files for point-in-time recovery

### Hardware Considerations

#### USB-C Performance

| Interface | Max Throughput | Typical SSD Throughput | Random 4K IOPS |
|-----------|---------------|----------------------|-----------------|
| NVMe (PCIe 4.0) | 7,000 MB/s | 3,000-5,000 MB/s | 500K-1M |
| USB 3.2 Gen 2 (10 Gbps) | 1,250 MB/s | 900-1,000 MB/s | 30K-80K |
| USB 3.2 Gen 1 (5 Gbps) | 625 MB/s | 400-500 MB/s | 15K-40K |
| USB 3.0 (5 Gbps) | 625 MB/s | 300-400 MB/s | 10K-30K |

**For cold/compressed data (sequential reads):** USB-C is perfectly adequate. Compressed chunks are read sequentially during queries and the bottleneck is decompression CPU, not I/O.

**For random I/O (hot data, indexes):** USB-C is 10-30x slower than NVMe in random IOPS. Never put active/uncompressed data on USB.

#### UASP (USB Attached SCSI Protocol)

UASP is critical for USB storage performance. Without UASP, USB drives use BOT (Bulk-Only Transport) which is significantly slower due to lack of command queuing.

**Check UASP support:**
```bash
# On the server, after plugging in the drive:
lsusb -t
# Look for "Driver=uas" (UASP) vs "Driver=usb-storage" (BOT)

# Or check dmesg:
dmesg | grep -i "uas\|usb-storage"
```

Most modern USB-C SSDs and enclosures support UASP. If the drive falls back to BOT, check:
- Kernel version (>= 4.0 for reliable UAS)
- USB controller compatibility
- Try `echo 0 > /sys/module/usb_storage/parameters/quirks` if UAS is being quirked off

#### Recommended Filesystem

**ext4** -- battle-tested, low overhead, excellent for PostgreSQL:
```bash
# Format the drive (replace /dev/sdX with actual device)
sudo mkfs.ext4 -L sitrep-cold -O dir_index,extent,filetype,sparse_super2 /dev/sdX1

# Mount options optimized for database workload:
# - noatime: skip access time updates (reduces writes)
# - data=ordered: journaling mode balancing safety and performance
# - discard: enable TRIM for SSD longevity
sudo mount -o noatime,data=ordered,discard /dev/sdX1 /mnt/usb-storage
```

**XFS** is also acceptable and handles large files well, but ext4 is the PostgreSQL community's default recommendation.

### Setup: Mount the Drive

```bash
# 1. Identify the drive
lsblk
sudo fdisk -l /dev/sdX

# 2. Partition (if not already)
sudo parted /dev/sdX mklabel gpt
sudo parted /dev/sdX mkpart primary ext4 0% 100%

# 3. Format
sudo mkfs.ext4 -L sitrep-cold /dev/sdX1

# 4. Create mount point
sudo mkdir -p /mnt/usb-storage

# 5. Get UUID for fstab (survives device re-enumeration)
sudo blkid /dev/sdX1
# Output: /dev/sdX1: UUID="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" TYPE="ext4"

# 6. Add to fstab for persistent mount
echo 'UUID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx /mnt/usb-storage ext4 noatime,data=ordered,discard,nofail 0 2' | sudo tee -a /etc/fstab

# The "nofail" option is CRITICAL for USB drives -- it prevents boot failure
# if the drive is unplugged or fails.

# 7. Mount
sudo mount /mnt/usb-storage

# 8. Set ownership for PostgreSQL container
sudo mkdir -p /mnt/usb-storage/pg-cold
sudo mkdir -p /mnt/usb-storage/pg-backups
sudo mkdir -p /mnt/usb-storage/pg-wal-archive
sudo chown -R 1000:1000 /mnt/usb-storage/pg-cold
sudo chown -R 1000:1000 /mnt/usb-storage/pg-backups
sudo chown -R 1000:1000 /mnt/usb-storage/pg-wal-archive
```

Note on ownership: The `timescale/timescaledb-ha` image runs PostgreSQL as UID 1000 (the `postgres` user inside the container). Verify with:
```bash
docker exec situationreport-postgres-1 id
# Should show uid=1000(postgres)
```

### Docker Compose Changes

Add volume mounts for the USB drive:

```yaml
services:
  postgres:
    image: timescale/timescaledb-ha:pg17
    volumes:
      - ${PG_DATA_DIR:-pgdata}:/home/postgres/pgdata/data
      - /mnt/usb-storage/pg-cold:/mnt/cold          # Tiered storage
      - /mnt/usb-storage/pg-backups:/mnt/backups     # Backup destination
      - /mnt/usb-storage/pg-wal-archive:/mnt/wal     # WAL archive
    # ... rest of config unchanged
```

### Performance Implications

| Workload | NVMe | USB-C SSD | Acceptable? |
|----------|------|-----------|-------------|
| Active writes (INSERT) | 3000+ MB/s | N/A (keep on NVMe) | -- |
| Uncompressed chunk reads | 3000+ MB/s | N/A (keep on NVMe) | -- |
| Compressed chunk reads | CPU-bound (decompression) | 500-1000 MB/s | Yes |
| Sequential backup writes | 3000+ MB/s | 500-1000 MB/s | Yes |
| WAL archiving | Small sequential writes | 500-1000 MB/s | Yes |
| Index random reads | 500K+ IOPS | 30-80K IOPS | No (keep indexes on NVMe) |

**Rule:** Hot data (uncompressed chunks, active indexes) stays on NVMe. Cold data (compressed chunks > N days) moves to USB. Backups and WAL archives go to USB.

---

## 5. TimescaleDB Tiered Storage

### Does TimescaleDB Community Support Tablespace Moves?

**Yes.** The `move_chunk()` function is available in TimescaleDB Community Edition (not just Enterprise/Cloud). It allows moving individual chunks -- and their indexes -- between PostgreSQL tablespaces. This is the foundation for tiered storage on self-hosted installations.

### Setup: Tablespace on USB Drive

```sql
-- Step 1: Create the tablespace (run as superuser)
-- The directory must exist and be owned by the postgres user BEFORE creating the tablespace.
CREATE TABLESPACE cold_storage LOCATION '/mnt/cold';

-- Step 2: Verify
SELECT spcname, pg_tablespace_location(oid) FROM pg_tablespace;
```

### Manual Chunk Movement

```sql
-- Move a specific compressed chunk to cold storage
SELECT move_chunk(
    chunk => '_timescaledb_internal._hyper_1_42_chunk',
    destination_tablespace => 'cold_storage',
    index_destination_tablespace => 'cold_storage',
    verbose => TRUE
);
```

### Automated Tiering Job

Create a custom background job that moves compressed chunks older than N days to the USB drive:

```sql
CREATE OR REPLACE FUNCTION tier_old_chunks(job_id INT, config JSONB)
RETURNS VOID AS $$
DECLARE
    chunk_rec RECORD;
    tier_after INTERVAL;
    moved_count INT := 0;
BEGIN
    tier_after := (config->>'tier_after')::interval;

    FOR chunk_rec IN
        SELECT chunk_schema || '.' || chunk_name AS chunk_full,
               chunk_name,
               range_start
        FROM timescaledb_information.chunks
        WHERE hypertable_name = 'events'
          AND is_compressed = true
          AND range_end < NOW() - tier_after
          -- Only move chunks not already on cold storage
          AND chunk_name NOT IN (
              SELECT c.relname
              FROM pg_class c
              JOIN pg_tablespace t ON c.reltablespace = t.oid
              WHERE t.spcname = 'cold_storage'
          )
        ORDER BY range_start ASC
    LOOP
        RAISE NOTICE 'Moving chunk % (range_start: %) to cold_storage',
            chunk_rec.chunk_name, chunk_rec.range_start;

        PERFORM move_chunk(
            chunk => chunk_rec.chunk_full::regclass,
            destination_tablespace => 'cold_storage',
            index_destination_tablespace => 'cold_storage'
        );
        moved_count := moved_count + 1;
    END LOOP;

    RAISE NOTICE 'tier_old_chunks: moved % chunks to cold_storage', moved_count;
END;
$$ LANGUAGE plpgsql;

-- Run daily at 04:00 UTC, move compressed chunks older than 14 days to USB
SELECT add_job(
    'tier_old_chunks',
    '1 day',
    config => '{"tier_after": "14 days"}'::jsonb,
    initial_start => '2026-03-05 04:00:00+00'::timestamptz
);
```

### Tiering Layout

```
NVMe (1 TB)                          USB-C SSD (4 TB)
+---------------------------------+   +----------------------------------+
| pg_default tablespace           |   | cold_storage tablespace          |
|                                 |   |                                  |
| Uncompressed chunks (0-2 days)  |   | Compressed chunks (14-180 days)  |
|   ~25 GB at steady state        |   |   ~80 GB at steady state         |
|                                 |   |                                  |
| Compressed chunks (2-14 days)   |   | WAL archive                     |
|   ~4 GB compressed              |   |   ~50-100 GB (retained 7 days)   |
|                                 |   |                                  |
| Indexes (on active chunks)      |   | pg_dump backups                  |
|   ~1.5 GB (2 day window)        |   |   ~20 GB per dump (compressed)   |
|                                 |   |                                  |
| WAL (active)                    |   |                                  |
|   4-8 GB                        |   |                                  |
|                                 |   |                                  |
| Other tables, aggregates        |   |                                  |
|   ~2 GB                         |   |                                  |
+---------------------------------+   +----------------------------------+
  ~35-40 GB used                       ~150-200 GB used
  ~960 GB free                         ~3.8 TB free
```

### Tradeoffs

| Factor | All on NVMe | Tiered (NVMe + USB) |
|--------|-------------|---------------------|
| Hot query performance | Excellent | Excellent (hot data stays on NVMe) |
| Cold query performance | Excellent | Good (USB-C sequential is fine for compressed reads) |
| Available NVMe space | ~775 GB | ~960 GB |
| Operational complexity | Simple | Medium (tablespace management, USB reliability) |
| USB drive failure impact | None | Loss of cold data + backups (recoverable from NVMe hot data + WAL) |

### Risk Mitigation: USB Drive Failure

If the USB drive fails or is disconnected:
- **Active database continues working** -- all hot data is on NVMe
- Queries touching cold chunks will fail with I/O errors for those specific chunks
- PostgreSQL will NOT crash -- errors are per-query, not system-wide
- The `nofail` fstab option prevents boot issues

Recovery: Replace drive, recreate tablespace, re-compress historical data from WAL or let the retention policy naturally rebuild.

### Estimated Space Savings

NVMe usage reduced from ~180-225 GB (everything) to ~35-40 GB (hot window only). The 4 TB USB drive holds all cold data with years of headroom.

---

## 6. Continuous Aggregate Optimization

### Current Aggregates

| Aggregate | Granularity | Refresh | Retention |
|-----------|-------------|---------|-----------|
| `events_5min` | 5 minutes | Every 1 minute | None (inherits from events) |
| `events_15min` | 15 minutes | Every 5 minutes | None |
| `events_hourly` | 1 hour | Every 30 minutes | 2 years |
| `events_daily` | 1 day | Every 1 hour | 5 years |
| `anomaly_baseline` | 1 hour | Every 1 hour | None |

### Recommendation: AIS-Specific Hourly Aggregate

Before dropping raw AIS events (section 2), ensure historical vessel traffic patterns are preserved in an aggregate:

```sql
-- AIS vessel traffic aggregate: hourly vessel counts and activity by region
CREATE MATERIALIZED VIEW IF NOT EXISTS ais_traffic_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)   AS bucket,
    region_code,
    COUNT(*)                            AS position_count,
    COUNT(DISTINCT entity_id)           AS unique_vessels,
    AVG(CASE WHEN payload->>'speed' IS NOT NULL
         THEN (payload->>'speed')::float ELSE NULL END) AS avg_speed,
    COUNT(*) FILTER (WHERE tags @> ARRAY['military']) AS military_count
FROM events
WHERE source_type = 'ais'
GROUP BY bucket, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('ais_traffic_hourly',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists => true
);

-- Keep AIS aggregates for 5 years (raw events only 7 days)
SELECT add_retention_policy('ais_traffic_hourly', INTERVAL '5 years',
    if_not_exists => true);
```

### Additional: Aviation Traffic Aggregate

```sql
CREATE MATERIALIZED VIEW IF NOT EXISTS aviation_traffic_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)   AS bucket,
    source_type,
    region_code,
    COUNT(*)                            AS position_count,
    COUNT(DISTINCT entity_id)           AS unique_aircraft,
    COUNT(*) FILTER (WHERE tags @> ARRAY['military']) AS military_count
FROM events
WHERE source_type IN ('airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky')
GROUP BY bucket, source_type, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('aviation_traffic_hourly',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists => true
);

SELECT add_retention_policy('aviation_traffic_hourly', INTERVAL '5 years',
    if_not_exists => true);
```

### Aggregate Size Estimate

Hourly aggregates produce ~24 rows/day per region. With ~16 AIS regions and ~4 ADSB point queries:
- AIS: 16 regions x 24 hours x 365 days x 5 years = ~700K rows (~50 MB)
- Aviation: 4 regions x 4 sources x 24 hours x 365 days x 5 years = ~7M rows (~500 MB)

Trivial compared to raw data.

### Tradeoffs

| Factor | Without aggregates | With aggregates |
|--------|-------------------|-----------------|
| AIS retention needed | 180 days (for trend queries) | 7 days (aggregates cover trends) |
| Historical trend queries | Fast (raw data available) | Fast (aggregate pre-computed) |
| Per-vessel historical queries | Available (180 days) | Lost after 7 days |
| Space for 5-year trends | ~180 GB (raw) | ~0.5 GB (aggregated) |

**Key tradeoff:** After reducing AIS raw retention to 7 days, you lose the ability to query individual vessel tracks older than 7 days. The aggregate preserves regional traffic patterns but not per-vessel detail. For a SIGINT/OSINT use case, this is usually acceptable -- military vessel tracking of interest should be captured in situations/entities, not raw events.

### Estimated Space Savings

Aggregates themselves add ~550 MB over 5 years. The savings come from enabling the 7-day AIS retention in section 2 -- approximately **83 GB** of steady-state reduction.

---

## 7. Source Deduplication

### Problem

Three aviation sources track the same aircraft simultaneously:

| Source | Daily Events | Avg Row Size | Overlap |
|--------|-------------|--------------|---------|
| airplaneslive | ~146K/day | 714 bytes | ~95% overlap with others |
| adsb-fi | ~145K/day | 697 bytes | ~95% overlap with others |
| adsb-lol | ~135K/day | 700 bytes | ~95% overlap with others |
| **Total** | **~426K/day** | | |

These sources query the same ADS-B data (from the same aircraft transponders, via different aggregators). The same aircraft appears in all three sources with slightly different timestamps and update frequencies.

### Option A: Dedup at Ingest (Recommended)

The cleanest approach is to deduplicate in the Rust application layer. The `AdsbAggregator` in `adsb.rs` already has a shared structure -- extend it with a dedup cache:

**Concept:**
```rust
// In-memory dedup: (icao_hex, last_seen_time) -> timestamp
// Skip insert if same aircraft was seen within the last N seconds from any ADSB source
struct AdsbDedup {
    seen: DashMap<String, Instant>,  // icao_hex -> last insert time
    dedup_window: Duration,          // e.g., 30 seconds
}

impl AdsbDedup {
    fn should_insert(&self, icao_hex: &str) -> bool {
        if let Some(last) = self.seen.get(icao_hex) {
            if last.elapsed() < self.dedup_window {
                return false; // Already seen recently from another source
            }
        }
        self.seen.insert(icao_hex.to_string(), Instant::now());
        true
    }
}
```

**Pros:**
- Zero DB overhead
- Reduces insert volume by ~66% (3 sources -> 1 effective source)
- Clean, deterministic behavior
- No schema changes

**Cons:**
- Requires Rust code changes in `adsb.rs` and `registry.rs`
- Slightly more complex: need a shared dedup state across the three ADSB source tasks
- First-writer-wins semantics (whichever source reports the position first "wins")

### Option B: Dedup in Database

Use the existing dedup index more aggressively:

```sql
-- The current dedup index:
-- CREATE UNIQUE INDEX idx_events_dedup ON events (source_type, source_id, event_time)
--     WHERE source_id IS NOT NULL;

-- Problem: This dedup is per-source_type. The same aircraft from airplaneslive
-- and adsb-fi have different source_type values, so both get inserted.

-- Solution: Create a cross-source dedup index on entity_id + event_time
-- with a coarser time granularity (round to nearest 30 seconds):
CREATE UNIQUE INDEX idx_events_aircraft_dedup
ON events (entity_id, date_trunc('minute', event_time))
WHERE source_type IN ('airplaneslive', 'adsb-fi', 'adsb-lol')
  AND entity_id IS NOT NULL;
```

**Problem with Option B:** The `INSERT ... ON CONFLICT DO NOTHING` uses the dedup index, but TimescaleDB's unique constraints on hypertables require the partitioning column in the constraint. Since `event_time` is the partition key and we are truncating it, this gets complex. The application-level dedup (Option A) is cleaner.

### Option C: Reduce to Single Source

The simplest approach: disable two of the three ADSB sources. AirplanesLive has the best coverage and update frequency (90-second polls with 3-second inter-request delay):

```sql
-- Disable redundant sources via the config table
UPDATE source_config SET enabled = false WHERE source_id IN ('adsb-fi', 'adsb-lol');
```

**Pros:** Immediate, zero code changes, 66% reduction
**Cons:** Lose redundancy -- if AirplanesLive goes down, no fallback

### Tradeoffs

| Approach | Events/Day Saved | Code Changes | Redundancy |
|----------|------------------|--------------|------------|
| Ingest dedup (A) | ~284K/day (~200 MB/day) | Moderate (Rust) | Maintained (any source fills gaps) |
| DB dedup (B) | ~284K/day | Complex (SQL + constraints) | Maintained |
| Single source (C) | ~280K/day (~196 MB/day) | None | Lost |

### Estimated Space Savings

~200 MB/day uncompressed, ~5 MB/day compressed. Over a 2-day uncompressed window: **~400 MB**. Modest but worthwhile as part of the overall optimization.

---

## 8. Backup Strategy

### Recommended Approach: Layered Backups on 4TB USB Drive

#### Layer 1: Logical Backups (pg_dump)

Weekly full dumps for maximum portability:

```bash
#!/bin/bash
# /opt/scripts/backup-situationreport.sh
# Run weekly via cron: 0 2 * * 0 /opt/scripts/backup-situationreport.sh

BACKUP_DIR="/mnt/usb-storage/pg-backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
CONTAINER="situationreport-postgres-1"
KEEP_WEEKS=8  # Keep 8 weekly backups (~160 GB max)

# Create compressed custom-format dump
docker exec ${CONTAINER} pg_dump \
    -U sitrep \
    -d situationreport \
    -Fc \
    --no-comments \
    -f /mnt/backups/situationreport_${TIMESTAMP}.dump

# Verify the dump
docker exec ${CONTAINER} pg_restore \
    --list /mnt/backups/situationreport_${TIMESTAMP}.dump > /dev/null 2>&1

if [ $? -eq 0 ]; then
    echo "$(date): Backup verified: situationreport_${TIMESTAMP}.dump"
else
    echo "$(date): WARNING: Backup verification failed!"
fi

# Rotate old backups
ls -t ${BACKUP_DIR}/situationreport_*.dump | tail -n +$((KEEP_WEEKS + 1)) | xargs -r rm

# Log size
du -sh ${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump
```

**Expected dump size:** With compression, pg_dump custom format produces ~5-15% of the raw DB size. For a 100 GB database: ~5-15 GB per dump. 8 weekly backups = ~40-120 GB.

#### Layer 2: Continuous WAL Archiving (Point-in-Time Recovery)

For point-in-time recovery between pg_dump snapshots. Add to the Docker Compose postgres command:

```yaml
services:
  postgres:
    command:
      # ... existing flags ...
      - "-c"
      - "archive_mode=on"
      - "-c"
      - "archive_command=test ! -f /mnt/wal/%f && cp %p /mnt/wal/%f"
      - "-c"
      - "archive_timeout=300"  # Archive incomplete WAL segment every 5 minutes
```

WAL cleanup script (prevent unbounded growth):
```bash
#!/bin/bash
# /opt/scripts/clean-wal-archive.sh
# Run daily: 0 5 * * * /opt/scripts/clean-wal-archive.sh

WAL_DIR="/mnt/usb-storage/pg-wal-archive"
KEEP_DAYS=7

find ${WAL_DIR} -name "0000*" -mtime +${KEEP_DAYS} -delete

echo "$(date): WAL archive cleaned (kept last ${KEEP_DAYS} days)"
du -sh ${WAL_DIR}
```

**Expected WAL volume:** With AIS generating ~9.3M inserts/day, WAL generation is significant -- estimate **10-20 GB/day** of WAL files. With 7-day retention: ~70-140 GB on the USB drive. The 4 TB drive has ample room.

#### Layer 3: pg_basebackup (Physical Backup)

Monthly physical backup for fastest possible recovery:

```bash
#!/bin/bash
# /opt/scripts/basebackup-situationreport.sh
# Run monthly: 0 3 1 * * /opt/scripts/basebackup-situationreport.sh

BACKUP_DIR="/mnt/usb-storage/pg-backups/base"
TIMESTAMP=$(date +%Y%m%d)
CONTAINER="situationreport-postgres-1"

mkdir -p ${BACKUP_DIR}

# Take a compressed base backup
docker exec ${CONTAINER} pg_basebackup \
    -U sitrep \
    -D /mnt/backups/base/base_${TIMESTAMP} \
    -Ft -z -P \
    --wal-method=stream

echo "$(date): Base backup complete: base_${TIMESTAMP}"
du -sh ${BACKUP_DIR}/base_${TIMESTAMP}

# Keep last 3 monthly base backups
ls -dt ${BACKUP_DIR}/base_* | tail -n +4 | xargs -r rm -rf
```

### Recovery Procedures

**From pg_dump (any point in the last 8 weeks):**
```bash
# 1. Stop the app container
docker compose stop app

# 2. Drop and recreate the database
docker exec -i situationreport-postgres-1 psql -U sitrep -c "DROP DATABASE situationreport;"
docker exec -i situationreport-postgres-1 psql -U sitrep -c "CREATE DATABASE situationreport;"
docker exec -i situationreport-postgres-1 psql -U sitrep -d situationreport -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
docker exec -i situationreport-postgres-1 psql -U sitrep -d situationreport -c "SELECT timescaledb_pre_restore();"

# 3. Restore
docker exec -i situationreport-postgres-1 pg_restore \
    -U sitrep -d situationreport \
    /mnt/backups/situationreport_YYYYMMDD_HHMMSS.dump

docker exec -i situationreport-postgres-1 psql -U sitrep -d situationreport -c "SELECT timescaledb_post_restore();"

# 4. Restart
docker compose up -d
```

**From base backup + WAL (point-in-time recovery):**
```bash
# 1. Stop postgres container
docker compose stop postgres

# 2. Replace data directory with base backup
sudo rm -rf /path/to/pgdata/*
sudo tar -xzf /mnt/usb-storage/pg-backups/base/base_YYYYMMDD/base.tar.gz -C /path/to/pgdata/

# 3. Create recovery.signal and configure restore_command
echo "restore_command = 'cp /mnt/wal/%f %p'" > /path/to/pgdata/recovery.signal
echo "recovery_target_time = '2026-03-04 12:00:00 UTC'" >> /path/to/pgdata/postgresql.auto.conf

# 4. Start postgres (it will replay WAL to the target time)
docker compose up -d postgres
```

### Backup Storage Budget

| Layer | Frequency | Retention | Estimated Size |
|-------|-----------|-----------|----------------|
| pg_dump | Weekly | 8 weeks | ~40-120 GB |
| WAL archive | Continuous | 7 days | ~70-140 GB |
| pg_basebackup | Monthly | 3 months | ~30-45 GB |
| **Total** | | | **~140-305 GB** |

This fits comfortably on the 4 TB USB drive alongside the tiered cold storage.

### Tradeoffs

| Factor | pg_dump only | pg_dump + WAL | All three layers |
|--------|-------------|---------------|------------------|
| Recovery granularity | Weekly | Any second (within 7 days) | Any second + fast monthly base |
| Recovery time (100 GB DB) | ~30-60 min | ~10-30 min (WAL replay) | ~5-10 min (base + short WAL replay) |
| Storage cost | ~40-120 GB | ~110-260 GB | ~140-305 GB |
| Operational complexity | Low | Medium | Medium-High |
| Setup effort | 1 cron job | Docker config + 2 cron jobs | Docker config + 3 cron jobs |

**Recommendation:** Start with Layer 1 (pg_dump weekly) immediately. Add Layer 2 (WAL archiving) after the USB drive is set up and tested. Add Layer 3 (monthly base backup) once you are comfortable with the WAL archiving workflow.

---

## 9. Recommended Implementation Order

Prioritized by impact-to-effort ratio, with dependencies noted.

### Phase 1: Immediate (This Week) -- No USB Drive Needed

| # | Action | Effort | Impact | Savings |
|---|--------|--------|--------|---------|
| 1 | **Reduce compression delay to 2 days** | 5 min (SQL) | High | ~64 GB peak reduction |
| 2 | **Create partial GIN index** | 10 min (SQL) | Medium | ~377 MB/day on indexes |
| 3 | **Create AIS/aviation aggregates** | 15 min (SQL) | Low now, enables Phase 2 | Enables AIS retention reduction |

**Commands for Phase 1:**

```sql
-- 1. Compression delay
SELECT remove_compression_policy('events');
SELECT add_compression_policy('events', INTERVAL '2 days');
-- Manually compress existing older chunks:
SELECT compress_chunk(c, if_not_compressed => true)
FROM show_chunks('events', older_than => INTERVAL '2 days') c;

-- 2. Partial GIN index
CREATE INDEX CONCURRENTLY idx_events_payload_filtered
ON events USING GIN (payload)
WHERE source_type NOT IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky', 'bgp');
-- Verify queries still work, then:
DROP INDEX CONCURRENTLY idx_events_payload;

-- 3. AIS aggregate
-- (Use the SQL from Section 6 above)
```

### Phase 2: Short-Term (After Aggregates Are Populated, ~3 Days)

| # | Action | Effort | Impact | Savings |
|---|--------|--------|--------|---------|
| 4 | **Add AIS-specific 7-day retention job** | 20 min (SQL) | Very High | ~83 GB steady-state reduction |
| 5 | **Disable redundant ADSB sources** | 2 min (SQL) | Medium | ~200 MB/day |

Wait for the AIS hourly aggregate to have at least 3 days of data before enabling the 7-day AIS retention, so you do not lose historical context.

### Phase 3: USB Drive Setup (When the 4TB Drive Arrives)

| # | Action | Effort | Impact | Savings |
|---|--------|--------|--------|---------|
| 6 | **Mount and format USB drive** | 30 min (shell) | Foundation | -- |
| 7 | **Set up pg_dump weekly backup** | 15 min (cron) | High (disaster recovery) | Peace of mind |
| 8 | **Create cold_storage tablespace** | 10 min (SQL) | Medium | Frees NVMe space |
| 9 | **Set up tiered storage job** | 20 min (SQL) | Medium | NVMe: 180 GB -> 35 GB |
| 10 | **Enable WAL archiving** | 20 min (Docker + cron) | High (PITR capability) | -- |

### Phase 4: Advanced (This Quarter)

| # | Action | Effort | Impact | Savings |
|---|--------|--------|--------|---------|
| 11 | **Ingest-level ADSB dedup** | 2-4 hours (Rust) | Low-Medium | ~200 MB/day |
| 12 | **Monthly pg_basebackup** | 15 min (cron) | Medium (faster recovery) | -- |
| 13 | **Evaluate separate tracking hypertable** | Days (Rust + SQL) | High (architectural) | Cleaner long-term |

### Expected Steady-State After All Optimizations

| Component | Before | After Phase 1-2 | After Phase 3 (NVMe) | USB Drive |
|-----------|--------|-----------------|---------------------|-----------|
| Uncompressed window | ~89 GB | ~25 GB | ~25 GB | -- |
| Compressed events (NVMe) | ~91 GB | ~8 GB | ~4 GB | ~4 GB |
| Compressed events (USB) | -- | -- | -- | ~4 GB |
| Indexes (uncompressed) | ~44 GB | ~1.5 GB | ~1.5 GB | -- |
| Position history | ~0.5 GB | ~0.5 GB | ~0.5 GB | -- |
| Aggregates | ~0.5 GB | ~1 GB | ~1 GB | -- |
| Other tables | ~0.2 GB | ~0.2 GB | ~0.2 GB | -- |
| Backups | -- | -- | -- | ~140-305 GB |
| **Total** | **~180-225 GB** | **~36 GB** | **~32 GB** | **~150-310 GB** |

With all optimizations, NVMe usage drops from a potential 225 GB steady-state to approximately 32 GB, leaving over 960 GB of NVMe headroom. The 4 TB USB drive handles cold storage and backups with years of room to grow.

---

## Appendix A: Monitoring Queries

### Check Compression Status and Ratios

```sql
SELECT
    chunk_name,
    range_start::date,
    range_end::date,
    is_compressed,
    pg_size_pretty(before_compression_total_bytes) AS before,
    pg_size_pretty(after_compression_total_bytes) AS after,
    CASE WHEN after_compression_total_bytes > 0
         THEN round(before_compression_total_bytes::numeric / after_compression_total_bytes, 1)
         ELSE NULL END AS ratio
FROM timescaledb_information.chunks
WHERE hypertable_name = 'events'
ORDER BY range_start DESC
LIMIT 20;
```

### Check Chunk Tablespace Distribution

```sql
SELECT
    t.spcname AS tablespace,
    COUNT(*) AS chunk_count,
    pg_size_pretty(SUM(pg_total_relation_size(c.oid))) AS total_size
FROM timescaledb_information.chunks ch
JOIN pg_class c ON c.relname = ch.chunk_name
LEFT JOIN pg_tablespace t ON c.reltablespace = t.oid
WHERE ch.hypertable_name = 'events'
GROUP BY t.spcname;
```

### Check Custom Job Status

```sql
SELECT
    j.job_id,
    j.proc_name,
    j.schedule_interval,
    js.last_run_started_at,
    js.last_successful_finish,
    js.last_run_status,
    js.total_runs,
    js.total_failures
FROM timescaledb_information.jobs j
JOIN timescaledb_information.job_stats js ON j.job_id = js.job_id
ORDER BY j.job_id;
```

### Check Index Sizes on Current (Uncompressed) Chunks

```sql
SELECT
    indexrelname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS index_size
FROM pg_stat_user_indexes
WHERE schemaname = '_timescaledb_internal'
  AND relname LIKE '_hyper_1_%'
ORDER BY pg_relation_size(indexrelid) DESC
LIMIT 20;
```

## Appendix B: Rollback Procedures

### Revert Compression Delay

```sql
SELECT remove_compression_policy('events');
SELECT add_compression_policy('events', INTERVAL '7 days');
```

### Revert Partial GIN Index

```sql
-- Recreate the full GIN index
CREATE INDEX CONCURRENTLY idx_events_payload ON events USING GIN (payload);
-- Then drop the partial one
DROP INDEX CONCURRENTLY idx_events_payload_filtered;
```

### Remove Custom Jobs

```sql
-- Find and remove custom jobs
SELECT job_id, proc_name FROM timescaledb_information.jobs
WHERE proc_name IN ('selective_retention', 'tier_old_chunks');

-- Remove by job_id
SELECT delete_job(<JOB_ID>);
```

### Revert Tiered Storage

```sql
-- Move chunks back to default tablespace
SELECT move_chunk(
    chunk => ch.chunk_name::regclass,
    destination_tablespace => 'pg_default',
    index_destination_tablespace => 'pg_default'
)
FROM timescaledb_information.chunks ch
JOIN pg_class c ON c.relname = ch.chunk_name
JOIN pg_tablespace t ON c.reltablespace = t.oid
WHERE t.spcname = 'cold_storage'
  AND ch.hypertable_name = 'events';

-- Then drop the tablespace
DROP TABLESPACE cold_storage;
```
