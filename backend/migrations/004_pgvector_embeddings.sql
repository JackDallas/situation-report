-- Enable pgvector extension for vector similarity search
CREATE EXTENSION IF NOT EXISTS vector;

-- Add embedding column for BGE-M3 (1024 dimensions)
ALTER TABLE events ADD COLUMN IF NOT EXISTS embedding vector(1024);

-- HNSW index for fast approximate nearest-neighbor search using cosine distance.
-- Only applies to uncompressed chunks (last 7 days per compression policy),
-- which is fine — we only need vector search within the active 6h correlation window.
CREATE INDEX IF NOT EXISTS idx_events_embedding
    ON events USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);
