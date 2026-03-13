-- Reset failure counters for sources that hit the max-failures circuit breaker
-- (consecutive_failures >= 8 for poll sources, >= 10 for streaming).
-- After deploy, the registry spawns fresh tokio tasks with in-memory counters
-- starting at 0, so these sources will resume polling automatically.
-- This migration clears the DB-side health record so the /api/sources dashboard
-- no longer shows them as errored.

UPDATE source_health
SET consecutive_failures = 0,
    status = 'unknown',
    last_error = NULL
WHERE source_id IN ('gdelt', 'gdacs', 'ooni');
