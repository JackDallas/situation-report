use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

/// Persist an event's embedding to the `events.embedding` column.
///
/// Updates the row matching (source_type, source_id, event_time).
pub async fn store_embedding(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: chrono::DateTime<chrono::Utc>,
    embedding: &[f32],
) -> Result<()> {
    let vec = Vector::from(embedding.to_vec());
    let result = sqlx::query(
        "UPDATE events SET embedding = $1
         WHERE source_type = $2 AND source_id = $3 AND event_time = $4",
    )
    .bind(vec)
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .execute(pool)
    .await?;

    debug!(
        rows = result.rows_affected(),
        source_type, source_id, "Stored embedding"
    );
    Ok(())
}

/// Find similar events using vector cosine distance.
///
/// Returns (source_type, source_id, event_time, distance) tuples
/// ordered by similarity (closest first).
pub async fn find_similar(
    pool: &PgPool,
    embedding: &[f32],
    limit: i64,
    min_similarity: f64,
) -> Result<Vec<SimilarEvent>> {
    let vec = Vector::from(embedding.to_vec());
    // cosine distance = 1 - cosine_similarity, so we filter where distance <= (1 - min_similarity)
    let max_distance = 1.0 - min_similarity;

    let rows = sqlx::query_as::<_, SimilarEvent>(
        "SELECT source_type, source_id, event_time,
                (embedding <=> $1) AS distance
         FROM events
         WHERE embedding IS NOT NULL
           AND (embedding <=> $1) <= $2
         ORDER BY embedding <=> $1
         LIMIT $3",
    )
    .bind(vec)
    .bind(max_distance)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

#[derive(Debug, sqlx::FromRow)]
pub struct SimilarEvent {
    pub source_type: String,
    pub source_id: Option<String>,
    pub event_time: chrono::DateTime<chrono::Utc>,
    pub distance: f64,
}

/// Persist a situation cluster's centroid embedding to the `situations` table.
pub async fn store_centroid(
    pool: &PgPool,
    situation_id: Uuid,
    centroid: &[f32],
) -> Result<()> {
    let vec = Vector::from(centroid.to_vec());
    sqlx::query("UPDATE situations SET centroid_embedding = $1 WHERE id = $2")
        .bind(vec)
        .bind(situation_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Load all active situation centroids for cache warming on startup.
///
/// Returns (situation_id, centroid_vector) pairs for all non-resolved situations
/// that have a persisted centroid embedding.
pub async fn load_all_centroids(pool: &PgPool) -> Result<Vec<(Uuid, Vec<f32>)>> {
    #[derive(sqlx::FromRow)]
    struct CentroidRow {
        id: Uuid,
        embedding_text: String,
    }

    let rows = sqlx::query_as::<_, CentroidRow>(
        "SELECT id, centroid_embedding::text AS embedding_text \
         FROM situations \
         WHERE centroid_embedding IS NOT NULL \
           AND phase::text NOT IN ('resolved', 'historical') \
           AND updated_at > NOW() - INTERVAL '72 hours'",
    )
    .fetch_all(pool)
    .await?;

    // Parse pgvector text format "[0.1,0.2,...]" into Vec<f32>
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let trimmed = r.embedding_text.trim_matches(|c| c == '[' || c == ']');
            let vec: Vec<f32> = trimmed
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if vec.len() == 1024 {
                Some((r.id, vec))
            } else {
                None
            }
        })
        .collect())
}
