use std::collections::HashMap;
use std::num::NonZeroUsize;

use lru::LruCache;
use uuid::Uuid;

/// In-memory cache for event embeddings and cluster centroids.
///
/// Event embeddings are keyed by `"source_type:source_id"` or
/// `"source_type:event_type:timestamp"` for events without a source_id.
///
/// Cluster centroids are keyed by cluster UUID and updated via EWMA
/// (exponentially-weighted moving average) so recent events dominate.
pub struct EmbeddingCache {
    /// Event embedding vectors, keyed by embed_key. LRU eviction on insert when at capacity.
    events: LruCache<String, Vec<f32>>,
    /// Cluster centroid vectors, keyed by cluster ID.
    centroids: HashMap<Uuid, Vec<f32>>,
    /// EWMA smoothing factor for centroid updates.
    /// Alpha=0.05 means ~50% weight on the last ~14 events.
    centroid_alpha: f32,
}

impl EmbeddingCache {
    pub fn new(max_events: usize, centroid_alpha: f32) -> Self {
        let cap = NonZeroUsize::new(max_events).unwrap_or(NonZeroUsize::new(1).unwrap());
        Self {
            events: LruCache::new(cap),
            centroids: HashMap::new(),
            centroid_alpha,
        }
    }

    /// Insert an event embedding into the cache.
    /// LruCache automatically evicts the least-recently-used entry when at capacity.
    pub fn insert(&mut self, key: String, embedding: Vec<f32>) {
        self.events.put(key, embedding);
    }

    /// Get an event's embedding by key (promotes to most-recently-used).
    pub fn get(&mut self, key: &str) -> Option<&Vec<f32>> {
        self.events.get(key)
    }

    /// Get an event's embedding by key without promoting it in the LRU order.
    pub fn peek(&self, key: &str) -> Option<&Vec<f32>> {
        self.events.peek(key)
    }

    /// Get a cluster's centroid vector.
    pub fn get_cluster_centroid(&self, cluster_id: &Uuid) -> Option<&Vec<f32>> {
        self.centroids.get(cluster_id)
    }

    /// Initialize a cluster centroid from a single event's embedding.
    pub fn init_centroid(&mut self, cluster_id: Uuid, embedding: &[f32]) {
        self.centroids.insert(cluster_id, embedding.to_vec());
    }

    /// Incrementally update a cluster centroid with a new event's embedding.
    /// Uses EWMA: `centroid[i] = centroid[i] * (1 - alpha) + new[i] * alpha`
    /// so recent events dominate the centroid direction.
    pub fn update_centroid(&mut self, cluster_id: Uuid, embedding: &[f32]) {
        if !self.centroids.contains_key(&cluster_id) {
            self.init_centroid(cluster_id, embedding);
            return;
        }

        let alpha = self.centroid_alpha;
        let decay = 1.0 - alpha;
        if let Some(centroid) = self.centroids.get_mut(&cluster_id) {
            for (i, val) in centroid.iter_mut().enumerate() {
                if let Some(&new_val) = embedding.get(i) {
                    *val = *val * decay + new_val * alpha;
                }
            }
        }
    }

    /// Remove a cluster's centroid (used during prune).
    pub fn remove_centroid(&mut self, cluster_id: &Uuid) {
        self.centroids.remove(cluster_id);
    }

    /// Cosine similarity between two vectors. Returns 0.0 if either is zero-length.
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;

        for i in 0..a.len() {
            dot += a[i] * b[i];
            norm_a += a[i] * a[i];
            norm_b += b[i] * b[i];
        }

        let denom = norm_a.sqrt() * norm_b.sqrt();
        if denom < f32::EPSILON {
            0.0
        } else {
            dot / denom
        }
    }

    /// Number of cached event embeddings.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Number of cached cluster centroids.
    pub fn centroid_count(&self) -> usize {
        self.centroids.len()
    }
}

/// Build the cache key for an event's embedding.
pub fn embed_key(event: &sr_sources::InsertableEvent) -> String {
    match &event.source_id {
        Some(sid) => format!("{}:{}", event.source_type, sid),
        None => format!(
            "{}:{}:{}",
            event.source_type,
            event.event_type,
            event.event_time.timestamp()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingCache::cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = EmbeddingCache::cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = EmbeddingCache::cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingCache::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingCache::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = EmbeddingCache::new(100, 0.05);
        cache.insert("test:1".to_string(), vec![1.0, 2.0, 3.0]);
        assert!(cache.get("test:1").is_some());
        assert!(cache.get("test:2").is_none());
    }

    #[test]
    fn test_centroid_init_and_update_ewma() {
        let alpha = 0.5; // Large alpha for easy hand-calculation
        let mut cache = EmbeddingCache::new(100, alpha);
        let id = Uuid::new_v4();

        // Init with [1.0, 2.0, 3.0]
        cache.init_centroid(id, &[1.0, 2.0, 3.0]);
        let c = cache.get_cluster_centroid(&id).unwrap();
        assert_eq!(c, &vec![1.0, 2.0, 3.0]);

        // Update with [3.0, 4.0, 5.0]
        // EWMA: centroid = old * 0.5 + new * 0.5 = [2.0, 3.0, 4.0]
        cache.update_centroid(id, &[3.0, 4.0, 5.0]);
        let c = cache.get_cluster_centroid(&id).unwrap();
        assert!((c[0] - 2.0).abs() < 1e-6);
        assert!((c[1] - 3.0).abs() < 1e-6);
        assert!((c[2] - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_centroid_remove() {
        let mut cache = EmbeddingCache::new(100, 0.05);
        let id = Uuid::new_v4();
        cache.init_centroid(id, &[1.0, 2.0]);
        assert!(cache.get_cluster_centroid(&id).is_some());
        cache.remove_centroid(&id);
        assert!(cache.get_cluster_centroid(&id).is_none());
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = EmbeddingCache::new(3, 0.05);
        cache.insert("a".to_string(), vec![1.0]);
        cache.insert("b".to_string(), vec![2.0]);
        cache.insert("c".to_string(), vec![3.0]);
        assert_eq!(cache.event_count(), 3);
        // Next insert triggers eviction of half (1 entry)
        cache.insert("d".to_string(), vec![4.0]);
        assert!(cache.event_count() <= 3);
    }

    #[test]
    fn test_ewma_centroid_tracks_recent_events() {
        // With alpha=0.05, after many updates the centroid should be
        // much closer to recent embeddings than to old ones.
        let mut cache = EmbeddingCache::new(100, 0.05);
        let id = Uuid::new_v4();

        // Phase 1: 50 events pointing in the [1, 0, 0] direction
        let old_dir = [1.0_f32, 0.0, 0.0];
        cache.init_centroid(id, &old_dir);
        for _ in 0..49 {
            cache.update_centroid(id, &old_dir);
        }

        // Phase 2: 50 events pointing in the [0, 1, 0] direction
        let new_dir = [0.0_f32, 1.0, 0.0];
        for _ in 0..50 {
            cache.update_centroid(id, &new_dir);
        }

        let centroid = cache.get_cluster_centroid(&id).unwrap();
        let sim_old = EmbeddingCache::cosine_similarity(centroid, &old_dir);
        let sim_new = EmbeddingCache::cosine_similarity(centroid, &new_dir);

        // Centroid should be closer to the recent direction
        assert!(
            sim_new > sim_old,
            "EWMA centroid should track recent events: sim_new={sim_new} should be > sim_old={sim_old}"
        );
    }

    #[test]
    fn test_ewma_centroid_convergence() {
        // With alpha=0.3 (fast tracking), after many identical updates
        // the centroid should converge to the update vector.
        let mut cache = EmbeddingCache::new(100, 0.3);
        let id = Uuid::new_v4();

        cache.init_centroid(id, &[1.0, 0.0, 0.0]);

        let target = [0.0_f32, 1.0, 0.0];
        for _ in 0..100 {
            cache.update_centroid(id, &target);
        }

        let centroid = cache.get_cluster_centroid(&id).unwrap();
        let sim = EmbeddingCache::cosine_similarity(centroid, &target);
        // After 100 updates at alpha=0.3, (1-0.3)^100 ~ 2.5e-16 of old signal remains
        assert!(
            sim > 0.999,
            "Centroid should have converged to target: sim={sim}"
        );
    }

    #[test]
    fn test_update_centroid_without_init_acts_as_init() {
        let mut cache = EmbeddingCache::new(100, 0.05);
        let id = Uuid::new_v4();

        // update_centroid on unknown cluster should initialize it
        cache.update_centroid(id, &[5.0, 6.0, 7.0]);
        let c = cache.get_cluster_centroid(&id).unwrap();
        assert_eq!(c, &vec![5.0, 6.0, 7.0]);
    }
}
