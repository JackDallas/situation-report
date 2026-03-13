-- Reset source health counters for clean start with new sources
-- (ACLED, Bluesky, UKMTO Warnings) and rate limit jitter changes.
UPDATE source_health SET consecutive_failures = 0, status = 'unknown', last_error = NULL;
