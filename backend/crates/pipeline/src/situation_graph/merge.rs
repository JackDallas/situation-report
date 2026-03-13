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
use tracing::info;

use super::scoring::regions_overlap;
use super::{SituationCluster, SituationGraph};

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
    /// Merge overlapping clusters into parent-child hierarchy.
    /// Returns `Vec<(parent_id, child_id, skip_audit)>`.
    /// When `skip_audit` is true, the merge was a forced consolidation (title-identity
    /// or regional absorb) and should not be sent to the LLM audit.
    pub fn merge_overlapping(&mut self, embedding_cache: Option<&EmbeddingCache>) -> Vec<(Uuid, Uuid, bool)> {
        let ids: Vec<Uuid> = self.clusters.keys().copied().collect();
        let mut merges: Vec<(Uuid, Uuid, bool)> = Vec::new(); // (parent, child, skip_audit)

        // Pre-count children per cluster for cap enforcement
        let mut child_count: HashMap<Uuid, usize> = HashMap::new();
        for c in self.clusters.values() {
            if let Some(pid) = c.parent_id {
                *child_count.entry(pid).or_default() += 1;
            }
        }

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
                        for c in self.clusters.values() {
                            if c.parent_id == Some(a_id) {
                                ents.extend(c.entities.iter().take(5).cloned());
                            }
                        }
                        ents
                    };
                    b_entities_expanded = {
                        let mut ents = b.entities.clone();
                        for c in self.clusters.values() {
                            if c.parent_id == Some(b_id) {
                                ents.extend(c.entities.iter().take(5).cloned());
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

                // Cluster title similarity
                let cluster_titles_similar = {
                    let words_a: HashSet<String> = a.title
                        .to_lowercase()
                        .split_whitespace()
                        .filter(|w| w.len() > 2)
                        .map(|w| w.to_string())
                        .collect();
                    let words_b: HashSet<String> = b.title
                        .to_lowercase()
                        .split_whitespace()
                        .filter(|w| w.len() > 2)
                        .map(|w| w.to_string())
                        .collect();
                    if words_a.len() < 3 || words_b.len() < 3 {
                        0.0
                    } else {
                        let intersection = words_a.intersection(&words_b).count();
                        let union = words_a.union(&words_b).count();
                        if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
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

                // Title-similarity fast path: if titles are highly similar and
                // regions overlap (or both lack region data), merge regardless of
                // embedding state. This handles duplicate clusters from restart
                // and related sub-topics.
                let both_regions_empty = a.region_codes.is_empty() && b.region_codes.is_empty();
                let title_identity_merge = cluster_titles_similar >= 0.60
                    && (shared_region || both_regions_empty)
                    && !cross_region;

                // Guard: if both clusters have zero entities — allow through if
                // titles are moderately similar in the same region
                if !title_identity_merge
                    && a.entities.is_empty() && b.entities.is_empty()
                    && semantically_similar < 0.75
                    && cluster_titles_similar < 0.40
                {
                    continue;
                }

                // Low-content guard — bypass when titles or regions suggest relatedness
                let a_signals = a.entities.len() + a.topics.len();
                let b_signals = b.entities.len() + b.topics.len();
                if !title_identity_merge
                    && a_signals <= 2 && b_signals <= 2
                    && semantically_similar < 0.80
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
                let regional_absorb = !cross_region && (shared_region || both_regions_empty)
                    && smaller_events <= 20 && larger_events_count >= 50
                    && smaller_source_types.iter().all(|s| matches!(s,
                        SourceType::RssNews | SourceType::Gdelt | SourceType::GdeltGeo
                        | SourceType::Geoconfirmed | SourceType::Telegram));
                let should_merge = if title_identity_merge {
                    true
                } else if regional_absorb {
                    true
                } else if sim_f64 < 0.01 {
                    // Fallback: when centroids are missing, use entity+region+title heuristic
                    let effective_region = shared_region || both_regions_empty;
                    (shared_entities >= 2 && effective_region)
                        || (cluster_titles_similar >= 0.40 && effective_region && shared_entities >= 1)
                        // Absorb small orphan into large cluster with shared entity+region
                        || (smaller_events <= 20 && larger_events_count >= 20
                            && shared_entities >= 1 && effective_region
                            && cluster_titles_similar >= 0.25)
                        // Same-region clusters with moderate title overlap (catches "Sudan Civil War X" variants)
                        || (cluster_titles_similar >= 0.40 && effective_region && !cross_region
                            && (a_signals + b_signals) >= 2)
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
            let child_own_children = self.clusters.values()
                .filter(|c| c.parent_id == Some(child_id))
                .count();
            if child_own_children >= 3 {
                let title_override = if let (Some(parent), Some(child)) =
                    (self.clusters.get(&parent_id), self.clusters.get(&child_id))
                {
                    let words_a: HashSet<String> = parent.title.to_lowercase()
                        .split_whitespace().filter(|w| w.len() > 2)
                        .map(|w| w.to_string()).collect();
                    let words_b: HashSet<String> = child.title.to_lowercase()
                        .split_whitespace().filter(|w| w.len() > 2)
                        .map(|w| w.to_string()).collect();
                    if words_a.is_empty() || words_b.is_empty() {
                        false
                    } else {
                        let inter = words_a.intersection(&words_b).count();
                        let union = words_a.union(&words_b).count();
                        let sim = if union == 0 { 0.0 } else { inter as f64 / union as f64 };
                        sim >= 0.8
                    }
                } else {
                    false
                };
                if !title_override {
                    continue;
                }
            }
            let live_children = child_count.get(&parent_id).copied().unwrap_or(0);
            let child_has_children = self.clusters.values().any(|c| c.parent_id == Some(child_id));
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
                let grandchild_ids: Vec<Uuid> = self.clusters.values()
                    .filter(|c| c.parent_id == Some(child_id))
                    .map(|c| c.id)
                    .collect();
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

            // Collect child data to merge into parent
            let (child_entities, child_topics, child_event_ids, child_source_types,
                 child_region_codes, child_event_titles, child_severity,
                 child_first_seen, child_last_updated, child_centroid, child_event_count,
                 child_signal_count) = {
                let child = self.clusters.get(&child_id).unwrap();
                (
                    child.entities.clone(),
                    child.topics.clone(),
                    child.event_ids.clone(),
                    child.source_types.clone(),
                    child.region_codes.clone(),
                    child.event_titles.clone(),
                    child.severity,
                    child.first_seen,
                    child.last_updated,
                    child.centroid,
                    child.event_count,
                    child.signal_event_count,
                )
            };

            let parent_child_count = self.clusters.values().filter(|c| c.parent_id == Some(parent_id)).count();

            if let Some(parent) = self.clusters.get_mut(&parent_id) {
                let entities_before = parent.entities.len();
                let source_types_before = parent.source_types.len();
                for e in &child_entities {
                    if parent.entities.len() >= self.config.cluster_caps.max_entities { break; }
                    if parent.entities.insert(e.clone()) {
                        self.entity_index
                            .entry(e.clone())
                            .or_default()
                            .insert(parent_id);
                    }
                }
                let shared_child_topics: Vec<String> = child_topics
                    .iter()
                    .filter(|t| parent.topics.contains(*t))
                    .cloned()
                    .collect();
                for t in &shared_child_topics {
                    self.topic_index
                        .entry(t.clone())
                        .or_default()
                        .insert(parent_id);
                }
                parent.event_ids.extend(child_event_ids);
                let max_eids = self.config.cluster_caps.max_event_ids;
                if parent.event_ids.len() > max_eids {
                    let drain_count = parent.event_ids.len() - max_eids;
                    parent.event_ids.drain(..drain_count);
                }
                parent.event_count += child_event_count;
                parent.signal_event_count += child_signal_count;
                parent.source_types.extend(child_source_types);
                parent.region_codes.extend(child_region_codes);
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
                if let Some((clat, clon)) = child_centroid {
                    let cn = child_event_count as f64;
                    parent.centroid = Some(match parent.centroid {
                        Some((plat, plon)) => {
                            let pn = (parent.event_count as f64) - cn;
                            let total = pn + cn;
                            ((plat * pn + clat * cn) / total, (plon * pn + clon * cn) / total)
                        }
                        None => (clat, clon),
                    });
                }
                let entities_added = parent.entities.len().saturating_sub(entities_before);
                let source_types_added = parent.source_types.len().saturating_sub(source_types_before);
                if entities_added >= 3 || source_types_added >= 1 {
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
            let mut children_per_parent: HashMap<Uuid, Vec<(Uuid, usize)>> = HashMap::new();
            for c in self.clusters.values() {
                if let Some(pid) = c.parent_id {
                    children_per_parent.entry(pid).or_default().push((c.id, c.event_count));
                }
            }
            for (pid, mut kids) in children_per_parent {
                if kids.len() <= max_children {
                    continue;
                }
                kids.sort_by(|a, b| b.1.cmp(&a.1));
                let orphaned = kids.len() - max_children;
                for &(kid_id, _) in kids.iter().skip(max_children) {
                    if let Some(kid) = self.clusters.get_mut(&kid_id) {
                        kid.parent_id = None;
                    }
                }
                info!(
                    parent = %pid,
                    orphaned = orphaned,
                    "Post-merge cap enforcement: orphaned excess children"
                );
            }
        }

        applied_merges
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

    /// Split clusters that have grown too large and contain divergent entity subgroups.
    pub fn split_divergent(&mut self) {
        let large_ids: Vec<Uuid> = self.clusters
            .iter()
            .filter(|(_, c)| c.event_count >= 30 && c.parent_id.is_none())
            .map(|(&id, _)| id)
            .collect();

        let mut splits: Vec<(Uuid, Vec<HashSet<String>>)> = Vec::new();

        for cid in large_ids {
            let cluster = match self.clusters.get(&cid) {
                Some(c) => c,
                None => continue,
            };

            let entities: Vec<String> = cluster.entities.iter().cloned().collect();
            if entities.len() < 4 {
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
                if overlap_ratio < 0.7 {
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

                let child = SituationCluster {
                    id: child_id,
                    title: String::new(),
                    entities: entity_group.clone(),
                    topics: child_topics.clone(),
                    event_ids: child_event_ids,
                    region_codes: parent.region_codes.clone(),
                    severity: parent.severity,
                    first_seen: parent.first_seen,
                    last_updated: parent.last_updated,
                    centroid: parent.centroid,
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
            if let Some(emb) = embedding_cache.get(ref_id) {
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

        let new_cluster = SituationCluster {
            id: new_id,
            title: String::new(),
            entities: HashSet::new(),
            topics: HashSet::new(),
            event_ids: split_event_ids.clone(),
            region_codes: parent_region_codes,
            severity: parent_severity,
            first_seen: parent_first_seen,
            last_updated: parent_last_updated,
            centroid: parent_centroid,
            event_count: split_event_count,
            signal_event_count: 0,
            source_types: parent_source_types,
            parent_id: Some(cluster_id),
            event_titles: Vec::new(),
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
        };

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
