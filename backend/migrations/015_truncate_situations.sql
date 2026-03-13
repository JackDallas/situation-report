-- Clean slate: truncate situations with garbage titles from broken AI period (March 7-13, 2026).
-- The pipeline situation graph is in-memory and rebuilds from scratch on restart.
-- New situations will get proper Ollama titles or heuristic fallbacks.
-- CASCADE removes dependent rows in entity_events and any other FK-linked tables.
TRUNCATE situations CASCADE;
