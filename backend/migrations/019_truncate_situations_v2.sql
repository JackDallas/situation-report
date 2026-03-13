-- Wipe all situations for clean rebuild with improved title quality,
-- adaptive percentile scoring, and new LlmClient.
-- Pipeline graph rebuilds in-memory from 6h event backfill on restart.
TRUNCATE situations CASCADE;
TRUNCATE situation_summaries;
TRUNCATE situation_timeline;
