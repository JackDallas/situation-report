//! Cluster merge logic for the situation graph.
//!
//! Contains `merge_overlapping()` (parent-child hierarchy creation),
//! `unmerge()`, `expire_merge_rejections()`, `split_divergent()`,
//! and post-merge cap enforcement.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sr_intel::search::SearchHistory;
use sr_types::SourceType;
use uuid::Uuid;

use sr_embeddings::EmbeddingCache;
use tracing::{info, warn};

use super::scoring::regions_overlap;
use super::{median_centroid, SituationCluster, SituationGraph};

/// K-means with k=2 for splitting incoherent clusters.
///
/// Initialises centroids as the two most distant embeddings (lowest cosine
/// similarity), then iterates up to `max_iters` Lloyd steps using cosine
/// similarity as the assignment metric.
///
/// Returns `(group_a_indices, group_b_indices)` or `None` if the split is
/// not viable (fewer than 2 embeddings, or one group ends up empty).
pub(crate) fn kmeans_2(embeddings: &[Vec<f32>], max_iters: usize) -> Option<(Vec<usize>, Vec<usize>)> {
    if embeddings.len() < 2 {
        return None;
    }

    // 1. Init: pick the two most distant embeddings as initial centroids
    let mut min_sim = f32::MAX;
    let mut seed_a = 0usize;
    let mut seed_b = 1usize;
    for i in 0..embeddings.len() {
        for j in (i + 1)..embeddings.len() {
            let sim = EmbeddingCache::cosine_similarity(&embeddings[i], &embeddings[j]);
            if sim < min_sim {
                min_sim = sim;
                seed_a = i;
                seed_b = j;
            }
        }
    }

    let dim = embeddings[0].len();
    let mut centroid_a: Vec<f32> = embeddings[seed_a].clone();
    let mut centroid_b: Vec<f32> = embeddings[seed_b].clone();

    let mut assignments: Vec<u8> = vec![0; embeddings.len()];

    for _iter in 0..max_iters {
        let prev = assignments.clone();

        // Assign each embedding to nearest centroid
        for (idx, emb) in embeddings.iter().enumerate() {
            let sim_a = EmbeddingCache::cosine_similarity(emb, &centroid_a);
            let sim_b = EmbeddingCache::cosine_similarity(emb, &centroid_b);
            assignments[idx] = if sim_a >= sim_b { 0 } else { 1 };
        }

        // Early termination if assignments didn't change
        if assignments == prev {
            break;
        }

        // Recompute centroids as component-wise mean of assigned embeddings
        let mut sum_a = vec![0.0f32; dim];
        let mut sum_b = vec![0.0f32; dim];
        let mut count_a = 0usize;
        let mut count_b = 0usize;

        for (idx, emb) in embeddings.iter().enumerate() {
            if assignments[idx] == 0 {
                for (d, val) in emb.iter().enumerate() {
                    sum_a[d] += val;
                }
                count_a += 1;
            } else {
                for (d, val) in emb.iter().enumerate() {
                    sum_b[d] += val;
                }
                count_b += 1;
            }
        }

        if count_a == 0 || count_b == 0 {
            // Degenerate — one cluster got everything
            return None;
        }

        for d in 0..dim {
            centroid_a[d] = sum_a[d] / count_a as f32;
            centroid_b[d] = sum_b[d] / count_b as f32;
        }
    }

    let group_a: Vec<usize> = assignments.iter().enumerate()
        .filter(|(_, a)| **a == 0)
        .map(|(i, _)| i)
        .collect();
    let group_b: Vec<usize> = assignments.iter().enumerate()
        .filter(|(_, a)| **a == 1)
        .map(|(i, _)| i)
        .collect();

    if group_a.is_empty() || group_b.is_empty() {
        return None;
    }

    Some((group_a, group_b))
}

impl SituationGraph {
    /// Build a map of parent_id -> count of children.
    fn child_count_map(&self) -> HashMap<Uuid, usize> {
        let mut counts: HashMap<Uuid, usize> = HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                *counts.entry(pid).or_default() += 1;
            }
        }
        counts
    }

    /// Build an index of parent_id -> list of child IDs.
    fn parent_to_children_index(&self) -> HashMap<Uuid, Vec<Uuid>> {
        let mut index: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                index.entry(pid).or_default().push(c.id);
            }
        }
        index
    }

    /// Merge a child cluster's data into a parent cluster.
    ///
    /// Transfers event_ids, source_types, event_titles, severity, first_seen,
    /// last_updated, coord_buffer, and event_count from child to parent.
    /// signal_event_count is NOT transferred — each cluster tracks only its
    /// own directly-ingested high-signal events to prevent inflation from
    /// merge/unmerge cycles. Returns `false` if either cluster is missing.
    pub(crate) fn merge_child_into_parent(&mut self, parent_id: Uuid, child_id: Uuid) -> bool {
        // Collect child data first (immutable borrow)
        let (child_event_ids, child_source_types, child_event_titles,
             child_severity, child_first_seen, child_last_updated,
             child_coord_buffer, child_event_count) = {
            let Some(child) = self.clusters.get(&child_id) else {
                warn!(child = %child_id, "child missing from clusters during merge");
                return false;
            };
            (
                child.event_ids.clone(),
                child.source_types.clone(),
                child.event_titles.clone(),
                child.severity,
                child.first_seen,
                child.last_updated,
                child.coord_buffer.clone(),
                child.event_count,
            )
        };

        let Some(parent) = self.clusters.get_mut(&parent_id) else {
            warn!(parent = %parent_id, "parent missing from clusters during merge");
            return false;
        };

        parent.event_ids.extend(child_event_ids);
        let max_eids = self.config.cluster_caps.max_event_ids;
        if parent.event_ids.len() > max_eids {
            let drain_count = parent.event_ids.len() - max_eids;
            parent.event_ids.drain(..drain_count);
        }
        parent.event_count += child_event_count;
        // NOTE: signal_event_count is NOT transferred from child to parent.
        // Each cluster's signal count reflects only its own directly-ingested
        // high-signal events. Transferring it caused runaway inflation because
        // merge/unmerge cycles would re-add the child's count to the parent
        // without ever subtracting it on unmerge.
        parent.source_types.extend(child_source_types);
        for title in child_event_titles {
            if parent.event_titles.len() < self.config.cluster_caps.max_event_titles {
                parent.event_titles.push(title);
            }
        }
        if child_event_count >= self.config.quality.min_events_standalone {
            parent.severity = parent.severity.max(child_severity);
        }
        if child_first_seen < parent.first_seen {
            parent.first_seen = child_first_seen;
        }
        if child_last_updated > parent.last_updated {
            parent.last_updated = child_last_updated;
        }
        if !child_coord_buffer.is_empty() {
            // Filter out Null Island (0,0) coords inherited from child
            let filtered: Vec<(f64, f64)> = child_coord_buffer
                .into_iter()
                .filter(|(lat, lon)| !super::is_null_island(*lat, *lon))
                .collect();
            if !filtered.is_empty() {
                parent.coord_buffer.extend(filtered);
                if parent.coord_buffer.len() > 30 {
                    parent.coord_buffer.drain(..parent.coord_buffer.len() - 30);
                }
                parent.centroid = Some(median_centroid(&parent.coord_buffer));
            }
        }

        true
    }

    /// Merge overlapping clusters into parent-child hierarchy.
    /// Returns `Vec<(parent_id, child_id, skip_audit)>`.
    /// When `skip_audit` is true, the merge was a forced consolidation (title-identity
    /// or regional absorb) and should not be sent to the LLM audit.
    pub fn merge_overlapping(&mut self, embedding_cache: Option<&EmbeddingCache>) -> Vec<(Uuid, Uuid, bool)> {
        let ids: Vec<Uuid> = self.clusters.keys().copied().collect();
        let mut merges: Vec<(Uuid, Uuid, bool)> = Vec::new(); // (parent, child, skip_audit)

        // Pre-count children per cluster for cap enforcement
        let mut child_count = self.child_count_map();
        let parent_to_children = self.parent_to_children_index();

        let max_children = self.config.cluster_caps.max_children_per_parent;
        let max_events = self.config.cluster_caps.max_events_per_parent;

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let a_id = ids[i];
                let b_id = ids[j];
                let (a, b) = match (self.clusters.get(&a_id), self.clusters.get(&b_id)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => continue,
                };

                // Skip if either already has a parent
                if a.parent_id.is_some() || b.parent_id.is_some() {
                    continue;
                }

                // Skip pairs previously rejected by LLM audit
                let rejection_key = if a_id < b_id { (a_id, b_id) } else { (b_id, a_id) };
                if self.merge_rejections.contains_key(&rejection_key) {
                    continue;
                }

                // Skip if the larger cluster already has too many children
                let a_children = child_count.get(&a_id).copied().unwrap_or(0);
                let b_children = child_count.get(&b_id).copied().unwrap_or(0);
                let larger_children = if a.event_count >= b.event_count { a_children } else { b_children };
                let _smaller_children = if a.event_count >= b.event_count { b_children } else { a_children };
                let both_have_children = a_children > 0 && b_children > 0;
                let child_cap = if both_have_children { max_children + 1 } else { max_children };
                if larger_children >= child_cap {
                    continue;
                }
                let larger_events = a.event_count.max(b.event_count);
                if larger_events >= max_events {
                    continue;
                }

                // Check region overlap
                let region_overlap = a.region_codes.intersection(&b.region_codes).count();
                let cross_region = region_overlap == 0 && !a.region_codes.is_empty() && !b.region_codes.is_empty();

                // Check overlap: >= 2 shared entities.
                // For parent-parent comparisons, include child entities
                let a_entities_expanded: HashSet<String>;
                let b_entities_expanded: HashSet<String>;
                let both_are_parents_preview = a_children > 0 && b_children > 0;
                if both_are_parents_preview {
                    a_entities_expanded = {
                        let mut ents = a.entities.clone();
                        if let Some(children) = parent_to_children.get(&a_id) {
                            for &cid in children {
                                if let Some(c) = self.clusters.get(&cid) {
                                    ents.extend(c.entities.iter().take(5).cloned());
                                }
                            }
                        }
                        ents
                    };
                    b_entities_expanded = {
                        let mut ents = b.entities.clone();
                        if let Some(children) = parent_to_children.get(&b_id) {
                            for &cid in children {
                                if let Some(c) = self.clusters.get(&cid) {
                                    ents.extend(c.entities.iter().take(5).cloned());
                                }
                            }
                        }
                        ents
                    };
                } else {
                    a_entities_expanded = a.entities.clone();
                    b_entities_expanded = b.entities.clone();
                }
                let shared_entities = a_entities_expanded.intersection(&b_entities_expanded).count();
                let shared_region = regions_overlap(&a.region_codes, &b.region_codes);

                // Cluster title similarity (require >= 3 significant words each)
                let cluster_titles_similar = {
                    let enough_words = |t: &str| t.split_whitespace().filter(|w| w.len() > 2).count() >= 3;
                    if enough_words(&a.title) && enough_words(&b.title) {
                        super::scoring::title_jaccard(&a.title, &b.title)
                    } else {
                        0.0
                    }
                };

                // Semantic similarity via embeddings
                let semantically_similar = embedding_cache
                    .and_then(|cache| {
                        let ca = cache.get_cluster_centroid(&a_id)?;
                        let cb = cache.get_cluster_centroid(&b_id)?;
                        Some(EmbeddingCache::cosine_similarity(ca, cb))
                    })
                    .unwrap_or(0.0);

                // News-only clusters need stronger evidence
                let both_news_only = a.source_types.iter().all(|s| matches!(s,
                    SourceType::RssNews | SourceType::Gdelt | SourceType::GdeltGeo))
                    && b.source_types.iter().all(|s| matches!(s,
                    SourceType::RssNews | SourceType::Gdelt | SourceType::GdeltGeo));

                // Title-similarity fast path: merge clusters with similar titles.
                // Very high similarity (≥0.80) merges regardless of region — identical
                // titles obviously belong together even with different region codes.
                // Moderate similarity (≥threshold) requires region overlap.
                let both_regions_empty = a.region_codes.is_empty() && b.region_codes.is_empty();
                let title_identity_merge = if cluster_titles_similar >= 0.80 {
                    // Near-identical titles: skip region check entirely
                    true
                } else {
                    cluster_titles_similar >= self.config.merge.title_identity_threshold
                        && (shared_region || both_regions_empty)
                        && !cross_region
                };

                // Guard: if both clusters have zero entities — allow through if
                // titles are moderately similar in the same region
                if !title_identity_merge
                    && a.entities.is_empty() && b.entities.is_empty()
                    && semantically_similar < self.config.merge.entity_empty_semantic_threshold as f32
                    && cluster_titles_similar < 0.40
                {
                    continue;
                }

                // Low-content guard — bypass when titles or regions suggest relatedness
                let a_signals = a.entities.len() + a.topics.len();
                let b_signals = b.entities.len() + b.topics.len();
                if !title_identity_merge
                    && a_signals <= 2 && b_signals <= 2
                    && semantically_similar < self.config.merge.low_content_semantic_threshold as f32
                    && !(cluster_titles_similar >= 0.40 && (shared_region || both_regions_empty))
                {
                    continue;
                }

                // Vector-primary merge (or title/heuristic fallback)
                let sim_f64 = semantically_similar as f64;
                let smaller_events = a.event_count.min(b.event_count);
                let larger_events_count = a.event_count.max(b.event_count);

                // Regional consolidation: small news-only clusters in a region
                // with a dominant situation should be absorbed as children.
                // This prevents top-level clutter from policy/diplomatic sub-angles.
                let smaller_source_types = if a.event_count <= b.event_count {
                    &a.source_types
                } else {
                    &b.source_types
                };
                let shared_topics = a.topics.intersection(&b.topics).count();
                let regional_absorb = !cross_region && (shared_region || both_regions_empty)
                    && smaller_events <= self.config.merge.regional_absorb_max_smaller
                    && larger_events_count >= self.config.merge.regional_absorb_min_larger
                    && (shared_topics >= 1 || shared_entities >= 1)
                    && smaller_source_types.iter().all(|s| matches!(s,
                        SourceType::RssNews | SourceType::Gdelt | SourceType::GdeltGeo
                        | SourceType::Geoconfirmed | SourceType::Telegram | SourceType::Bluesky));
                let should_merge = if title_identity_merge {
                    true
                } else if regional_absorb {
                    true
                } else if sim_f64 < 0.01 {
                    // Fallback: when centroids are missing, use entity+region+title heuristic.
                    // Require stronger evidence to prevent spurious merges.
                    let effective_region = shared_region || both_regions_empty;
                    let heuristic_title = self.config.merge.heuristic_title_threshold;
                    (shared_entities >= 2 && effective_region)
                        || (cluster_titles_similar >= heuristic_title && effective_region && shared_entities >= 1)
                        // Same-region clusters with strong title overlap
                        || (cluster_titles_similar >= heuristic_title && effective_region && !cross_region
                            && shared_topics >= 1)
                } else {
                    let mc = &self.config.merge;
                    let mut score = sim_f64;
                    if shared_entities >= 2 {
                        score += mc.vector_boost_entities_2;
                    } else if shared_entities == 1 && shared_region {
                        score += mc.vector_boost_entity_region;
                    }
                    if shared_region {
                        score += mc.vector_boost_region;
                    }
                    if cluster_titles_similar >= 0.6 {
                        score += mc.vector_boost_title_similar;
                    }
                    if shared_topics >= 2 {
                        score += mc.vector_boost_shared_topics;
                    }

                    // Natural disasters are genuinely global — don't penalize cross-region
                    let both_natural_disaster = {
                        let a_nd = a.topics.iter().any(|t| super::scoring::is_natural_disaster_topic(t));
                        let b_nd = b.topics.iter().any(|t| super::scoring::is_natural_disaster_topic(t));
                        a_nd && b_nd
                    };

                    let threshold = if cross_region && !both_natural_disaster {
                        mc.vector_threshold_cross_region
                    } else if both_news_only {
                        mc.vector_threshold_news_only
                    } else {
                        mc.vector_threshold_default
                    };
                    score >= threshold
                };

                if !should_merge {
                    continue;
                }

                // Determine parent/child: larger cluster absorbs smaller
                // Skip audit for forced consolidation (title-identity, regional absorb)
                // and for heuristic fallback merges when centroids are missing (recovery mode)
                let skip_audit = title_identity_merge || regional_absorb || sim_f64 < 0.01;
                if a.event_count >= b.event_count {
                    merges.push((a_id, b_id, skip_audit));
                } else {
                    merges.push((b_id, a_id, skip_audit));
                }
            }
        }

        // Apply merges
        let mut applied_merges: Vec<(Uuid, Uuid, bool)> = Vec::new();
        for (parent_id, child_id, skip_audit) in merges {
            if !self.clusters.contains_key(&child_id) {
                continue;
            }
            if !self.clusters.contains_key(&parent_id) {
                continue;
            }
            if self.clusters.get(&child_id).is_some_and(|c| c.parent_id.is_some()) {
                continue;
            }
            // Grandparent guard
            let child_own_children = parent_to_children.get(&child_id).map_or(0, |c| c.len());
            if child_own_children >= 3 {
                let title_override = if let (Some(parent), Some(child)) =
                    (self.clusters.get(&parent_id), self.clusters.get(&child_id))
                {
                    super::scoring::title_jaccard(&parent.title, &child.title) >= 0.8
                } else {
                    false
                };
                if !title_override {
                    continue;
                }
            }
            let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
            let child_has_children = parent_to_children.get(&child_id).map_or(false, |c| !c.is_empty());
            let effective_cap = if child_has_children { max_children + 1 } else { max_children };
            if live_children >= effective_cap {
                continue;
            }
            let parent_events = self.clusters.get(&parent_id).map(|c| c.event_count).unwrap_or(0);
            if parent_events >= max_events {
                continue;
            }

            if let Some(child) = self.clusters.get_mut(&child_id) {
                child.parent_id = Some(parent_id);
            }
            *child_count.entry(parent_id).or_default() += 1;

            if !child_has_children {
                let grandchild_ids: Vec<Uuid> = parent_to_children.get(&child_id)
                    .map_or_else(Vec::new, |ids| ids.clone());
                let current_children = child_count.get(&parent_id).copied().unwrap_or(0);
                let can_absorb = max_children.saturating_sub(current_children);
                for gc_id in grandchild_ids.iter().take(can_absorb) {
                    if let Some(gc) = self.clusters.get_mut(gc_id) {
                        gc.parent_id = Some(parent_id);
                    }
                    *child_count.entry(parent_id).or_default() += 1;
                }
                for gc_id in grandchild_ids.iter().skip(can_absorb) {
                    if let Some(gc) = self.clusters.get_mut(gc_id) {
                        gc.parent_id = None;
                    }
                }
            }

            applied_merges.push((parent_id, child_id, skip_audit));

            // Merge child data into parent
            let source_types_before = self.clusters.get(&parent_id).map_or(0, |p| p.source_types.len());
            if !self.merge_child_into_parent(parent_id, child_id) {
                continue;
            }

            let parent_child_count = child_count.get(&parent_id).copied().unwrap_or(0);

            if let Some(parent) = self.clusters.get_mut(&parent_id) {
                let source_types_added = parent.source_types.len().saturating_sub(source_types_before);
                if source_types_added >= 1 {
                    parent.has_ai_title = false;
                    parent.title_signal_count_at_gen = 0;
                }
                if !parent.has_ai_title {
                    let new_title = Self::generate_title(
                        &parent.entities,
                        &parent.topics,
                        &parent.region_codes,
                    );
                    if Self::should_accept_title(&parent.title, &new_title, parent_child_count, parent.event_count, parent.phase, parent.severity) {
                        parent.title = new_title;
                    } else {
                        info!(
                            cluster_id = %parent_id,
                            old_title = %parent.title,
                            rejected_title = %new_title,
                            child_count = parent_child_count,
                            event_count = parent.event_count,
                            "Merge title update rejected: stability check for parent situation"
                        );
                    }
                }
            }
        }

        // Post-merge cap enforcement
        if !applied_merges.is_empty() {
            self.enforce_max_children_cap(max_children);
        }

        applied_merges
    }

    /// Topic-density consolidation pass — catches related situations that the
    /// pairwise embedding merge misses.
    ///
    /// Algorithm:
    /// 1. Collect top-level situations (no parent_id) with their topics and entities.
    /// 2. Build topic groups: for each topic appearing in >=2 top-level situations,
    ///    collect those situation IDs. Only considers topics >= min_entity_len chars.
    /// 3. For each pair found via shared topics, check if they share >= min_shared_topics
    ///    topics total. If so, merge (larger absorbs smaller).
    ///    Also merges pairs sharing a key entity + any shared topic.
    /// 4. Skips pairs in merge_rejections. Respects max_children and max_events caps.
    /// 5. Returns merges with skip_audit=true (forced consolidation, no LLM audit needed).
    pub fn consolidate_by_topic(&mut self) -> Vec<(Uuid, Uuid, bool)> {
        let min_entity_len = self.config.merge.min_entity_len_consolidation;
        let min_shared_topics = self.config.merge.min_shared_topics_consolidation;
        let max_children = self.config.cluster_caps.max_children_per_parent;
        let max_events = self.config.cluster_caps.max_events_per_parent;

        // 1. Collect top-level situation IDs
        let top_level_ids: Vec<Uuid> = self.clusters.iter()
            .filter(|(_, c)| c.parent_id.is_none())
            .map(|(&id, _)| id)
            .collect();

        if top_level_ids.len() < 2 {
            return Vec::new();
        }

        // 2. Build topic -> set of top-level situation IDs
        // Use topics as the primary grouping signal (entities are often empty
        // because enrichment populates topics but not always entities).
        let mut topic_groups: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for &sid in &top_level_ids {
            if let Some(cluster) = self.clusters.get(&sid) {
                for topic in &cluster.topics {
                    if topic.len() >= min_entity_len {
                        topic_groups.entry(topic.clone()).or_default().insert(sid);
                    }
                }
            }
        }

        // Also build entity groups for entity-based matching
        let mut entity_groups: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for &sid in &top_level_ids {
            if let Some(cluster) = self.clusters.get(&sid) {
                for entity in &cluster.entities {
                    if entity.len() >= min_entity_len {
                        entity_groups.entry(entity.clone()).or_default().insert(sid);
                    }
                }
            }
        }

        // Only keep topics/entities that appear in >=2 top-level situations
        topic_groups.retain(|_, sids| sids.len() >= 2);
        entity_groups.retain(|_, sids| sids.len() >= 2);

        // 2b. Build title word sets for Jaccard-based title consolidation.
        // This catches near-duplicate situations with empty topics but similar titles.
        let title_jaccard_threshold = self.config.merge.title_jaccard_consolidation;
        let title_words: HashMap<Uuid, HashSet<String>> = top_level_ids.iter()
            .filter_map(|&sid| {
                let cluster = self.clusters.get(&sid)?;
                let words: HashSet<String> = cluster.title.to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() >= 3)
                    .filter(|w| !matches!(*w,
                        // English stopwords
                        "the" | "and" | "for" | "with" | "from" | "that" | "this"
                        | "into" | "over" | "has" | "are" | "was" | "were" | "been"
                        | "have" | "will" | "not" | "but" | "its" | "says" | "new"
                        | "after" | "amid" | "near" | "against"
                        // Region/geography stopwords — prevent "{Country} — REGION" false matches
                        | "middle" | "east" | "west" | "north" | "south" | "southeast"
                        | "asia" | "africa" | "europe" | "america" | "americas" | "pacific"
                        | "atlantic" | "eastern" | "western" | "southern" | "northern"
                        | "central" | "region" | "global"
                    ))
                    // Stem-normalize by truncating to 4 chars: "iranian"→"iran", "israeli"→"isra"
                    .map(|w| {
                        let stem: String = w.chars().take(4).collect();
                        stem
                    })
                    .collect();
                if words.is_empty() { None } else { Some((sid, words)) }
            })
            .collect();

        // Build word -> situation IDs index for efficient pair generation
        let mut word_groups: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for (&sid, words) in &title_words {
            for word in words {
                word_groups.entry(word.clone()).or_default().insert(sid);
            }
        }
        word_groups.retain(|_, sids| sids.len() >= 2);

        // Track word frequency for specificity filtering: a "specific" word
        // appears in ≤8 titles. Shared words that are ALL generic (>8 titles)
        // won't count as valid Jaccard matches.
        let word_freq: HashMap<&str, usize> = word_groups.iter()
            .map(|(w, sids)| (w.as_str(), sids.len()))
            .collect();

        if topic_groups.is_empty() && entity_groups.is_empty() && word_groups.is_empty() {
            return Vec::new();
        }

        // Pre-count children per cluster for cap enforcement
        let mut child_count = self.child_count_map();
        let parent_to_children = self.parent_to_children_index();

        // 3. Collect candidate merges from topic groups
        let mut merge_candidates: Vec<(Uuid, Uuid)> = Vec::new();
        let mut seen_pairs: HashSet<(Uuid, Uuid)> = HashSet::new();

        // Topic-based: pairs sharing a topic, then check if they share >= N total topics
        for (_topic, sids) in &topic_groups {
            let sid_vec: Vec<Uuid> = sids.iter().copied().collect();
            for i in 0..sid_vec.len() {
                for j in (i + 1)..sid_vec.len() {
                    let a_id = sid_vec[i];
                    let b_id = sid_vec[j];
                    let canonical = if a_id < b_id { (a_id, b_id) } else { (b_id, a_id) };

                    if seen_pairs.contains(&canonical) {
                        continue;
                    }
                    seen_pairs.insert(canonical);

                    if self.merge_rejections.contains_key(&canonical) {
                        continue;
                    }

                    let shared_topics = match (self.clusters.get(&a_id), self.clusters.get(&b_id)) {
                        (Some(a), Some(b)) => a.topics.intersection(&b.topics).count(),
                        _ => continue,
                    };

                    if shared_topics >= min_shared_topics {
                        merge_candidates.push((a_id, b_id));
                    }
                }
            }
        }

        // Entity-based: pairs sharing a key entity + at least 1 shared topic
        for (_entity, sids) in &entity_groups {
            let sid_vec: Vec<Uuid> = sids.iter().copied().collect();
            for i in 0..sid_vec.len() {
                for j in (i + 1)..sid_vec.len() {
                    let a_id = sid_vec[i];
                    let b_id = sid_vec[j];
                    let canonical = if a_id < b_id { (a_id, b_id) } else { (b_id, a_id) };

                    if seen_pairs.contains(&canonical) {
                        continue;
                    }
                    seen_pairs.insert(canonical);

                    if self.merge_rejections.contains_key(&canonical) {
                        continue;
                    }

                    let shared_topics = match (self.clusters.get(&a_id), self.clusters.get(&b_id)) {
                        (Some(a), Some(b)) => a.topics.intersection(&b.topics).count(),
                        _ => continue,
                    };

                    if shared_topics >= 1 {
                        merge_candidates.push((a_id, b_id));
                    }
                }
            }
        }

        // Country/proper-noun stems that should always count as "specific" for
        // title-word matching, regardless of how many titles contain them.
        const COUNTRY_STEMS: &[&str] = &[
            "iran", "iraq", "isra", "ukra", "russ", "chin", "indi", "japa", "kore",
            "yeme", "syri", "turk", "paki", "afgh", "suda", "soma", "liby", "egyp",
            "saud", "qata", "bahr", "cana", "fran", "germ", "pola", "serb", "mexi",
            "vene", "peru", "guat", "indo", "vanu", "taiw", "phil", "myan", "horm",
            "hout", "hezb",
        ];

        // Title-word Jaccard: pairs sharing a title word, then check Jaccard >= threshold.
        // Catches near-duplicate situations with empty topics but similar titles,
        // e.g. "Iran-Israel Conflict Escalates" + "Iran-Israel Conflict Escalation".
        if title_jaccard_threshold > 0.0 {
            for (_word, sids) in &word_groups {
                let sid_vec: Vec<Uuid> = sids.iter().copied().collect();
                for i in 0..sid_vec.len() {
                    for j in (i + 1)..sid_vec.len() {
                        let a_id = sid_vec[i];
                        let b_id = sid_vec[j];
                        let canonical = if a_id < b_id { (a_id, b_id) } else { (b_id, a_id) };

                        if seen_pairs.contains(&canonical) {
                            continue;
                        }
                        seen_pairs.insert(canonical);

                        if self.merge_rejections.contains_key(&canonical) {
                            continue;
                        }

                        // Compute Jaccard similarity on title words
                        let (a_words, b_words) = match (title_words.get(&a_id), title_words.get(&b_id)) {
                            (Some(a), Some(b)) => (a, b),
                            _ => continue,
                        };
                        let shared: Vec<&String> = a_words.intersection(b_words).collect();
                        let union = a_words.union(b_words).count();
                        if union == 0 || shared.is_empty() {
                            continue;
                        }

                        // Country stems always count as "specific" (proper nouns are
                        // meaningful even when frequent). Other words use the ≤8 filter.
                        let has_country = shared.iter().any(|w| COUNTRY_STEMS.contains(&w.as_str()));
                        let has_specific = has_country || shared.iter().any(|w| {
                            word_freq.get(w.as_str()).copied().unwrap_or(0) <= 8
                        });
                        if !has_specific {
                            continue;
                        }

                        // Require at least 2 shared words normally, but 1 country stem
                        // + Jaccard >= threshold is enough (e.g. "Iran War" + "Iran Spy").
                        if shared.len() < 2 && !has_country {
                            continue;
                        }

                        let jaccard = shared.len() as f64 / union as f64;
                        if jaccard >= title_jaccard_threshold {
                            merge_candidates.push((a_id, b_id));
                        }
                    }
                }
            }
        }

        if merge_candidates.is_empty() {
            return Vec::new();
        }

        // 4. Apply merges: larger absorbs smaller
        let mut applied_merges: Vec<(Uuid, Uuid, bool)> = Vec::new();

        for (a_id, b_id) in merge_candidates {
            // Re-check both are still top-level (a previous merge in this pass
            // may have already made one a child)
            let (a_events, a_parent) = match self.clusters.get(&a_id) {
                Some(c) => (c.event_count, c.parent_id),
                None => continue,
            };
            let (b_events, b_parent) = match self.clusters.get(&b_id) {
                Some(c) => (c.event_count, c.parent_id),
                None => continue,
            };

            if a_parent.is_some() || b_parent.is_some() {
                continue;
            }

            // Determine parent (larger) and child (smaller)
            let (parent_id, child_id) = if a_events >= b_events {
                (a_id, b_id)
            } else {
                (b_id, a_id)
            };

            // Cap checks
            let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
            let child_has_children = parent_to_children.get(&child_id).map_or(false, |c| !c.is_empty());
            let effective_cap = if child_has_children { max_children + 1 } else { max_children };
            if live_children >= effective_cap {
                continue;
            }
            let parent_events = self.clusters.get(&parent_id).map(|c| c.event_count).unwrap_or(0);
            if parent_events >= max_events {
                continue;
            }

            // Perform the merge
            if let Some(child) = self.clusters.get_mut(&child_id) {
                child.parent_id = Some(parent_id);
            }
            *child_count.entry(parent_id).or_default() += 1;

            // Reparent grandchildren to parent (flatten hierarchy)
            if child_has_children {
                let grandchild_ids: Vec<Uuid> = parent_to_children.get(&child_id)
                    .map_or_else(Vec::new, |ids| ids.clone());
                let current_children = child_count.get(&parent_id).copied().unwrap_or(0);
                let can_absorb = max_children.saturating_sub(current_children);
                for gc_id in grandchild_ids.iter().take(can_absorb) {
                    if let Some(gc) = self.clusters.get_mut(gc_id) {
                        gc.parent_id = Some(parent_id);
                    }
                    *child_count.entry(parent_id).or_default() += 1;
                }
                // Any that can't be absorbed stay as children of the (now child) situation
            }

            // Merge child data into parent
            let source_types_before = self.clusters.get(&parent_id).map_or(0, |p| p.source_types.len());
            if !self.merge_child_into_parent(parent_id, child_id) {
                continue;
            }

            let parent_child_count = child_count.get(&parent_id).copied().unwrap_or(0);

            if let Some(parent) = self.clusters.get_mut(&parent_id) {
                let source_types_added = parent.source_types.len().saturating_sub(source_types_before);
                if source_types_added >= 1 {
                    parent.has_ai_title = false;
                    parent.title_signal_count_at_gen = 0;
                }
                if !parent.has_ai_title {
                    let new_title = Self::generate_title(
                        &parent.entities,
                        &parent.topics,
                        &parent.region_codes,
                    );
                    if Self::should_accept_title(&parent.title, &new_title, parent_child_count, parent.event_count, parent.phase, parent.severity) {
                        parent.title = new_title;
                    }
                }
            }

            applied_merges.push((parent_id, child_id, true));
            info!(
                %parent_id, %child_id,
                "Topic-density consolidation: merged (shared entity + topics)"
            );
        }

        applied_merges
    }

    /// Return top-level situations for periodic LLM batch consolidation.
    ///
    /// Returns `(id, title, topics)` for each top-level situation (no parent_id).
    /// The pipeline layer uses this to build entity-based groups and send them
    /// to the LLM for grouping decisions.
    pub fn situations_for_llm_consolidation(&self) -> Vec<(Uuid, String, Vec<String>)> {
        self.clusters.values()
            .filter(|c| c.parent_id.is_none())
            .map(|c| (
                c.id,
                c.title.clone(),
                c.entities.iter().cloned().collect::<Vec<_>>(),
                c.topics.iter().cloned().collect::<Vec<_>>(),
            ))
            .map(|(id, title, _entities, topics)| (id, title, topics))
            .collect()
    }

    /// Return top-level situations with entities for LLM consolidation grouping.
    ///
    /// Returns `(id, title, entities, topics, event_count)` for each top-level situation.
    pub fn situations_for_llm_consolidation_with_entities(&self) -> Vec<(Uuid, String, Vec<String>, Vec<String>, usize)> {
        self.clusters.values()
            .filter(|c| c.parent_id.is_none())
            .map(|c| (
                c.id,
                c.title.clone(),
                c.entities.iter().cloned().collect(),
                c.topics.iter().cloned().collect(),
                c.event_count,
            ))
            .collect()
    }

    /// Apply LLM-decided consolidation merges.
    ///
    /// Takes groups of situation IDs (each group should be merged together).
    /// Within each group, the situation with the most events becomes the parent.
    /// Returns merges as `(parent_id, child_id, skip_audit=true)`.
    pub fn apply_llm_consolidation_groups(&mut self, groups: &[Vec<Uuid>]) -> Vec<(Uuid, Uuid, bool)> {
        let max_children = self.config.cluster_caps.max_children_per_parent;
        let max_events = self.config.cluster_caps.max_events_per_parent;

        let mut child_count = self.child_count_map();

        let mut applied: Vec<(Uuid, Uuid, bool)> = Vec::new();

        for group in groups {
            if group.len() < 2 {
                continue;
            }

            // Find the largest situation in this group (by event_count)
            let mut group_with_events: Vec<(Uuid, usize)> = group.iter()
                .filter_map(|&id| {
                    let c = self.clusters.get(&id)?;
                    if c.parent_id.is_some() {
                        return None; // already merged
                    }
                    Some((id, c.event_count))
                })
                .collect();

            if group_with_events.len() < 2 {
                continue;
            }

            // Sort descending by event_count — first is the parent
            group_with_events.sort_by(|a, b| b.1.cmp(&a.1));
            let parent_id = group_with_events[0].0;

            let parent_title = self.clusters.get(&parent_id).map(|c| c.title.clone()).unwrap_or_default();
            for &(child_id, _) in group_with_events.iter().skip(1) {
                // Cap checks
                let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
                if live_children >= max_children {
                    let child_title = self.clusters.get(&child_id).map(|c| c.title.as_str()).unwrap_or("?");
                    info!(
                        parent = %parent_title, child = child_title,
                        live_children, max_children,
                        "LLM consolidation blocked: max_children cap"
                    );
                    break;
                }
                let parent_events = self.clusters.get(&parent_id).map(|c| c.event_count).unwrap_or(0);
                if parent_events >= max_events {
                    let child_title = self.clusters.get(&child_id).map(|c| c.title.as_str()).unwrap_or("?");
                    info!(
                        parent = %parent_title, child = child_title,
                        parent_events, max_events,
                        "LLM consolidation blocked: max_events cap"
                    );
                    break;
                }

                // LLM consolidation overrides audit rejections — batch grouping
                // is a more deliberate decision than single-pair audit.
                let rejection_key = if parent_id < child_id { (parent_id, child_id) } else { (child_id, parent_id) };
                if self.merge_rejections.remove(&rejection_key).is_some() {
                    let child_title = self.clusters.get(&child_id).map(|c| c.title.as_str()).unwrap_or("?");
                    info!(
                        parent = %parent_title, child = child_title,
                        "LLM consolidation: cleared prior rejection, proceeding with merge"
                    );
                }

                // Re-verify child is still top-level
                if self.clusters.get(&child_id).is_some_and(|c| c.parent_id.is_some()) {
                    continue;
                }

                // Perform the merge
                if let Some(child) = self.clusters.get_mut(&child_id) {
                    child.parent_id = Some(parent_id);
                }
                *child_count.entry(parent_id).or_default() += 1;

                // Merge child data into parent
                if !self.merge_child_into_parent(parent_id, child_id) {
                    continue;
                }
                if let Some(parent) = self.clusters.get_mut(&parent_id) {
                    parent.has_ai_title = false;
                    parent.title_signal_count_at_gen = 0;
                }

                applied.push((parent_id, child_id, true));
                info!(
                    %parent_id, %child_id,
                    "LLM consolidation: merged by batch grouping"
                );
            }
        }

        applied
    }

    /// Undo a merge: clear the child's parent_id so it becomes a standalone cluster again.
    pub fn unmerge(&mut self, parent_id: Uuid, child_id: Uuid) {
        if let Some(child) = self.clusters.get_mut(&child_id) {
            child.parent_id = None;
            let key = if parent_id < child_id { (parent_id, child_id) } else { (child_id, parent_id) };
            self.merge_rejections.insert(key, Utc::now());
            info!(%parent_id, %child_id, title = %child.title,
                rejections = self.merge_rejections.len(),
                "Unmerged cluster after audit rejection — cached to prevent re-merge");
        }
    }

    /// Expire old merge rejections so situations can be reconsidered as they evolve.
    pub fn expire_merge_rejections(&mut self) {
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        let before = self.merge_rejections.len();
        self.merge_rejections.retain(|_, ts| *ts > cutoff);
        let expired = before - self.merge_rejections.len();
        if expired > 0 {
            info!(expired, remaining = self.merge_rejections.len(), "Expired stale merge rejections");
        }
    }

    /// Dynamic country-sweep consolidation: groups top-level situations by
    /// country/region keywords in titles and force-merges when a region has
    /// too many fragments.
    ///
    /// The threshold adapts to the total top-level count:
    ///   - >100 top-level → merge when a region has ≥2 situations
    ///   - 50-100 → merge when ≥3
    ///   - <50 → merge when ≥4
    ///
    /// This catches situations that share a conflict theater (e.g. Iran/Israel/
    /// Yemen) but have no shared topics, entities, or embeddings to link them.
    pub fn country_sweep_consolidation(&mut self) -> Vec<(Uuid, Uuid, bool)> {
        let max_children = self.config.cluster_caps.max_children_per_parent;

        // Collect top-level situations with their titles
        let top_level: Vec<(Uuid, String, usize)> = self.clusters.iter()
            .filter(|(_, c)| c.parent_id.is_none())
            .map(|(&id, c)| (id, c.title.to_lowercase(), c.event_count))
            .collect();

        let tl_count = top_level.len();
        if tl_count < 2 {
            return Vec::new();
        }

        // Adaptive threshold based on total top-level count
        let merge_threshold: usize = if tl_count > 100 {
            2
        } else if tl_count > 50 {
            3
        } else {
            4
        };

        // Extract country keywords from each title and group situation IDs
        // Uses the same country list as LLM consolidation batching.
        const COUNTRY_KEYWORDS: &[&str] = &[
            "iran", "iraq", "israel", "ukraine", "russia", "china", "india",
            "japan", "korea", "yemen", "syria", "turkey", "pakistan", "afghan",
            "sudan", "somalia", "libya", "egypt", "saudi", "qatar", "bahrain",
            "france", "germany", "poland", "serbia", "mexico",
            "venezuela", "peru", "indonesia", "vanuatu",
            "taiwan", "philippines", "myanmar", "hormuz", "houthi", "hezbollah",
            "palestine", "palestinian", "gaza", "lebanon", "persian",
        ];

        // Conflict theaters: countries that should be grouped together
        // because they're part of the same broader conflict.
        fn theater_key(keyword: &str) -> &'static str {
            match keyword {
                "iran" | "israel" | "yemen" | "houthi" | "hezbollah"
                | "hormuz" | "persian" | "palestine" | "palestinian"
                | "gaza" | "lebanon" | "iraq" | "syria" => "mideast-iran-israel",
                "ukraine" | "russia" => "ukraine-russia",
                "china" | "taiwan" => "china-taiwan",
                _ => "other",
            }
        }

        // Title words that indicate conflict/military activity — situations with
        // these words belong in a conflict theater. Situations about elections,
        // cyber-attacks, diplomacy, etc. should NOT be swept into the theater
        // even if they mention the same country (e.g., "Russia Election
        // Interference" should not merge with "Ukraine Kupiansk Offensive").
        const CONFLICT_TITLE_WORDS: &[&str] = &[
            "war", "offensive", "strike", "strikes", "attack", "attacks",
            "military", "missile", "drone", "bomb", "bombing", "shelling",
            "frontline", "battle", "siege", "invasion", "escalat",
            "combat", "artillery", "rocket", "casualties", "killed",
            "naval", "navy", "airforce", "army", "armed", "forces",
            "weapon", "nuclear", "troops", "deploy", "advance",
            "coalition", "defense", "defence", "fighter", "jet",
            "ship", "shipping", "fleet", "blockade", "patrol",
            "tensions", "crisis", "conflict",
            "threat", "guard", "guards", "incursion", "operation", "operations",
            "pressure", "pressures", "surge", "revolutionary", "sanctions",
            "desperation", "espionage", "spy", "assassination", "urges",
            "warns", "warning", "embargo", "intercept", "seize", "seizes",
        ];

        fn has_conflict_title(title: &str) -> bool {
            CONFLICT_TITLE_WORDS.iter().any(|w| title.contains(w))
        }

        let mut theater_groups: HashMap<String, Vec<(Uuid, usize)>> = HashMap::new();
        for &(id, ref title, event_count) in &top_level {
            // Check ALL keywords, prefer conflict theater over individual country
            let mut best_theater: Option<&str> = None;
            let mut best_keyword: Option<&str> = None;
            for &kw in COUNTRY_KEYWORDS {
                if title.contains(kw) {
                    let theater = theater_key(kw);
                    if theater != "other" {
                        // Conflict theater takes priority — stop looking
                        best_theater = Some(theater);
                        break;
                    } else if best_keyword.is_none() {
                        best_keyword = Some(kw);
                        best_theater = Some("other");
                    }
                }
            }
            if let Some(theater) = best_theater {
                // For named conflict theaters, only include situations whose
                // titles indicate military/conflict activity. This prevents
                // "Russia Election Interference" from absorbing war situations
                // just because it mentions "Russia".
                if theater != "other" && !has_conflict_title(title) {
                    continue;
                }
                let key = if theater == "other" {
                    best_keyword.unwrap_or("unknown").to_string()
                } else {
                    theater.to_string()
                };
                theater_groups.entry(key).or_default().push((id, event_count));
            }
        }

        let mut applied = Vec::new();
        let mut child_count = self.child_count_map();

        for (theater, mut sids) in theater_groups {
            // Deduplicate (a title might match multiple keywords in same theater)
            sids.sort_by_key(|(id, _)| *id);
            sids.dedup_by_key(|(id, _)| *id);

            if sids.len() < merge_threshold {
                continue;
            }

            // Pick the situation with most events as parent
            sids.sort_by(|a, b| b.1.cmp(&a.1));
            let parent_id = sids[0].0;

            let parent_title = self.clusters.get(&parent_id)
                .map(|c| c.title.clone()).unwrap_or_default();

            for &(child_id, _) in sids.iter().skip(1) {
                // Skip if child is already parented (may have been merged this pass)
                if self.clusters.get(&child_id).is_some_and(|c| c.parent_id.is_some()) {
                    continue;
                }

                // Check children cap (no events cap — deliberate consolidation)
                let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
                if live_children >= max_children {
                    break;
                }

                // Clear any rejection for this pair
                let rejection_key = if parent_id < child_id {
                    (parent_id, child_id)
                } else {
                    (child_id, parent_id)
                };
                self.merge_rejections.remove(&rejection_key);

                // Merge child into parent
                if let Some(child) = self.clusters.get_mut(&child_id) {
                    child.parent_id = Some(parent_id);
                }
                *child_count.entry(parent_id).or_default() += 1;

                // Transfer data to parent
                if !self.merge_child_into_parent(parent_id, child_id) {
                    continue;
                }
                if let Some(parent) = self.clusters.get_mut(&parent_id) {
                    parent.has_ai_title = false;
                    parent.title_signal_count_at_gen = 0;
                }

                let child_title = self.clusters.get(&child_id)
                    .map(|c| c.title.as_str()).unwrap_or("?");
                info!(
                    %parent_id, %child_id,
                    parent = %parent_title, child = %child_title,
                    theater,
                    "Country sweep: merged by conflict theater"
                );

                applied.push((parent_id, child_id, true)); // skip_audit = true
            }
        }

        if !applied.is_empty() {
            info!(count = applied.len(), "Country sweep consolidation produced merges");
        }
        applied
    }

    /// Split clusters that have grown too large and contain divergent entity subgroups.
    pub fn split_divergent(&mut self) {
        let min_events = self.config.sweep.split_divergent_min_events;
        let large_ids: Vec<Uuid> = self.clusters
            .iter()
            .filter(|(_, c)| c.event_count >= min_events && c.parent_id.is_none())
            .map(|(&id, _)| id)
            .collect();

        let mut splits: Vec<(Uuid, Vec<HashSet<String>>)> = Vec::new();

        for cid in large_ids {
            let cluster = match self.clusters.get(&cid) {
                Some(c) => c,
                None => continue,
            };

            let entities: Vec<String> = cluster.entities.iter().cloned().collect();
            if entities.len() < self.config.sweep.split_divergent_min_entities {
                continue;
            }

            let mut cooccur: HashMap<String, HashSet<String>> = HashMap::new();
            for title in &cluster.event_titles {
                let lower = title.to_lowercase();
                let present: Vec<&String> = entities.iter()
                    .filter(|e| lower.contains(e.as_str()))
                    .collect();
                for e in &present {
                    let entry = cooccur.entry((*e).clone()).or_default();
                    for other in &present {
                        if *other != *e {
                            entry.insert((*other).clone());
                        }
                    }
                }
            }

            let mut remaining: HashSet<String> = entities.into_iter().collect();
            let mut subgroups: Vec<HashSet<String>> = Vec::new();

            while !remaining.is_empty() {
                let seed = remaining.iter().next().cloned().unwrap();
                let mut group: HashSet<String> = HashSet::new();
                group.insert(seed.clone());
                remaining.remove(&seed);

                if let Some(neighbors) = cooccur.get(&seed) {
                    for n in neighbors {
                        if remaining.contains(n) {
                            group.insert(n.clone());
                            remaining.remove(n);
                        }
                    }
                }
                subgroups.push(group);
            }

            if subgroups.len() >= 2 {
                let total_entities: usize = subgroups.iter().map(|g| g.len()).sum();
                let largest = subgroups.iter().map(|g| g.len()).max().unwrap_or(0);
                let overlap_ratio = largest as f64 / total_entities as f64;
                if overlap_ratio < self.config.sweep.split_divergent_max_overlap {
                    splits.push((cid, subgroups));
                }
            }
        }

        // Apply splits
        for (parent_id, subgroups) in splits {
            let parent = match self.clusters.remove(&parent_id) {
                Some(c) => c,
                None => continue,
            };

            for e in &parent.entities {
                if let Some(set) = self.entity_index.get_mut(e) {
                    set.remove(&parent_id);
                }
            }
            for t in &parent.topics {
                if let Some(set) = self.topic_index.get_mut(t) {
                    set.remove(&parent_id);
                }
            }

            for entity_group in subgroups {
                let child_id = Uuid::new_v4();

                let child_topics: HashSet<String> = parent.topics.iter()
                    .filter(|t| {
                        parent.event_titles.iter().any(|title| {
                            let lower = title.to_lowercase();
                            entity_group.iter().any(|e| lower.contains(e.as_str()))
                                && lower.contains(t.as_str())
                        })
                    })
                    .cloned()
                    .collect();

                let proportion = entity_group.len() as f64 / parent.entities.len().max(1) as f64;
                let child_event_count = ((parent.event_count as f64) * proportion).ceil() as usize;
                let child_event_count = child_event_count.max(1);

                let take_count = child_event_count.min(parent.event_ids.len());
                let child_event_ids: Vec<(chrono::DateTime<chrono::Utc>, String)> = parent.event_ids
                    .iter()
                    .take(take_count)
                    .cloned()
                    .collect();

                let child_title = SituationGraph::generate_title(&entity_group, &child_topics, &parent.region_codes);
                let child = SituationCluster {
                    id: child_id,
                    title: child_title,
                    entities: entity_group.clone(),
                    topics: child_topics.clone(),
                    event_ids: child_event_ids,
                    region_codes: parent.region_codes.clone(),
                    severity: parent.severity,
                    first_seen: parent.first_seen,
                    last_updated: parent.last_updated,
                    centroid: parent.centroid,
                    coord_buffer: parent.coord_buffer.clone(),
                    event_count: child_event_count,
                    signal_event_count: 0,
                    source_types: parent.source_types.clone(),
                    parent_id: Some(parent_id),
                    event_titles: parent.event_titles.iter()
                        .filter(|t| {
                            let lower = t.to_lowercase();
                            entity_group.iter().any(|e| lower.contains(e.as_str()))
                        })
                        .take(10)
                        .cloned()
                        .collect(),
                    has_ai_title: false,
                    title_signal_count_at_gen: 0,
                    last_title_gen: Utc::now(),
                    supplementary: None,
                    last_searched: None,
                    search_history: SearchHistory::default(),
                    phase: parent.phase,
                    phase_changed_at: parent.phase_changed_at,
                    peak_event_rate: parent.peak_event_rate,
                    peak_rate_at: parent.peak_rate_at,
                    phase_transitions: Vec::new(),
                    certainty: 0.0,
                    anomaly_score: 0.0,
                    last_retro_sweep: None,
                    total_events_ingested: child_event_count,
                    // Split children don't have direct ingestion history — start fresh
                    direct_event_count: 0,
                    direct_source_types: HashSet::new(),
                };

                for e in &entity_group {
                    self.entity_index.entry(e.clone()).or_default().insert(child_id);
                }
                for t in &child_topics {
                    self.topic_index.entry(t.clone()).or_default().insert(child_id);
                }

                self.clusters.insert(child_id, child);
            }

            let mut parent_shell = parent;
            parent_shell.has_ai_title = false;
            parent_shell.entities.clear();
            self.clusters.insert(parent_id, parent_shell);
        }
    }

    /// Split an incoherent cluster using k-means (k=2) on recent event embeddings.
    ///
    /// Looks up the last `coherence_sample_size` event embeddings from the cache,
    /// runs 2-means clustering, and if both groups are large enough, creates a new
    /// child cluster from the smaller group's events.
    ///
    /// Returns `Some(new_cluster_id)` if a split was performed, `None` otherwise.
    pub fn split_by_coherence(
        &mut self,
        cluster_id: Uuid,
        embedding_cache: &EmbeddingCache,
        min_group_size: usize,
    ) -> Option<Uuid> {
        let sample_size = self.config.sweep.coherence_sample_size;

        // Collect recent event_ids (ref keys) from the cluster
        let recent_keys: Vec<(chrono::DateTime<Utc>, String)> = {
            let cluster = self.clusters.get(&cluster_id)?;
            cluster.event_ids.iter()
                .rev()
                .take(sample_size)
                .cloned()
                .collect()
        };

        // Look up embeddings, keeping track of which index maps to which event
        let mut keyed_embeddings: Vec<(usize, Vec<f32>)> = Vec::new();
        for (idx, (_ts, ref_id)) in recent_keys.iter().enumerate() {
            if let Some(emb) = embedding_cache.peek(ref_id) {
                keyed_embeddings.push((idx, emb.clone()));
            }
        }

        // Need at least 6 embeddings to meaningfully split
        if keyed_embeddings.len() < 6 {
            return None;
        }

        let embeddings: Vec<Vec<f32>> = keyed_embeddings.iter().map(|(_, e)| e.clone()).collect();
        let (group_a, group_b) = kmeans_2(&embeddings, 10)?;

        // The smaller group becomes the new cluster
        let (_larger_group, smaller_group) = if group_a.len() >= group_b.len() {
            (group_a, group_b)
        } else {
            (group_b, group_a)
        };

        if smaller_group.len() < min_group_size {
            return None;
        }

        // Map k-means indices back to event_ids indices
        let smaller_event_indices: HashSet<usize> = smaller_group.iter()
            .map(|&kmeans_idx| keyed_embeddings[kmeans_idx].0)
            .collect();

        // Collect the event_ids for the smaller group
        let split_event_ids: Vec<(chrono::DateTime<Utc>, String)> = smaller_event_indices.iter()
            .filter_map(|&idx| recent_keys.get(idx).cloned())
            .collect();

        if split_event_ids.is_empty() {
            return None;
        }

        // Snapshot parent data before mutation
        let (parent_region_codes, parent_severity, parent_first_seen,
             parent_last_updated, parent_centroid, parent_source_types,
             parent_phase, parent_phase_changed_at, parent_peak_event_rate,
             parent_peak_rate_at) = {
            let cluster = self.clusters.get(&cluster_id)?;
            (
                cluster.region_codes.clone(),
                cluster.severity,
                cluster.first_seen,
                cluster.last_updated,
                cluster.centroid,
                cluster.source_types.clone(),
                cluster.phase,
                cluster.phase_changed_at,
                cluster.peak_event_rate,
                cluster.peak_rate_at,
            )
        };

        // Build new child cluster
        let new_id = Uuid::new_v4();
        let split_event_count = split_event_ids.len();

        let (parent_coord_buffer, parent_entities, parent_topics, parent_event_titles) = {
            let cluster = self.clusters.get(&cluster_id)?;
            (
                cluster.coord_buffer.clone(),
                cluster.entities.clone(),
                cluster.topics.clone(),
                cluster.event_titles.clone(),
            )
        };

        // Populate child's entities/topics from split event ref_ids.
        // Match entities and topics that appear in event titles associated with the split events.
        let _split_ref_ids_set: HashSet<&str> = split_event_ids.iter()
            .map(|(_, ref_id)| ref_id.as_str())
            .collect();

        // Use the same approach as split_divergent(): filter parent's entities/topics
        // by co-occurrence with event titles. Since we don't have a direct mapping from
        // event_ids to titles, we use a proportion-based allocation — take entities/topics
        // that appear in the parent's event titles most associated with the split group.
        let child_entities: HashSet<String> = parent_entities.iter()
            .filter(|e| {
                parent_event_titles.iter().any(|t| t.to_lowercase().contains(e.to_lowercase().as_str()))
            })
            .cloned()
            .collect();

        let child_topics: HashSet<String> = parent_topics.iter()
            .filter(|t| {
                parent_event_titles.iter().any(|title| title.to_lowercase().contains(t.as_str()))
            })
            .cloned()
            .collect();

        let child_event_titles: Vec<String> = parent_event_titles.iter()
            .take(split_event_count.min(10))
            .cloned()
            .collect();

        let split_title = SituationGraph::generate_title(&child_entities, &child_topics, &parent_region_codes);
        let new_cluster = SituationCluster {
            id: new_id,
            title: split_title,
            entities: child_entities,
            topics: child_topics,
            event_ids: split_event_ids.clone(),
            region_codes: parent_region_codes,
            severity: parent_severity,
            first_seen: parent_first_seen,
            last_updated: parent_last_updated,
            centroid: parent_centroid,
            coord_buffer: parent_coord_buffer,
            event_count: split_event_count,
            signal_event_count: 0,
            source_types: parent_source_types,
            parent_id: Some(cluster_id),
            event_titles: child_event_titles,
            has_ai_title: false,
            title_signal_count_at_gen: 0,
            last_title_gen: Utc::now(),
            supplementary: None,
            last_searched: None,
            search_history: SearchHistory::default(),
            phase: parent_phase,
            phase_changed_at: parent_phase_changed_at,
            peak_event_rate: parent_peak_event_rate,
            peak_rate_at: parent_peak_rate_at,
            phase_transitions: Vec::new(),
            certainty: 0.0,
            anomaly_score: 0.0,
            last_retro_sweep: None,
            total_events_ingested: split_event_count,
            // Coherence-split children don't have direct ingestion history — start fresh
            direct_event_count: 0,
            direct_source_types: HashSet::new(),
        };

        // Update entity/topic indexes for the new child cluster
        if let Some(ref cluster) = Some(&new_cluster) {
            for e in &cluster.entities {
                self.entity_index.entry(e.clone()).or_default().insert(new_id);
            }
            for t in &cluster.topics {
                self.topic_index.entry(t.clone()).or_default().insert(new_id);
            }
        }
        self.clusters.insert(new_id, new_cluster);

        // Remove the split events from the original cluster
        let split_ref_ids: HashSet<String> = split_event_ids.iter()
            .map(|(_, ref_id)| ref_id.clone())
            .collect();

        if let Some(cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.event_ids.retain(|(_, ref_id)| !split_ref_ids.contains(ref_id));
            cluster.event_count = cluster.event_ids.len();
            cluster.has_ai_title = false;
            cluster.title_signal_count_at_gen = 0;
        }

        // Initialize centroid for the new cluster from its embeddings
        // (average of the split group's embeddings)
        let dim = embeddings[0].len();
        let mut centroid_sum = vec![0.0f32; dim];
        for &kmeans_idx in &smaller_group {
            for (d, val) in embeddings[kmeans_idx].iter().enumerate() {
                centroid_sum[d] += val;
            }
        }
        let n = smaller_group.len() as f32;
        let new_centroid: Vec<f32> = centroid_sum.iter().map(|v| v / n).collect();

        // We can't mutate the cache here since we only have &EmbeddingCache,
        // but the centroid will be rebuilt as new events arrive.
        // The split itself is the important action.
        let _ = new_centroid; // centroid computed but cache is immutable here

        Some(new_id)
    }
}
