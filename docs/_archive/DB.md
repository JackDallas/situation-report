# sitrep.watch — Database Architecture & Technical Reference

> **Stack:** PostgreSQL 17 + TimescaleDB + PostGIS  
> **Purpose:** Conflict intelligence platform aggregating OSINT data into a live geospatial + temporal dashboard  
> **Hosting:** Self-hosted on Hetzner VPS (CX43: 8 vCPU, 16 GB RAM, 160 GB NVMe — €9.49/month)

---

## Overview

sitrep.watch ingests data from 10+ heterogeneous OSINT sources (ADS-B aircraft tracking, AIS ship tracking, NASA FIRMS fire data, GPS jamming maps, NOTAMs, Telegram channels, internet outage monitoring, satellite imagery metadata, nighttime lights, nuclear/radiation monitoring, economic indicators) and presents them on a unified live dashboard with geospatial and temporal filtering.

PostgreSQL + TimescaleDB + PostGIS was chosen because:

- **PostGIS** provides the most mature open-source geospatial engine (20+ years, full OGC compliance, GIST R-tree spatial indexes, spatial joins, clustering, GeoJSON support)
- **TimescaleDB** adds time-series superpowers: automatic time partitioning (hypertables), continuous aggregates, columnar compression, and data retention policies
- Combined, they handle the core query pattern: *"show me all military aircraft, ship movements, and fire detections within 100km of Kherson in the last 6 hours"* — geo + time filtering in a single query
- Runs comfortably on a single 16 GB VPS with no external dependencies
- Standard PostgreSQL ecosystem: every ORM, driver, visualisation tool, and backup utility works out of the box

---

## Docker Setup

Use the official TimescaleDB HA image which bundles PostgreSQL 17 + TimescaleDB + PostGIS:

```yaml
# docker-compose.yml
version: "3.8"

services:
  db:
    image: timescale/timescaledb-ha:pg17
    container_name: sitrep-db
    restart: unless-stopped
    environment:
      POSTGRES_DB: sitrep
      POSTGRES_USER: sitrep
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    ports:
      - "127.0.0.1:5432:5432"
    volumes:
      - pgdata:/home/postgres/pgdata/data
      - ./init.sql:/docker-entrypoint-initdb.d/001-init.sql
    shm_size: "256mb"
    command:
      - "postgres"
      - "-c" 
      - "shared_buffers=4GB"
      - "-c"
      - "effective_cache_size=8GB"
      - "-c"
      - "work_mem=64MB"
      - "-c"
      - "maintenance_work_mem=512MB"
      - "-c"
      - "max_connections=100"
      - "-c"
      - "random_page_cost=1.1"
      - "-c"
      - "wal_level=replica"

  redis:
    image: redis:7-alpine
    container_name: sitrep-redis
    restart: unless-stopped
    ports:
      - "127.0.0.1:6379:6379"
    volumes:
      - redisdata:/data

volumes:
  pgdata:
  redisdata:
```

After first boot, run `timescaledb-tune` inside the container to auto-optimise PostgreSQL settings:

```bash
docker exec -it sitrep-db timescaledb-tune --yes
docker restart sitrep-db
```

---

## Database Initialisation

```sql
-- init.sql — run once on database creation

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS timescaledb;
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pg_trgm;      -- fuzzy text search
CREATE EXTENSION IF NOT EXISTS btree_gist;   -- GiST indexes on scalar types

-- Optional: H3 hexagonal indexing for heatmaps
-- Requires: https://github.com/zachasme/h3-pg
-- CREATE EXTENSION IF NOT EXISTS h3;
-- CREATE EXTENSION IF NOT EXISTS h3_postgis;
```

---

## Schema

### Core Events Table

A single unified events hypertable with fixed columns for universally-queryable fields and JSONB for source-specific payloads. This means **no schema migration when adding new data sources** — just write a new collector that outputs the common fields plus source-specific JSON.

```sql
CREATE TABLE events (
    -- Temporal
    event_time      TIMESTAMPTZ     NOT NULL,
    ingested_at     TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    
    -- Source identification
    source_type     TEXT            NOT NULL,   -- 'adsb', 'ais', 'firms', 'telegram', 'notam', 'gpsjam', 'ioda', 'viirs', etc.
    source_id       TEXT,                       -- dedup key from upstream source
    
    -- Geospatial
    location        GEOGRAPHY(POINT, 4326),     -- for point events (lat/lon)
    geometry        GEOGRAPHY,                  -- for area events (NOTAM zones, conflict polygons)
    region_code     TEXT,                       -- ISO 3166-1/2 code for region-level filtering
    
    -- Entity tracking
    entity_id       TEXT,                       -- aircraft ICAO hex, ship MMSI, Telegram channel ID, etc.
    entity_name     TEXT,                       -- human-readable name if known
    
    -- Classification
    event_type      TEXT,                       -- 'position', 'fire', 'outage', 'message', 'airspace_closure', etc.
    severity        TEXT            DEFAULT 'low',  -- 'low', 'medium', 'high', 'critical'
    confidence      REAL            DEFAULT 1.0,    -- 0.0–1.0 normalised confidence score
    tags            TEXT[],                     -- flexible categorisation: ['military', 'naval', 'ukraine', etc.]
    
    -- Content
    title           TEXT,                       -- human-readable one-line summary
    description     TEXT,                       -- longer description if available
    
    -- Source-specific payload (all raw/extra data goes here)
    payload         JSONB           NOT NULL DEFAULT '{}'::jsonb
);

-- Convert to TimescaleDB hypertable with 1-day chunks
SELECT create_hypertable('events', 'event_time',
    chunk_time_interval => INTERVAL '1 day'
);
```

### Indexes

```sql
-- Primary geospatial index (GIST on geography column)
CREATE INDEX idx_events_location ON events USING GIST (location);

-- Compound time + source filtering (most common dashboard query pattern)
CREATE INDEX idx_events_source_time ON events (source_type, event_time DESC);

-- Entity tracking (find all positions for a specific aircraft/ship)
CREATE INDEX idx_events_entity_time ON events (entity_id, event_time DESC)
    WHERE entity_id IS NOT NULL;

-- Region-level filtering
CREATE INDEX idx_events_region_time ON events (region_code, event_time DESC)
    WHERE region_code IS NOT NULL;

-- Severity filtering for alerts
CREATE INDEX idx_events_severity ON events (severity, event_time DESC)
    WHERE severity IN ('high', 'critical');

-- JSONB payload queries (GIN index)
CREATE INDEX idx_events_payload ON events USING GIN (payload);

-- Tags array queries (GIN index)
CREATE INDEX idx_events_tags ON events USING GIN (tags);

-- Deduplication index
CREATE UNIQUE INDEX idx_events_dedup ON events (source_type, source_id, event_time)
    WHERE source_id IS NOT NULL;
```

### Optional: H3 Hexagonal Index Column

If `h3-pg` is installed, add a generated column for hex-based heatmaps and spatial aggregation:

```sql
-- Add H3 index at resolution 5 (~252 km² hexagons — good for regional overview)
ALTER TABLE events ADD COLUMN h3_r5 h3index
    GENERATED ALWAYS AS (
        CASE WHEN location IS NOT NULL
            THEN h3_lat_lng_to_cell(
                ST_Y(location::geometry)::float,
                ST_X(location::geometry)::float,
                5
            )
        END
    ) STORED;

CREATE INDEX idx_events_h3 ON events (h3_r5, event_time DESC)
    WHERE h3_r5 IS NOT NULL;

-- Resolution guide:
-- res 3: ~12,393 km² — continental overview
-- res 5: ~252 km²   — regional (good default for conflict zones)
-- res 7: ~5.16 km²  — city-level
-- res 9: ~0.105 km² — neighbourhood-level
```

### Latest Positions Table

For entities that emit continuous position updates (aircraft, ships), maintain a "latest known position" table for efficient map rendering:

```sql
CREATE TABLE latest_positions (
    entity_id       TEXT            PRIMARY KEY,
    source_type     TEXT            NOT NULL,
    entity_name     TEXT,
    location        GEOGRAPHY(POINT, 4326) NOT NULL,
    heading         REAL,
    speed           REAL,
    altitude        REAL,
    last_seen       TIMESTAMPTZ     NOT NULL,
    payload         JSONB           NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX idx_latest_pos_location ON latest_positions USING GIST (location);
CREATE INDEX idx_latest_pos_source ON latest_positions (source_type, last_seen DESC);
```

Update via upsert on each position report:

```sql
INSERT INTO latest_positions (entity_id, source_type, entity_name, location, heading, speed, altitude, last_seen, payload)
VALUES ($1, $2, $3, ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography, $6, $7, $8, $9, $10)
ON CONFLICT (entity_id) DO UPDATE SET
    location = EXCLUDED.location,
    heading = EXCLUDED.heading,
    speed = EXCLUDED.speed,
    altitude = EXCLUDED.altitude,
    last_seen = EXCLUDED.last_seen,
    entity_name = COALESCE(EXCLUDED.entity_name, latest_positions.entity_name),
    payload = latest_positions.payload || EXCLUDED.payload;
```

### Conflict Zones & Regions of Interest

Static or semi-static polygon geometries for conflict zones, exclusion areas, borders:

```sql
CREATE TABLE zones (
    id              SERIAL PRIMARY KEY,
    name            TEXT            NOT NULL,
    zone_type       TEXT            NOT NULL,   -- 'conflict_zone', 'exclusion', 'notam', 'fir', 'border'
    geometry        GEOGRAPHY       NOT NULL,
    properties      JSONB           DEFAULT '{}'::jsonb,
    valid_from      TIMESTAMPTZ,
    valid_until     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ     DEFAULT NOW()
);

CREATE INDEX idx_zones_geometry ON zones USING GIST (geometry);
CREATE INDEX idx_zones_type ON zones (zone_type);
```

---

## Continuous Aggregates

Pre-compute dashboard rollups so panels load in milliseconds. TimescaleDB continuous aggregates are incrementally refreshed — only new data is recomputed.

### Hourly Event Counts by Source and Region

```sql
CREATE MATERIALIZED VIEW events_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)   AS bucket,
    source_type,
    region_code,
    severity,
    COUNT(*)                            AS event_count,
    COUNT(DISTINCT entity_id)           AS unique_entities
FROM events
GROUP BY bucket, source_type, region_code, severity
WITH NO DATA;

-- Refresh policy: update every 30 minutes, look back 2 hours for late data
SELECT add_continuous_aggregate_policy('events_hourly',
    start_offset    => INTERVAL '2 hours',
    end_offset      => INTERVAL '30 minutes',
    schedule_interval => INTERVAL '30 minutes'
);
```

### Daily Summary

```sql
CREATE MATERIALIZED VIEW events_daily
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 day', event_time)    AS bucket,
    source_type,
    region_code,
    COUNT(*)                            AS event_count,
    COUNT(DISTINCT entity_id)           AS unique_entities,
    AVG(confidence)                     AS avg_confidence
FROM events
GROUP BY bucket, source_type, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('events_daily',
    start_offset    => INTERVAL '3 days',
    end_offset      => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);
```

### Anomaly Detection Baseline

Compute rolling statistics per (source_type, region, day-of-week) for z-score anomaly detection using Welford's online algorithm approach:

```sql
CREATE MATERIALIZED VIEW anomaly_baseline
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)                       AS bucket,
    source_type,
    region_code,
    EXTRACT(DOW FROM event_time)::int                       AS day_of_week,
    COUNT(*)                                                AS event_count
FROM events
GROUP BY bucket, source_type, region_code, EXTRACT(DOW FROM event_time)::int
WITH NO DATA;

SELECT add_continuous_aggregate_policy('anomaly_baseline',
    start_offset    => INTERVAL '7 days',
    end_offset      => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);
```

Anomaly detection query (z-score > 2.0 = significant):

```sql
WITH baseline AS (
    SELECT
        source_type,
        region_code,
        day_of_week,
        AVG(event_count)    AS mean_count,
        STDDEV(event_count) AS stddev_count
    FROM anomaly_baseline
    WHERE bucket > NOW() - INTERVAL '90 days'
    GROUP BY source_type, region_code, day_of_week
),
current AS (
    SELECT
        source_type,
        region_code,
        EXTRACT(DOW FROM bucket)::int AS day_of_week,
        event_count
    FROM anomaly_baseline
    WHERE bucket >= date_trunc('hour', NOW()) - INTERVAL '1 hour'
)
SELECT
    c.source_type,
    c.region_code,
    c.event_count,
    b.mean_count,
    CASE WHEN b.stddev_count > 0
        THEN (c.event_count - b.mean_count) / b.stddev_count
        ELSE 0
    END AS z_score
FROM current c
JOIN baseline b USING (source_type, region_code, day_of_week)
WHERE b.stddev_count > 0
  AND (c.event_count - b.mean_count) / b.stddev_count > 2.0
ORDER BY z_score DESC;
```

---

## Compression & Data Retention

TimescaleDB columnar compression achieves 90–98% compression on numeric/timestamp data. Important caveat: **compressed chunks lose GIST spatial indexes** — only `segmentby` and `orderby` columns are used for filtering on compressed data.

Strategy: keep recent data uncompressed for spatial queries, compress older data for time-range queries and storage savings.

```sql
-- Compress chunks older than 7 days
-- segmentby: columns used for equality filtering on compressed data
-- orderby: sort order within compressed segments
ALTER TABLE events SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'source_type, region_code',
    timescaledb.compress_orderby = 'event_time DESC'
);

SELECT add_compression_policy('events', INTERVAL '7 days');

-- Drop raw data older than 180 days (continuous aggregates retain summaries)
SELECT add_retention_policy('events', INTERVAL '180 days');

-- Keep continuous aggregates longer
SELECT add_retention_policy('events_hourly', INTERVAL '2 years');
SELECT add_retention_policy('events_daily', INTERVAL '5 years');
```

**Query implications:**

- Last 7 days: full spatial + temporal queries (GIST index active)
- 7–180 days: temporal queries efficient (segmentby filtering), spatial queries require sequential scan within matched segments — still fast for filtered time ranges
- 180+ days: only continuous aggregate data available

---

## Common Query Patterns

### 1. Events within radius + time window (core dashboard query)

```sql
SELECT
    event_time, source_type, entity_id, entity_name,
    title, severity, confidence,
    ST_AsGeoJSON(location)::json AS geojson,
    payload
FROM events
WHERE event_time > NOW() - INTERVAL '6 hours'
  AND ST_DWithin(
      location,
      ST_SetSRID(ST_MakePoint(32.6, 46.6), 4326)::geography,  -- Kherson
      100000  -- 100km radius in metres
  )
ORDER BY event_time DESC;
```

### 2. Events within a conflict zone polygon

```sql
SELECT e.*
FROM events e
JOIN zones z ON ST_Within(e.location::geometry, z.geometry::geometry)
WHERE z.name = 'Donbas Front Line'
  AND e.event_time > NOW() - INTERVAL '24 hours'
ORDER BY e.event_time DESC;
```

### 3. All current military aircraft in a bounding box

```sql
SELECT *
FROM latest_positions
WHERE source_type = 'adsb'
  AND ST_Within(
      location::geometry,
      ST_MakeEnvelope(22.0, 44.0, 40.0, 52.0, 4326)  -- Black Sea region bbox
  )
  AND last_seen > NOW() - INTERVAL '5 minutes'
  AND payload->>'is_military' = 'true';
```

### 4. Spatial clustering of fire detections

```sql
SELECT
    cluster_id,
    COUNT(*) AS fire_count,
    ST_AsGeoJSON(ST_Centroid(ST_Collect(location::geometry)))::json AS centroid,
    MIN(event_time) AS first_detected,
    MAX(event_time) AS last_detected,
    AVG(confidence) AS avg_confidence
FROM (
    SELECT *,
        ST_ClusterDBSCAN(location::geometry, eps := 0.05, minpoints := 3)
            OVER () AS cluster_id
    FROM events
    WHERE source_type = 'firms'
      AND event_time > NOW() - INTERVAL '24 hours'
) clustered
WHERE cluster_id IS NOT NULL
GROUP BY cluster_id
ORDER BY fire_count DESC;
```

### 5. Track history for a specific entity

```sql
SELECT
    event_time,
    ST_Y(location::geometry) AS lat,
    ST_X(location::geometry) AS lon,
    payload->>'altitude' AS altitude,
    payload->>'speed' AS speed,
    payload->>'heading' AS heading
FROM events
WHERE entity_id = 'AE1234'  -- ICAO hex
  AND source_type = 'adsb'
  AND event_time > NOW() - INTERVAL '24 hours'
ORDER BY event_time ASC;
```

### 6. H3 hexagonal heatmap (requires h3-pg)

```sql
SELECT
    h3_cell_to_lat_lng(h3_r5) AS center,
    h3_r5,
    COUNT(*) AS event_count,
    array_agg(DISTINCT source_type) AS source_types
FROM events
WHERE event_time > NOW() - INTERVAL '7 days'
  AND h3_r5 IS NOT NULL
GROUP BY h3_r5
ORDER BY event_count DESC
LIMIT 500;
```

### 7. Convergence detection (multiple signal types spiking in same region)

```sql
WITH hourly AS (
    SELECT
        region_code,
        source_type,
        COUNT(*) AS cnt
    FROM events
    WHERE event_time > NOW() - INTERVAL '1 hour'
      AND region_code IS NOT NULL
    GROUP BY region_code, source_type
),
multi_source AS (
    SELECT
        region_code,
        COUNT(DISTINCT source_type) AS active_sources,
        SUM(cnt) AS total_events,
        array_agg(source_type || ':' || cnt ORDER BY cnt DESC) AS breakdown
    FROM hourly
    GROUP BY region_code
    HAVING COUNT(DISTINCT source_type) >= 3  -- 3+ different source types
)
SELECT * FROM multi_source
ORDER BY active_sources DESC, total_events DESC;
```

---

## Real-Time Live Feed

### PostgreSQL LISTEN/NOTIFY

Use a trigger to push new events to connected clients via PostgreSQL's built-in pub/sub:

```sql
CREATE OR REPLACE FUNCTION notify_new_event()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('new_event', json_build_object(
        'event_time', NEW.event_time,
        'source_type', NEW.source_type,
        'entity_id', NEW.entity_id,
        'severity', NEW.severity,
        'title', NEW.title,
        'lat', ST_Y(NEW.location::geometry),
        'lon', ST_X(NEW.location::geometry)
    )::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_notify_event
    AFTER INSERT ON events
    FOR EACH ROW
    WHEN (NEW.severity IN ('high', 'critical'))  -- only notify for significant events
    EXECUTE FUNCTION notify_new_event();
```

### Application Server: LISTEN → SSE

Server-Sent Events are simpler than WebSockets and provide automatic reconnection. The flow:

1. PostgreSQL trigger fires `pg_notify('new_event', ...)` on INSERT
2. Application server listens on that channel with `LISTEN new_event`
3. SSE endpoint streams events to connected browser clients

Python example (FastAPI + asyncpg):

```python
import asyncio
import json
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
import asyncpg

app = FastAPI()

async def event_stream(pool: asyncpg.Pool):
    conn = await pool.acquire()
    try:
        await conn.add_listener('new_event', lambda *args: None)
        queue = asyncio.Queue()
        
        def callback(conn, pid, channel, payload):
            queue.put_nowait(payload)
        
        await conn.add_listener('new_event', callback)
        
        while True:
            payload = await queue.get()
            yield f"data: {payload}\n\n"
    finally:
        await pool.release(conn)

@app.get("/api/stream")
async def stream_events():
    pool = app.state.db_pool  # set up on startup
    return StreamingResponse(
        event_stream(pool),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
        }
    )
```

Browser client:

```javascript
const source = new EventSource('/api/stream');
source.onmessage = (event) => {
    const data = JSON.parse(event.data);
    addEventToMap(data);
    addEventToFeed(data);
};
```

---

## Ingestion Architecture

### Collector Pattern

Each data source gets its own Python collector script. Collectors share a common interface:

```python
# collectors/base.py
from abc import ABC, abstractmethod
import asyncpg
from datetime import datetime

class BaseCollector(ABC):
    def __init__(self, pool: asyncpg.Pool):
        self.pool = pool
        self.source_type = self.__class__.SOURCE_TYPE
    
    @abstractmethod
    async def collect(self) -> list[dict]:
        """Fetch data from source. Return list of normalised event dicts."""
        ...
    
    async def ingest(self, events: list[dict]):
        """Batch insert normalised events."""
        async with self.pool.acquire() as conn:
            await conn.executemany('''
                INSERT INTO events (
                    event_time, source_type, source_id, location,
                    region_code, entity_id, entity_name,
                    event_type, severity, confidence,
                    tags, title, description, payload
                ) VALUES (
                    $1, $2, $3,
                    ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography,
                    $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
                )
                ON CONFLICT (source_type, source_id, event_time)
                DO NOTHING
            ''', [self._to_row(e) for e in events])
    
    def _to_row(self, event: dict) -> tuple:
        return (
            event['event_time'],
            self.source_type,
            event.get('source_id'),
            event.get('lon'),
            event.get('lat'),
            event.get('region_code'),
            event.get('entity_id'),
            event.get('entity_name'),
            event.get('event_type', 'unknown'),
            event.get('severity', 'low'),
            event.get('confidence', 1.0),
            event.get('tags', []),
            event.get('title'),
            event.get('description'),
            json.dumps(event.get('payload', {})),
        )
    
    async def run(self):
        events = await self.collect()
        if events:
            await self.ingest(events)
        return len(events)
```

### Collector Schedule

| Source | Collector | Interval | Typical volume |
|--------|-----------|----------|----------------|
| ADS-B (OpenSky) | `collectors/adsb.py` | 10–15 sec | ~1000 events/min (filtered military) |
| AIS ships | `collectors/ais.py` | 1–5 min | ~200 events/min |
| NASA FIRMS fires | `collectors/firms.py` | 10 min | ~50–500/batch |
| Telegram channels | `collectors/telegram.py` | Persistent (Telethon) | Variable |
| NOTAMs | `collectors/notams.py` | 1 hour | ~10–50/batch |
| GPSJam | `collectors/gpsjam.py` | 1 hour | ~20–100/batch |
| IODA internet outages | `collectors/ioda.py` | 5 min | ~5–20/batch |
| VIIRS nighttime lights | `collectors/viirs.py` | Daily | ~10–50/batch |
| Cloudflare Radar | `collectors/cloudflare.py` | 15 min | ~10–30/batch |
| Economic indicators | `collectors/economic.py` | Daily | ~5–20/batch |

Use APScheduler or systemd timers. No message queue needed at this scale — direct database inserts handle thousands of rows/sec.

### When to add Redis Streams

Add Redis Streams as an ingestion buffer when:

- Multiple consumers need the same raw data (e.g. both the DB writer and a real-time anomaly detector)
- A collector failure shouldn't block other collectors
- You need replay capability for reprocessing
- Community contributors are running remote collectors that need a central ingest point

```
Collectors → Redis Streams → Workers → PostgreSQL
```

This is a **growth-phase** addition, not needed for the prototype.

---

## Visualisation

### Map: MapLibre GL JS or deck.gl

- **MapLibre GL JS** — open-source Mapbox GL fork, excellent for interactive maps with GeoJSON layers
- **deck.gl** — WebGL-powered, better for large point clouds (100k+ markers), used by World Monitor

Both consume GeoJSON from the API. Serve GeoJSON from PostGIS:

```sql
SELECT json_build_object(
    'type', 'FeatureCollection',
    'features', COALESCE(json_agg(ST_AsGeoJSON(e.*)::json), '[]'::json)
) AS geojson
FROM (
    SELECT location, source_type, entity_id, title, severity, event_time
    FROM events
    WHERE event_time > NOW() - INTERVAL '1 hour'
      AND location IS NOT NULL
) e;
```

### Time-Series Panels: Grafana

Grafana has a native PostgreSQL/TimescaleDB datasource. Point it at the continuous aggregates for instant dashboard panels. Example query for a Grafana time-series panel:

```sql
SELECT
    bucket AS time,
    source_type AS metric,
    event_count AS value
FROM events_hourly
WHERE $__timeFilter(bucket)
  AND region_code = '$region'
ORDER BY bucket;
```

### Ad-Hoc Exploration: Apache Superset

Superset connects to PostgreSQL and provides SQL Lab + deck.gl map visualisations. Good for investigative queries.

---

## Backup & Maintenance

### Daily Backups

```bash
#!/bin/bash
# backup.sh — run via cron daily at 03:00 UTC
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backups"

# Dump with compression
docker exec sitrep-db pg_dump -U sitrep -Fc sitrep > "${BACKUP_DIR}/sitrep_${TIMESTAMP}.dump"

# Upload to object storage (Hetzner Storage Box or Backblaze B2)
rclone copy "${BACKUP_DIR}/sitrep_${TIMESTAMP}.dump" remote:sitrep-backups/

# Keep local backups for 7 days
find "${BACKUP_DIR}" -name "*.dump" -mtime +7 -delete
```

### Restore

```bash
docker exec -i sitrep-db pg_restore -U sitrep -d sitrep --clean < sitrep_20260301.dump
```

### Monitoring Queries

```sql
-- Check hypertable size and compression ratio
SELECT * FROM hypertable_detailed_size('events');

-- Check chunk status (compressed vs uncompressed)
SELECT
    chunk_name,
    is_compressed,
    pg_size_pretty(before_compression_total_bytes) AS before,
    pg_size_pretty(after_compression_total_bytes) AS after
FROM chunk_compression_stats('events')
ORDER BY chunk_name DESC
LIMIT 20;

-- Check continuous aggregate refresh status
SELECT * FROM timescaledb_information.continuous_aggregate_stats;

-- Estimated total rows
SELECT reltuples::bigint AS estimated_rows
FROM pg_class WHERE relname = 'events';

-- Recent ingestion rate
SELECT
    source_type,
    COUNT(*) AS events_last_hour,
    MAX(ingested_at) AS last_ingested
FROM events
WHERE ingested_at > NOW() - INTERVAL '1 hour'
GROUP BY source_type
ORDER BY events_last_hour DESC;
```

---

## Resource Budget (16 GB VPS)

| Component | Memory Allocation |
|-----------|-------------------|
| PostgreSQL shared_buffers | 4 GB |
| OS page cache (effective_cache_size) | 8 GB |
| Python collector workers | 1–2 GB |
| Web application server | 512 MB |
| Redis | 256 MB |
| System overhead | 256 MB |

### Storage Growth Estimates

Filtering for militarily interesting events only (not all global ADS-B/AIS traffic):

- Raw ingest: **50–200 MB/day**
- With TimescaleDB compression (after 7 days): **~10–40 MB/day effective**
- Monthly storage: **~300 MB – 1.5 GB compressed**
- 160 GB NVMe lasts **years** at this rate
- Retention policy drops raw data after 180 days; aggregates kept indefinitely

---

## Key Technical Notes

1. **Geography vs Geometry**: Use `GEOGRAPHY` type for storage (accurate distance calculations on the WGS-84 ellipsoid in metres). Cast to `::geometry` only when needed for functions that require it (e.g. `ST_ClusterDBSCAN`).

2. **Compression + spatial queries**: Compressed chunks lose GIST indexes. Design your queries so spatial filtering always includes a time constraint that hits uncompressed chunks (last 7 days). Historical spatial queries work but are slower.

3. **JSONB indexing**: The GIN index on `payload` supports `@>` (contains), `?` (key exists), and `?|` / `?&` (any/all keys exist). For frequently queried payload fields, consider adding expression indexes: `CREATE INDEX idx_events_military ON events ((payload->>'is_military')) WHERE payload->>'is_military' = 'true';`

4. **Deduplication**: The unique index on `(source_type, source_id, event_time)` with `ON CONFLICT DO NOTHING` handles duplicate data from sources that don't guarantee exactly-once delivery.

5. **Connection pooling**: For production, add PgBouncer in front of PostgreSQL to handle connection multiplexing. At hobby scale with <100 connections, direct connections are fine.

6. **SRID 4326**: All coordinates use WGS-84 (EPSG:4326). Longitude first in `ST_MakePoint(lon, lat)` — this catches many people out.
