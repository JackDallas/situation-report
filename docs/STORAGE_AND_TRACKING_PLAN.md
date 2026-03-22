# Storage & Global Tracking Implementation Plan

**Date:** 2026-03-05
**Status:** Plan -- no changes applied yet
**Prerequisites:** STORAGE_OPTIMIZATION.md (analysis), DATA_GROWTH_PROJECTION.md (projections)

---

## Executive Summary

This plan covers two coupled changes:

1. **Tiered storage** -- move cold compressed data to the USB-C SSD at `/mnt/sitrep-cold`, reduce compression delay, automate backups to the SATA WD Blue.
2. **Global tracking** -- remove all geographic bounding boxes from AIS and aviation sources to consume worldwide vessel and aircraft data.

These changes are coupled because global tracking will increase data volume by 5-30x, making tiered storage a prerequisite rather than a nice-to-have.

### Hardware Summary

| Device | Mount | Capacity | Free | Write Speed | Role |
|--------|-------|----------|------|-------------|------|
| Samsung 990 PRO NVMe | `/` (pgdata on named volume) | 930 GB | 226 GB | 4,300 MB/s | Hot data (uncompressed chunks, indexes, WAL) |
| Crucial X9 USB-C SSD | `/mnt/sitrep-cold` | 932 GB | 932 GB (fresh) | 908 MB/s | Cold storage (compressed chunks >14d) |
| WD Blue SATA SSD | `/run/media/system/Storage` | 2 TB | 1.5 TB | 434 MB/s | Backups (pg_dump, WAL archive) |
| 4 TB USB-C (arriving) | TBD | ~3.7 TB | ~3.7 TB | ~900 MB/s | Replaces Crucial X9 as cold storage |

---

## Part 1: Tiered Storage Setup

### 1.1 USB-C Drive Preparation (Crucial X9 at `/mnt/sitrep-cold`)

The drive is being formatted as ext4. Verify mount and set ownership for the PostgreSQL container user (UID 1000 in `timescale/timescaledb-ha:pg17`).

```bash
# Verify the mount
mount | grep sitrep-cold
df -h /mnt/sitrep-cold

# Verify the UID PostgreSQL runs as inside the container
docker exec situationreport-postgres-1 id
# Expected: uid=1000(postgres)

# Create directory structure
sudo mkdir -p /mnt/sitrep-cold/pg-cold
sudo chown -R 1000:1000 /mnt/sitrep-cold/pg-cold

# Ensure fstab entry includes nofail (prevents boot hang if drive disconnected)
# Should look like:
# UUID=<uuid> /mnt/sitrep-cold ext4 noatime,data=ordered,discard,nofail 0 2
```

### 1.2 Docker Compose Changes

**File:** `/Users/dallas/git/osint/situationreport/docker-compose.yml`

Add the USB-C mount to the postgres service volumes:

```yaml
services:
  postgres:
    image: timescale/timescaledb-ha:pg17
    volumes:
      - ${PG_DATA_DIR:-pgdata}:/home/postgres/pgdata/data
      - /mnt/sitrep-cold/pg-cold:/mnt/cold    # Cold tablespace for tiered storage
    # ... rest unchanged
```

After changing `docker-compose.yml`, recreate the postgres container:

```bash
cd /Users/dallas/git/osint/situationreport
docker compose down postgres
docker compose up -d postgres
# Wait for healthcheck to pass
docker compose ps
```

Verify the mount is visible inside the container:

```bash
docker exec situationreport-postgres-1 ls -la /mnt/cold
# Should show empty directory owned by postgres (uid 1000)
```

### 1.3 Create PostgreSQL Tablespace on USB-C Drive

Connect to the database and create the tablespace:

```sql
-- Run as superuser (sitrep user has superuser in this setup)
-- The /mnt/cold directory must exist and be owned by postgres BEFORE this command
CREATE TABLESPACE cold_storage LOCATION '/mnt/cold';

-- Verify
SELECT spcname, pg_tablespace_location(oid) FROM pg_tablespace;
-- Expected row: cold_storage | /mnt/cold
```

### 1.4 Compression Delay Reduction

**Current:** 7 days (uncompressed window = ~89 GB/day * 7 = ~89 GB at current AIS rate)
**Target:** 2 days (uncompressed window = ~25 GB at current rate)

Why 2 days and not 1 day: The enrichment pipeline (`enrich.rs`) attaches `payload.enrichment` to events via UPDATE. A 2-day window gives the enrichment pipeline ample time, handles container restarts gracefully, and still provides 72% of the space savings compared to 1 day. See STORAGE_OPTIMIZATION.md section 1 for full tradeoff analysis.

```sql
-- Step 1: Find the current compression job
SELECT j.job_id, j.schedule_interval, j.config
FROM timescaledb_information.jobs j
WHERE j.proc_name = 'policy_compression'
  AND j.hypertable_name = 'events';
-- Note the job_id (likely 1003)

-- Step 2: Replace the compression policy
SELECT remove_compression_policy('events');
SELECT add_compression_policy('events', INTERVAL '2 days');

-- Step 3: Compress existing uncompressed chunks older than 2 days
SELECT compress_chunk(c, if_not_compressed => true)
FROM show_chunks('events', older_than => INTERVAL '2 days') c;

-- Step 4: Verify compression state
SELECT chunk_name,
       range_start,
       range_end,
       is_compressed,
       before_compression_total_bytes,
       after_compression_total_bytes
FROM timescaledb_information.chunks
WHERE hypertable_name = 'events'
ORDER BY range_start DESC
LIMIT 20;
```

**Estimated impact:** Reduces peak uncompressed data from ~89 GB to ~25 GB at current AIS rate. With global tracking enabled, this becomes even more critical (see Part 2).

### 1.5 Automated Tiering Job (Move Cold Chunks to USB-C)

Create a background job that moves compressed chunks older than 14 days to the cold tablespace:

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

-- Run daily at 04:00 UTC
SELECT add_job(
    'tier_old_chunks',
    '1 day',
    config => '{"tier_after": "14 days"}'::jsonb,
    initial_start => '2026-03-06 04:00:00+00'::timestamptz
);
```

**Result:** Chunks 0-2 days old stay uncompressed on NVMe. Chunks 2-14 days old stay compressed on NVMe. Chunks 14+ days old move to USB-C (compressed).

### 1.6 Partial GIN Index (Reduce Index Bloat)

The JSONB payload GIN index consumes ~397 MB/day -- more than any other single index. AIS/ADSB payloads are never queried via GIN operators.

```sql
-- Create the partial index (CONCURRENTLY avoids locking)
CREATE INDEX CONCURRENTLY idx_events_payload_filtered
ON events USING GIN (payload)
WHERE source_type NOT IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky', 'bgp');

-- Verify the planner uses it for typical queries
EXPLAIN ANALYZE
SELECT * FROM events
WHERE payload ? 'enrichment'
  AND source_type = 'rss-news'
  AND event_time > NOW() - INTERVAL '1 day';

-- Only after verification: drop the old full index
DROP INDEX CONCURRENTLY idx_events_payload;
```

**Estimated savings:** ~377 MB/day in the uncompressed window.

### 1.7 Backup Strategy (WD Blue SATA at `/run/media/system/Storage`)

#### Docker Compose Changes for Backup Mount

```yaml
services:
  postgres:
    volumes:
      - ${PG_DATA_DIR:-pgdata}:/home/postgres/pgdata/data
      - /mnt/sitrep-cold/pg-cold:/mnt/cold
      - /run/media/system/Storage/pg-backups:/mnt/backups
      - /run/media/system/Storage/pg-wal-archive:/mnt/wal
```

Prepare the directories:

```bash
sudo mkdir -p /run/media/system/Storage/pg-backups
sudo mkdir -p /run/media/system/Storage/pg-wal-archive
sudo chown -R 1000:1000 /run/media/system/Storage/pg-backups
sudo chown -R 1000:1000 /run/media/system/Storage/pg-wal-archive
```

#### WAL Archiving

Add to the postgres command in `docker-compose.yml`:

```yaml
      - "-c"
      - "archive_mode=on"
      - "-c"
      - "archive_command=test ! -f /mnt/wal/%f && cp %p /mnt/wal/%f"
      - "-c"
      - "archive_timeout=300"
```

#### Weekly pg_dump Script

```bash
#!/bin/bash
# /opt/scripts/backup-situationreport.sh
# Cron: 0 2 * * 0 /opt/scripts/backup-situationreport.sh

CONTAINER="situationreport-postgres-1"
BACKUP_DIR="/mnt/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
KEEP_WEEKS=8

docker exec ${CONTAINER} pg_dump \
    -U sitrep \
    -d situationreport \
    -Fc \
    --no-comments \
    -f ${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump

# Verify
docker exec ${CONTAINER} pg_restore \
    --list ${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump > /dev/null 2>&1

if [ $? -eq 0 ]; then
    echo "$(date): Backup OK: situationreport_${TIMESTAMP}.dump"
else
    echo "$(date): BACKUP VERIFICATION FAILED" >&2
fi

# Rotate (keep last 8)
docker exec ${CONTAINER} bash -c \
    "ls -t ${BACKUP_DIR}/situationreport_*.dump | tail -n +$((KEEP_WEEKS + 1)) | xargs -r rm"
```

#### WAL Cleanup Script

```bash
#!/bin/bash
# /opt/scripts/clean-wal-archive.sh
# Cron: 0 5 * * * /opt/scripts/clean-wal-archive.sh

WAL_DIR="/run/media/system/Storage/pg-wal-archive"
KEEP_DAYS=7
find ${WAL_DIR} -name "0000*" -mtime +${KEEP_DAYS} -delete
echo "$(date): WAL archive cleaned (kept ${KEEP_DAYS} days)"
du -sh ${WAL_DIR}
```

---

## Part 2: Global Tracking (Remove Bounding Boxes)

### 2.1 AIS Changes

#### Current State

**File:** `/Users/dallas/git/osint/situationreport/backend/crates/sources/src/ais.rs`

The AIS source subscribes to aisstream.io with 4 bounding boxes defined in the `BOUNDING_BOXES` constant (lines 26-35):

```rust
const BOUNDING_BOXES: &[(&str, [[f64; 2]; 2])] = &[
    ("Europe", [[25.0, -15.0], [70.0, 45.0]]),
    ("Middle East", [[0.0, 30.0], [42.0, 75.0]]),
    ("Indian Ocean", [[-15.0, 40.0], [15.0, 100.0]]),
    ("East Asia", [[-10.0, 95.0], [50.0, 150.0]]),
];
```

These boxes are sent to aisstream.io in `build_subscribe_message()` (line 54-62).

#### Required Changes

**Option A (Recommended): Single global bounding box**

Replace the `BOUNDING_BOXES` constant with a single global box:

```rust
/// Global bounding box -- receive all AIS data worldwide.
const BOUNDING_BOXES: &[(&str, [[f64; 2]; 2])] = &[
    ("Global", [[-90.0, -180.0], [90.0, 180.0]]),
];
```

This is the simplest change -- a single line replacement. The `build_subscribe_message()` function already iterates over `BOUNDING_BOXES` and sends them as an array, so a single-element array works fine.

**Additional changes in `ais.rs`:**

1. **Update the info log** at line 437-439: Change `regions = BOUNDING_BOXES.len()` to something more descriptive since there is only one "region" now.

2. **Expand `NAMED_REGIONS`** (lines 65-82): Add regions for areas not currently covered. The current list already covers major global chokepoints, but consider adding:
   - Western Hemisphere: Gulf of Mexico, Caribbean, Panama Canal, Strait of Magellan
   - Africa: Gulf of Guinea, Mozambique Channel, Cape of Good Hope
   - Pacific: Sea of Okhotsk, Bering Sea, Coral Sea
   - Americas: US East Coast, US West Coast, Great Lakes

3. **Update the test** at line 576: Change `assert_eq!(boxes.len(), 4)` to `assert_eq!(boxes.len(), 1)`.

4. **Expand `MILITARY_MMSI_PREFIXES`** (lines 39-43): The current list only has US and Iran. Consider adding major navies whose vessels will now be visible globally:
   - `"211"` -- Germany (common MID, may need tighter filtering)
   - `"226"` -- France
   - `"235"` -- UK
   - `"273"` -- Russia
   - `"431"` -- Japan
   - `"412"` -- China

   **Caution:** Country MID prefixes are shared between military and civilian vessels. The current approach of only listing known military-specific allocations (338, 369, 422) avoids false positives. Expanding this list broadly would mark many civilian ships as military. A better approach is to use external databases (like MarineTraffic's military vessel list) or ship-type codes in ShipStaticData messages. For the initial rollout, keep the current conservative list and add more over time based on observed data.

#### Volume Projection: Global AIS

**Current (4 bounding boxes):** ~9.3M messages/day (~387K/hour)

**Global estimate:** aisstream.io is a terrestrial AIS network with coverage approximately 200 km from coastlines worldwide. Based on industry data:

- VT Explorer processes 300+ million AIS messages/day globally from combined terrestrial + satellite feeds
- aisstream.io is terrestrial-only, so fewer messages than satellite-inclusive providers
- Terrestrial AIS covers roughly 60-70% of global vessel traffic (ships near coasts and in major shipping lanes)
- There are approximately 200,000+ AIS-equipped vessels worldwide
- The 4 current boxes cover roughly 25-30% of global coastal shipping lanes (Europe + Middle East + Indian Ocean + East Asia)

**Conservative estimate:** The current 4 boxes capture the densest shipping regions. Going global adds the Americas (Gulf of Mexico, US coasts, Caribbean, Panama Canal), Africa (Gulf of Guinea, Cape of Good Hope), Australia/New Zealand, and lower-density routes. Expect a **3-5x increase** over the current rate:

| Scenario | Messages/day | Messages/hour |
|----------|-------------|---------------|
| Current (4 boxes) | ~9.3M | ~387K |
| Global (conservative, 3x) | ~28M | ~1.2M |
| Global (moderate, 5x) | ~47M | ~1.95M |
| Global (high, 8x) | ~74M | ~3.1M |

**Most likely range: 25-50M messages/day.** The current 4 boxes already cover the highest-density shipping regions (Mediterranean, Persian Gulf, South China Sea, Malacca Strait), so the multiplier is lower than simple area ratios suggest. Western Hemisphere and African waters are significantly less dense.

### 2.2 Aviation Changes

#### Current State

**ADS-B Sources (`backend/crates/sources/src/adsb.rs`):**

The `AdsbAggregator` uses two query strategies per poll:
1. **Military endpoint** (`/mil`) -- returns ALL military aircraft worldwide. **Already global.** No change needed.
2. **Rotated point queries** from `POINT_QUERIES` (line 62-67):
   ```rust
   const POINT_QUERIES: &[(&str, f64, f64, u32)] = &[
       ("iran", 32.5, 53.0, 250),
       ("levant", 33.0, 36.0, 200),
       ("persian_gulf", 26.5, 52.0, 250),
       ("red_sea_yemen", 15.0, 43.0, 250),
   ];
   ```
3. **Squawk 7700** (emergency) -- already global.

Three services use this: `airplaneslive()`, `adsb_lol()`, `adsb_fi()` -- all poll `/mil` globally and rotate through the same 4 regional point queries.

**OpenSky (`backend/crates/sources/src/opensky.rs`):**

OpenSky uses bounding boxes defined in its own `BOUNDING_BOXES` constant (lines 17-22):
```rust
const BOUNDING_BOXES: &[(&str, f64, f64, f64, f64)] = &[
    ("middle_east", 25.0, 34.0, 42.0, 63.0),
    ("ukraine", 44.0, 22.0, 53.0, 40.0),
    ("persian_gulf", 22.0, 46.0, 31.0, 57.0),
    ("red_sea_yemen", 10.0, 38.0, 20.0, 48.0),
];
```

It rotates through these bounding boxes one per poll (90-second interval, so all 4 regions polled every 360 seconds).

#### Required Changes

**For `adsb.rs` (AirplanesLive, adsb.lol, adsb.fi):**

Expand `POINT_QUERIES` to cover global regions. The readsb API point queries use (lat, lon, radius_nm) so we need to add major coverage areas:

```rust
const POINT_QUERIES: &[(&str, f64, f64, u32)] = &[
    // Existing
    ("iran", 32.5, 53.0, 250),
    ("levant", 33.0, 36.0, 200),
    ("persian_gulf", 26.5, 52.0, 250),
    ("red_sea_yemen", 15.0, 43.0, 250),
    // Europe
    ("central_europe", 48.0, 10.0, 350),
    ("eastern_med", 36.0, 28.0, 250),
    ("scandinavia", 60.0, 15.0, 300),
    ("uk_north_atlantic", 54.0, -5.0, 300),
    // Asia-Pacific
    ("taiwan_strait", 24.0, 120.0, 200),
    ("south_china_sea", 12.0, 112.0, 300),
    ("korea_japan", 37.0, 130.0, 300),
    ("india", 20.0, 78.0, 350),
    ("se_asia", 2.0, 105.0, 300),
    // Americas
    ("us_east_coast", 38.0, -76.0, 350),
    ("us_west_coast", 37.0, -122.0, 300),
    ("gulf_mexico", 26.0, -90.0, 350),
    ("caribbean", 18.0, -68.0, 300),
    // Africa
    ("horn_of_africa", 8.0, 47.0, 250),
    ("west_africa", 6.0, 3.0, 250),
    ("southern_africa", -30.0, 28.0, 300),
    // Oceania
    ("australia_east", -33.0, 151.0, 300),
];
```

**Impact on polling:** With 22 regions instead of 4, and 3 services rotating through them, each service takes 22 polls to complete a full rotation. At 120-second intervals, a full rotation takes 22 * 120 = 2,640 seconds (~44 minutes). This is acceptable -- the `/mil` endpoint already provides global military coverage every poll, and the regional queries supplement with civilian traffic.

**For `opensky.rs`:**

Expand `BOUNDING_BOXES` similarly. However, OpenSky has API credit limits (4,000 credits/day for authenticated users). With 4 regions and 90-second intervals: 4 * (86400/90) = 3,840 credits/day (nearly maxed). Going global would exceed limits.

**OpenSky strategy:** Keep OpenSky as a regional cross-reference source. Do NOT expand its bounding boxes to global. It serves as corroboration for the primary ADS-B sources (AirplanesLive, adsb.lol, adsb.fi) which have no API credit limits. Optionally, slowly add a few more regions to OpenSky as credit budget permits, or switch to fewer but larger boxes:

```rust
// Option: larger boxes, fewer of them (still 4 to stay within credit budget)
const BOUNDING_BOXES: &[(&str, f64, f64, f64, f64)] = &[
    ("europe_mideast", 25.0, -15.0, 55.0, 63.0),  // Combined
    ("east_asia", 0.0, 95.0, 50.0, 150.0),
    ("americas", 15.0, -130.0, 55.0, -60.0),
    ("africa_indian", -35.0, -20.0, 25.0, 80.0),
];
```

#### Aviation Volume Projection

**Current (4 regional point queries + /mil + /sqk/7700):**

| Source | Daily Events |
|--------|-------------|
| airplaneslive | ~146K |
| adsb-fi | ~145K |
| adsb-lol | ~135K |
| opensky | ~21K |
| **Total** | **~447K** |

The `/mil` endpoint already returns global military aircraft (~1,500-3,000 unique per query). The regional point queries add civilian traffic in 4 areas.

**With expanded point queries (22 regions):**

The `/mil` and `/sqk/7700` endpoints are already global -- their contribution stays the same. The regional point queries will increase because:
- More regions = more civilian aircraft captured per rotation
- Dense areas (US East Coast, Central Europe, East Asia) may return 500-2,000 aircraft each
- Less dense areas return fewer

Estimate per ADS-B source: from ~145K/day to ~500K-700K/day (3.5-5x increase).

| Scenario | Daily Events (all aviation) |
|----------|---------------------------|
| Current (4 regions) | ~447K |
| Global (22 regions, conservative) | ~1.5M |
| Global (22 regions, high) | ~2.5M |

**Most likely: ~1.5-2M aviation events/day total across all sources.**

### 2.3 Storage Impact Analysis

#### New Daily Data Volume

| Source | Current Daily | Global Daily (est.) | Avg Row Size | Daily GB (uncompressed) |
|--------|--------------|--------------------|--------------|-----------------------|
| AIS | 9.3M | 28-47M | 627 bytes | 17-28 GB |
| Aviation (all) | 447K | 1.5-2M | 700 bytes | 1.0-1.4 GB |
| Non-tracking (news, cyber, etc.) | 444K | 444K (unchanged) | ~1,500 bytes | 0.65 GB |
| **Total** | **~10.2M** | **~30-49M** | | **19-30 GB/day** |

With indexes (~80% of table data):

| Metric | Current | Global (conservative) | Global (high) |
|--------|---------|----------------------|---------------|
| Daily raw data + indexes | ~12.7 GB | ~34 GB | ~54 GB |
| 2-day uncompressed window | ~25 GB | ~68 GB | ~108 GB |
| 7-day compressed (on NVMe) | ~2 GB | ~5.6 GB | ~9 GB |
| 14-day compressed (on NVMe) | ~4 GB | ~11 GB | ~18 GB |
| 173-day compressed (180d retain) | ~91 GB | ~250 GB | ~400 GB |

#### Will the Hardware Be Enough?

**NVMe (930 GB, 226 GB free):**

With 2-day compression delay and the partial GIN index:

| Scenario | Uncompressed (2d) | Compressed (2-14d) | WAL + Other | Total NVMe | Fits? |
|----------|------------------|-------------------|-------------|-----------|-------|
| Current (4 boxes) | ~25 GB | ~4 GB | ~15 GB | ~44 GB | Yes |
| Global conservative | ~68 GB | ~11 GB | ~25 GB | ~104 GB | Yes (226 GB free) |
| Global high | ~108 GB | ~18 GB | ~35 GB | ~161 GB | Yes (tight) |

**Even the high scenario fits on the NVMe** with 2-day compression. Without the compression delay reduction (staying at 7 days), the high scenario would need ~378 GB for the uncompressed window -- dangerously close to the 226 GB free.

**Crucial X9 USB-C (932 GB):**

Cold storage (compressed chunks 14-180 days):

| Scenario | Cold Data (166 days compressed @ 42:1) | Fits? |
|----------|---------------------------------------|-------|
| Current | ~80 GB | Yes (852 GB free) |
| Global conservative | ~220 GB | Yes (712 GB free) |
| Global high | ~350 GB | Yes (582 GB free) |

All scenarios fit, but the high scenario leaves only 582 GB free on a 932 GB drive. The 4 TB drive arriving later provides much more headroom.

**With 4 TB USB-C drive:**

Cold storage is completely unconstrained. Even at global high volume with 1-year retention (instead of 180 days), the 4 TB drive can hold it:
- 365 days * 54 GB/day / 42 (compression) = ~470 GB compressed
- Plenty of room for backups and WAL archives too

**With 7-day tracking retention (recommended):**

If tracking events (AIS, ADSB) are retained for only 7 days while intel events keep 180 days:

| Scenario | Tracking (7d compressed) | Intel (180d compressed) | Total Cold | Fits X9? |
|----------|------------------------|------------------------|------------|----------|
| Global conservative | ~6 GB | ~10 GB | ~16 GB | Yes |
| Global high | ~9 GB | ~10 GB | ~19 GB | Yes |

This dramatically reduces storage needs. The 7-day tracking retention combined with hourly aggregates (see section 2.5) is the recommended approach.

### 2.4 Performance Considerations

#### Can the Pipeline Handle 10-50x More Tracking Events?

The pipeline (`backend/crates/pipeline/src/pipeline.rs`) receives events through a `broadcast::channel` with a 4096-event buffer (line 47 of `main.rs`). At current AIS rates (~387K/hour = ~107/second), this is comfortable. At global rates:

| Scenario | Events/second | Buffer fill time (4096) |
|----------|--------------|------------------------|
| Current | ~107/s | ~38 seconds |
| Global conservative | ~324/s | ~12.6 seconds |
| Global high | ~547/s | ~7.5 seconds |

**The broadcast channel buffer is fine** -- it only needs to absorb bursts while consumers process events. The consumers (pipeline, persistence tasks) are async and can keep up at these rates.

**However, the pipeline has different paths for tracking vs. intel events:**

1. **Tracking events (FlightPosition, VesselPosition):** Classified as `is_high_volume()` in `EventType`. They are:
   - NOT embedded (skipped by `compose_text()`)
   - NOT enriched by Haiku/Ollama
   - NOT published individually on SSE (absorbed into summary buckets)
   - Still inserted into the events table and latest_positions/position_history

2. **Intel events (news, conflict, etc.):** Get full enrichment + embedding + SSE publish

So the pipeline processing overhead per tracking event is minimal (just DB insert + position upsert). The concern is **database write throughput**.

#### Database Write Throughput

At global high rates: ~547 inserts/second to the events table + ~547 upserts to latest_positions + ~547 inserts to position_history = ~1,641 DB operations/second.

With the current NVMe (Samsung 990 PRO) and `synchronous_commit=off` in the docker-compose config, PostgreSQL can sustain 10,000+ inserts/second easily. This is not a bottleneck.

**Potential bottleneck: index maintenance.** Each insert updates 7+ indexes on the events table. At 547 inserts/second, this is ~3,800 index updates/second. Still manageable on NVMe, but worth monitoring:

```sql
-- Monitor insert latency
SELECT source_type,
       COUNT(*) / EXTRACT(EPOCH FROM (MAX(ingested_at) - MIN(ingested_at))) AS inserts_per_sec
FROM events
WHERE event_time > NOW() - INTERVAL '1 hour'
GROUP BY source_type;
```

#### The `is_interesting_vessel()` Filter

**File:** `/Users/dallas/git/osint/situationreport/backend/crates/server/src/routes/positions.rs`

This function filters which AIS vessels appear on the map. It currently passes through:
- All non-AIS sources (aircraft)
- Military MMSI vessels
- Ship types: military ops (35), law enforcement (55), SAR/pilot/tug (50-59), cargo (70-79), tankers (80-89)
- Fast movers (>15 knots)

With global tracking, `latest_positions` will grow from ~24K to potentially 100K-200K+ entities. The filter becomes more important because:
- More fishing boats, pleasure craft, and inland vessels will appear
- The map needs to avoid visual clutter

**Recommendation:** The current filter is well-designed. No changes needed for global tracking. The filter runs in application code after DB query, so performance is fine. If map performance degrades with very high entity counts, add server-side filtering in the SQL query for `query_latest_positions` (add a `WHERE source_type != 'ais' OR ship_type IN (...)` clause).

#### Position Deduplication Across Aviation Sources

Three ADS-B sources track the same aircraft. The current system inserts all three, creating 3x redundant position data.

**Recommendation for global tracking:** Implement application-level dedup before making the aviation sources global. Without dedup, global aviation could produce ~2.5M events/day. With dedup, ~700K-800K/day.

**Implementation location:** `backend/crates/sources/src/adsb.rs` or `backend/crates/sources/src/registry.rs`

Concept: shared `DashMap<String, Instant>` (icao_hex -> last insert time) checked before `persist_event()`. If the same aircraft was persisted within the last 30 seconds from any ADS-B source, skip the duplicate.

```rust
// Add to registry.rs or a new adsb_dedup.rs
use dashmap::DashMap;
use std::time::{Duration, Instant};

pub struct AdsbDedup {
    seen: DashMap<String, Instant>,
    window: Duration,
}

impl AdsbDedup {
    pub fn new(window: Duration) -> Self {
        Self { seen: DashMap::new(), window }
    }

    pub fn should_insert(&self, icao_hex: &str) -> bool {
        if let Some(last) = self.seen.get(icao_hex) {
            if last.elapsed() < self.window {
                return false;
            }
        }
        self.seen.insert(icao_hex.to_string(), Instant::now());
        true
    }

    /// Periodic cleanup of stale entries (run every ~5 minutes)
    pub fn cleanup(&self) {
        self.seen.retain(|_, v| v.elapsed() < self.window * 2);
    }
}
```

Share an `Arc<AdsbDedup>` across the three ADS-B source persistence tasks. Check `should_insert()` in the streaming persistence subscriber for ADS-B sources before calling `persist_event()`.

#### Should Tracking Data Bypass the Events Table?

**Current flow:** AIS/ADSB -> `persist_event()` (events table) + `upsert_position_if_needed()` (latest_positions + position_history).

**Should we skip the events table for tracking?**

Pros of skipping:
- Massive reduction in events table size (~95% fewer rows)
- No index maintenance overhead for tracking rows
- Compression/retention management becomes trivial

Cons of skipping:
- The correlation window (`CorrelationWindow` in `pipeline.rs`) reads from the broadcast channel, not the DB. It needs tracking events for correlation rules like `gps_military` and `maritime_enforcement`. These rules match AIS/ADSB events against other events.
- The situation graph uses event metadata from the events table for evidence refs.
- Historical queries on raw tracking data (rare but possible for investigations) would be lost.

**Recommendation:** Do NOT bypass the events table yet. Instead, use aggressive retention (7 days for tracking sources) combined with the partial GIN index to minimize the impact. The events table serves as the source of truth for the pipeline's correlation window and situation evidence. Bypass would require significant refactoring of the pipeline to use a separate event store.

A less disruptive optimization: skip inserting AIS events that are NOT interesting (using the same `is_interesting_vessel` logic at ingest time). This would reduce AIS inserts by ~70-80% while keeping all military, cargo, tanker, and fast-mover events. Implement this in the streaming persistence subscriber in `registry.rs`.

### 2.5 Retention Strategy for Global Tracking

#### Selective Retention Job

```sql
CREATE OR REPLACE FUNCTION selective_retention(job_id INT, config JSONB)
RETURNS VOID AS $$
DECLARE
    tracking_retention INTERVAL;
    rows_deleted BIGINT;
BEGIN
    tracking_retention := (config->>'tracking_retention')::interval;

    DELETE FROM events
    WHERE source_type IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky')
      AND event_time < NOW() - tracking_retention;

    GET DIAGNOSTICS rows_deleted = ROW_COUNT;
    RAISE NOTICE 'selective_retention: deleted % tracking rows older than %',
        rows_deleted, tracking_retention;
END;
$$ LANGUAGE plpgsql;

-- Run daily at 03:00 UTC
SELECT add_job(
    'selective_retention',
    '1 day',
    config => '{"tracking_retention": "7 days"}'::jsonb,
    initial_start => '2026-03-06 03:00:00+00'::timestamptz
);
```

#### Continuous Aggregates for Long-Term Analytics

Before deleting raw tracking data, ensure aggregates capture the historical patterns:

```sql
-- AIS vessel traffic aggregate
CREATE MATERIALIZED VIEW IF NOT EXISTS ais_traffic_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time) AS bucket,
    region_code,
    COUNT(*)                         AS position_count,
    COUNT(DISTINCT entity_id)        AS unique_vessels,
    AVG(CASE WHEN payload->>'speed' IS NOT NULL
         THEN (payload->>'speed')::float ELSE NULL END) AS avg_speed,
    COUNT(*) FILTER (WHERE tags @> ARRAY['military']) AS military_count
FROM events
WHERE source_type = 'ais'
GROUP BY bucket, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('ais_traffic_hourly',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists => true
);

SELECT add_retention_policy('ais_traffic_hourly', INTERVAL '5 years',
    if_not_exists => true);

-- Aviation traffic aggregate
CREATE MATERIALIZED VIEW IF NOT EXISTS aviation_traffic_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time) AS bucket,
    source_type,
    region_code,
    COUNT(*)                         AS position_count,
    COUNT(DISTINCT entity_id)        AS unique_aircraft,
    COUNT(*) FILTER (WHERE tags @> ARRAY['military']) AS military_count
FROM events
WHERE source_type IN ('airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky')
GROUP BY bucket, source_type, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('aviation_traffic_hourly',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists => true
);

SELECT add_retention_policy('aviation_traffic_hourly', INTERVAL '5 years',
    if_not_exists => true);
```

#### Complete Retention Summary

| Data Type | Table | Retention | Notes |
|-----------|-------|-----------|-------|
| AIS events | events | 7 days | Via `selective_retention` job |
| Aviation events | events | 7 days | Via `selective_retention` job |
| Intel events (news, conflict, cyber, etc.) | events | 180 days | Existing `drop_chunks` policy |
| Latest positions | latest_positions | Indefinite | Overwritten per entity (self-limiting) |
| Position history | position_history | 24 hours | Existing retention policy |
| AIS hourly aggregate | ais_traffic_hourly | 5 years | New continuous aggregate |
| Aviation hourly aggregate | aviation_traffic_hourly | 5 years | New continuous aggregate |
| Events hourly/daily | events_hourly, events_daily | 2y / 5y | Existing |
| Situations + entities | situations, entities | Indefinite | Low volume |
| Intel reports | intel_reports | Indefinite | Low volume |

#### Revised Steady-State With Global Tracking + 7-Day Tracking Retention

| Component | Size |
|-----------|------|
| Uncompressed events (2 days, all sources) | 68-108 GB |
| Compressed tracking events (days 2-7) | 4-8 GB |
| Compressed intel events (days 2-180) | ~10 GB |
| Indexes (on 2-day uncompressed window) | 30-50 GB |
| Position history (24h) | ~1-2 GB |
| Latest positions | ~200 MB (with global tracking, ~150K entities) |
| Continuous aggregates (5 years) | ~1 GB |
| WAL, temp, other | ~15-20 GB |
| **Total NVMe** | **~130-200 GB** |
| Cold storage (USB-C, chunks 14-180d of intel only) | ~8 GB |

This fits comfortably on the NVMe. The USB-C drive is mostly used for backups and could also hold extended intel retention if desired.

---

## Part 3: Implementation Order

### Phase 1: Storage Foundation (Do Now)

**Duration:** 1-2 hours
**Risk:** Low
**Prerequisites:** USB-C drive formatted and mounted

1. **Verify USB-C mount and permissions** (see 1.1)
2. **Update docker-compose.yml** with USB-C volume mount (see 1.2)
3. **Recreate postgres container** to pick up new volume
4. **Create cold_storage tablespace** (see 1.3)
5. **Reduce compression delay to 2 days** (see 1.4)
6. **Create the partial GIN index** (see 1.6)
7. **Create the tiering job** (see 1.5)
8. **Verify:** Check that old chunks get compressed, that the tiering job moves chunks to cold_storage

### Phase 2: Global AIS (After Phase 1 Stable, ~1 Day Later)

**Duration:** 30 minutes code change, then monitoring
**Risk:** Medium (volume increase is the main risk)
**Prerequisites:** Phase 1 complete and verified

1. **Create continuous aggregates** (see 2.5) -- must exist BEFORE enabling the selective retention job
2. **Create the selective retention job** (see 2.5) -- start cleaning old tracking data
3. **Modify `ais.rs`** -- replace BOUNDING_BOXES with global box (see 2.1)
4. **Expand NAMED_REGIONS** for Western Hemisphere and Africa (see 2.1)
5. **Update tests** in ais.rs
6. **Build and deploy** (`cargo build --release`, redeploy container)
7. **Monitor for 24 hours:**
   - Watch AIS message rate: `SELECT COUNT(*), COUNT(*)/3600.0 AS per_sec FROM events WHERE source_type = 'ais' AND event_time > NOW() - INTERVAL '1 hour';`
   - Watch DB size growth: `SELECT pg_size_pretty(pg_database_size('situationreport'));`
   - Watch uncompressed chunk size: `SELECT chunk_name, pg_size_pretty(total_bytes) FROM chunks_detailed_size('events') WHERE NOT is_compressed ORDER BY total_bytes DESC;`
   - Watch broadcast channel lag: check for "lagged" warnings in app logs
8. **If volume exceeds expectations:** The AIS source can be quickly reverted by changing the bounding box back. Or throttle by filtering at ingest time (e.g., only persist military + large vessels globally, keep full tracking only in the original 4 regions).

### Phase 3: Global Aviation + Dedup (After 4 TB Drive Arrives)

**Duration:** 2-4 hours
**Risk:** Medium
**Prerequisites:** Phase 2 stable, 4 TB drive available

1. **Migrate cold tablespace to 4 TB drive:**
   ```bash
   # 1. Mount 4TB drive at /mnt/sitrep-cold-4tb (or replace /mnt/sitrep-cold)
   # 2. Stop tiering job temporarily
   # 3. Copy existing cold data
   rsync -avP /mnt/sitrep-cold/pg-cold/ /mnt/sitrep-cold-4tb/pg-cold/
   # 4. Update docker-compose.yml volume mount
   # 5. Recreate postgres container
   # 6. Drop and recreate tablespace pointing to new mount
   #    (or just update the mount point if using the same path)
   # 7. Re-enable tiering job
   ```

2. **Implement ADS-B dedup** (see 2.4) -- add `AdsbDedup` to the codebase
3. **Expand `POINT_QUERIES` in `adsb.rs`** to 22 global regions (see 2.2)
4. **Optionally expand OpenSky boxes** (see 2.2) -- only if credit budget permits
5. **Build and deploy**
6. **Monitor aviation volume** for 24 hours

### Phase 4: Backup Automation (After Phase 1)

**Duration:** 1 hour
**Risk:** Low
**Can run in parallel with Phase 2/3**

1. **Update docker-compose.yml** with SATA drive backup mounts (see 1.7)
2. **Add WAL archiving** to postgres command (see 1.7)
3. **Create backup scripts** on the host (see 1.7)
4. **Set up cron jobs:**
   ```bash
   # Weekly pg_dump (Sunday 2am)
   0 2 * * 0 /opt/scripts/backup-situationreport.sh >> /var/log/sitrep-backup.log 2>&1

   # Daily WAL cleanup
   0 5 * * * /opt/scripts/clean-wal-archive.sh >> /var/log/sitrep-wal-cleanup.log 2>&1
   ```
5. **Test restore** from a pg_dump backup to verify the process works

---

## Appendix A: File Change Summary

| File | Change | Phase |
|------|--------|-------|
| `docker-compose.yml` | Add volume mounts for USB-C and SATA drives; add WAL archiving flags | 1, 4 |
| `backend/crates/sources/src/ais.rs` | Replace `BOUNDING_BOXES` with global box; expand `NAMED_REGIONS`; update tests | 2 |
| `backend/crates/sources/src/adsb.rs` | Expand `POINT_QUERIES` to 22 global regions | 3 |
| `backend/crates/sources/src/opensky.rs` | Optionally expand `BOUNDING_BOXES` to larger regions (stay within 4 boxes) | 3 |
| `backend/crates/sources/src/registry.rs` | Add ADS-B dedup check in streaming persistence path | 3 |
| SQL (via psql) | Compression policy, tiering job, retention job, continuous aggregates, partial index, tablespace | 1, 2 |
| Host scripts | Backup and WAL cleanup cron scripts | 4 |

## Appendix B: Monitoring Queries

```sql
-- DB size
SELECT pg_size_pretty(pg_database_size('situationreport'));

-- Events by source today
SELECT source_type, COUNT(*), pg_size_pretty(SUM(pg_column_size(t.*))::bigint) AS estimated_size
FROM events t
WHERE event_time > CURRENT_DATE
GROUP BY source_type
ORDER BY COUNT(*) DESC;

-- Chunk compression status
SELECT chunk_name, range_start, range_end, is_compressed,
       pg_size_pretty(before_compression_total_bytes) AS before,
       pg_size_pretty(after_compression_total_bytes) AS after
FROM timescaledb_information.chunks
WHERE hypertable_name = 'events'
ORDER BY range_start DESC
LIMIT 20;

-- Tablespace usage
SELECT t.spcname, pg_size_pretty(pg_tablespace_size(t.oid)) AS size
FROM pg_tablespace t;

-- Events ingestion rate (last hour)
SELECT source_type,
       COUNT(*) AS total,
       COUNT(*) / 3600.0 AS per_second
FROM events
WHERE event_time > NOW() - INTERVAL '1 hour'
GROUP BY source_type
ORDER BY total DESC;

-- Latest positions count
SELECT source_type, COUNT(*)
FROM latest_positions
GROUP BY source_type;

-- Background jobs status
SELECT job_id, proc_name, schedule_interval,
       last_run_started_at, last_run_status, total_runs, total_failures
FROM timescaledb_information.jobs
ORDER BY job_id;

-- Broadcast channel lag (check application logs for)
-- "Stream persistence task lagged, skipping messages"
```

## Appendix C: Rollback Plan

If global tracking causes problems:

1. **Immediate (seconds):** Disable AIS via source_config:
   ```sql
   UPDATE source_config SET enabled = false WHERE source_id = 'ais';
   ```
   The registry checks `enabled` before each stream reconnect.

2. **Quick (minutes):** Revert `ais.rs` to 4-box BOUNDING_BOXES, rebuild, redeploy.

3. **Data cleanup:** If too much data accumulated:
   ```sql
   -- Delete excess AIS data beyond retention target
   DELETE FROM events
   WHERE source_type = 'ais'
     AND event_time < NOW() - INTERVAL '2 days';
   -- Recompress affected chunks
   SELECT compress_chunk(c, if_not_compressed => true)
   FROM show_chunks('events', older_than => INTERVAL '2 days') c;
   ```

4. **Tablespace rollback:** If the USB-C drive causes issues, chunks on cold_storage can be moved back:
   ```sql
   SELECT move_chunk(
       chunk => '<chunk_name>'::regclass,
       destination_tablespace => 'pg_default',
       index_destination_tablespace => 'pg_default'
   );
   ```
