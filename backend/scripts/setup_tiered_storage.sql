-- =============================================================================
-- Situation Report: Tiered Storage Setup
-- =============================================================================
-- Run this script AFTER restarting the postgres container with the new
-- docker-compose.yml volume mounts.
--
-- Prerequisites:
--   1. USB-C drive mounted at /var/mnt/sitrep-cold on the host
--   2. Directory /var/mnt/sitrep-cold/pg-cold exists and is owned by UID 1000
--   3. SATA drive mounted at /run/media/system/Storage on the host
--   4. Backup directories created (see setup_backups.sh)
--   5. Postgres container restarted with new volume mounts
--
-- Usage:
--   docker exec -i situationreport-postgres-1 \
--     psql -U sitrep -d situationreport -f /dev/stdin \
--     < backend/scripts/setup_tiered_storage.sql
--
-- Or connect via psql and paste sections one at a time.
-- =============================================================================

\echo '=== Step 1: Create cold_storage tablespace on USB-C drive ==='

-- The /mnt/cold directory is bind-mounted from /var/mnt/sitrep-cold/pg-cold
-- and must be owned by postgres (UID 1000) inside the container.
CREATE TABLESPACE cold_storage LOCATION '/mnt/cold';

-- Verify
SELECT spcname, pg_tablespace_location(oid)
FROM pg_tablespace
WHERE spcname = 'cold_storage';

\echo '=== Step 2: Reduce compression delay from 7 days to 2 days ==='

-- Show current compression policy
SELECT j.job_id, j.schedule_interval, j.config
FROM timescaledb_information.jobs j
WHERE j.proc_name = 'policy_compression'
  AND j.hypertable_name = 'events';

-- Replace the compression policy
SELECT remove_compression_policy('events');
SELECT add_compression_policy('events', INTERVAL '2 days');

-- Compress existing uncompressed chunks older than 2 days
SELECT compress_chunk(c, if_not_compressed => true)
FROM show_chunks('events', older_than => INTERVAL '2 days') c;

-- Verify compression state
SELECT chunk_name,
       range_start,
       range_end,
       is_compressed,
       pg_size_pretty(before_compression_total_bytes) AS before_size,
       pg_size_pretty(after_compression_total_bytes) AS after_size
FROM timescaledb_information.chunks
WHERE hypertable_name = 'events'
ORDER BY range_start DESC
LIMIT 20;

\echo '=== Step 3: Create automated tiering job ==='

-- Move compressed chunks older than 14 days to cold_storage tablespace.
-- Runs daily at 04:00 UTC.
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

SELECT add_job(
    'tier_old_chunks',
    '1 day',
    config => '{"tier_after": "14 days"}'::jsonb,
    initial_start => (CURRENT_DATE + 1 + TIME '04:00:00')::timestamptz
);

\echo '=== Step 4: Create partial GIN index (excludes high-volume tracking sources) ==='

-- The full GIN index on payload consumes ~397 MB/day. AIS/ADSB payloads
-- are never queried via GIN operators, so exclude them.
-- CONCURRENTLY avoids locking the table during creation.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_payload_filtered
ON events USING GIN (payload)
WHERE source_type NOT IN ('ais', 'airplaneslive', 'adsb-fi', 'adsb-lol', 'opensky', 'bgp');

-- Verify the planner uses the new index for typical queries
EXPLAIN ANALYZE
SELECT * FROM events
WHERE payload ? 'enrichment'
  AND source_type = 'rss-news'
  AND event_time > NOW() - INTERVAL '1 day';

-- NOTE: Only drop the old full index AFTER verifying the new partial index
-- is being used by the query planner. Uncomment and run manually:
-- DROP INDEX CONCURRENTLY idx_events_payload;

\echo '=== Step 5: Selective retention for tracking sources ==='

-- Delete tracking events (AIS, aviation) older than 7 days.
-- Intel events (news, conflict, cyber, etc.) keep 180-day retention.
-- Runs daily at 03:00 UTC.
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

SELECT add_job(
    'selective_retention',
    '1 day',
    config => '{"tracking_retention": "7 days"}'::jsonb,
    initial_start => (CURRENT_DATE + 1 + TIME '03:00:00')::timestamptz
);

\echo '=== Step 6: Verify all background jobs ==='

SELECT job_id,
       proc_name,
       schedule_interval,
       config,
       last_run_started_at,
       last_run_status,
       total_runs,
       total_failures
FROM timescaledb_information.jobs
ORDER BY job_id;

\echo '=== Step 7: Verify tablespace usage ==='

SELECT t.spcname,
       pg_size_pretty(pg_tablespace_size(t.oid)) AS size
FROM pg_tablespace t;

\echo ''
\echo '=== Setup complete ==='
\echo ''
\echo 'Data lifecycle:'
\echo '  0-2 days:   Uncompressed on NVMe (hot, writable)'
\echo '  2-14 days:  Compressed on NVMe (warm, read-only)'
\echo '  14+ days:   Compressed on USB-C SSD (cold, read-only)'
\echo '  Tracking:   7-day retention (AIS, aviation)'
\echo '  Intel:      180-day retention (news, conflict, cyber, etc.)'
\echo ''
\echo 'MANUAL STEP: After verifying idx_events_payload_filtered is used by'
\echo 'the query planner, drop the old full GIN index:'
\echo '  DROP INDEX CONCURRENTLY idx_events_payload;'
\echo ''
