-- Reset situations for consolidation tuning (lower topic threshold + title Jaccard).
-- Pipeline graph is in-memory — rebuilds from scratch on restart.
TRUNCATE situations CASCADE;
