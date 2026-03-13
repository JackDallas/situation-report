-- Persist embedding centroids for situation clusters so they survive restarts.
-- Without this, restarting the app clears the in-memory centroid cache, causing
-- cluster proliferation until the cache slowly rebuilds from new events.
ALTER TABLE situations ADD COLUMN IF NOT EXISTS centroid_embedding vector(1024);
