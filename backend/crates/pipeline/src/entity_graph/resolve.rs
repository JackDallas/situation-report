use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::model::{Entity, EntityMention, EntityType};

/// Entity resolver that matches mentions to canonical entities.
pub struct EntityResolver {
    /// Name -> entity ID index for fast lookup.
    name_index: HashMap<String, Uuid>,
    /// Canonical entities by ID.
    entities: HashMap<Uuid, Entity>,
}

impl EntityResolver {
    pub fn new() -> Self {
        Self {
            name_index: HashMap::new(),
            entities: HashMap::new(),
        }
    }

    /// Load entities from database on startup.
    pub fn load(&mut self, entities: Vec<Entity>) {
        for entity in entities {
            let normalized = normalize_name(&entity.canonical_name);
            self.name_index.insert(normalized, entity.id);
            for alias in &entity.aliases {
                let norm_alias = normalize_name(alias);
                self.name_index.insert(norm_alias, entity.id);
            }
            self.entities.insert(entity.id, entity);
        }
    }

    /// Resolve an entity mention to an existing entity or create a new one.
    /// Returns the entity ID and whether it was newly created.
    pub fn resolve(&mut self, mention: &EntityMention) -> (Uuid, bool) {
        let normalized = normalize_name(&mention.name);

        // Layer 1: Exact match on normalized name / alias
        if let Some(&id) = self.name_index.get(&normalized) {
            // Update mention count and last_seen
            if let Some(entity) = self.entities.get_mut(&id) {
                entity.mention_count += 1;
                entity.last_seen_at = chrono::Utc::now();
            }
            return (id, false);
        }

        // Layer 2: Trigram similarity check (substring / fuzzy)
        // Check if any existing name is a substring or vice versa
        if let Some(id) = self.fuzzy_match(&normalized) {
            // Add as alias
            if let Some(entity) = self.entities.get_mut(&id) {
                entity.aliases.push(mention.name.clone());
                entity.mention_count += 1;
                entity.last_seen_at = chrono::Utc::now();
            }
            self.name_index.insert(normalized, id);
            return (id, false);
        }

        // Layer 3: Wikidata QID match
        if let Some(ref qid) = mention.wikidata_qid {
            if let Some(id) = self.find_by_wikidata(qid) {
                if let Some(entity) = self.entities.get_mut(&id) {
                    entity.aliases.push(mention.name.clone());
                    entity.mention_count += 1;
                    entity.last_seen_at = chrono::Utc::now();
                }
                self.name_index.insert(normalized, id);
                return (id, false);
            }
        }

        // Layer 4: Create new entity
        let entity_type = mention
            .entity_type
            .as_deref()
            .and_then(EntityType::from_str)
            .unwrap_or(EntityType::Organization);

        let mut entity = Entity::new(mention.name.clone(), entity_type);
        if let Some(ref qid) = mention.wikidata_qid {
            entity.wikidata_id = Some(qid.clone());
        }
        let id = entity.id;
        self.name_index.insert(normalized, id);
        self.entities.insert(id, entity);
        (id, true)
    }

    /// Get an entity by ID.
    pub fn get(&self, id: &Uuid) -> Option<&Entity> {
        self.entities.get(id)
    }

    /// Find an entity by name using the name_index (O(1) lookup).
    pub fn find_by_name(&self, name: &str) -> Option<&Entity> {
        let normalized = normalize_name(name);
        let id = self.name_index.get(&normalized)?;
        self.entities.get(id)
    }

    /// Get all entities.
    pub fn all_entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// Number of tracked entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Fuzzy substring match against existing entity names.
    fn fuzzy_match(&self, normalized: &str) -> Option<Uuid> {
        // Simple substring matching for high-value matches
        // (e.g., "Hezbollah" matches "Lebanese Hezbollah")
        if normalized.len() < 4 {
            return None;
        }

        for (name, &id) in &self.name_index {
            // Skip very short names to avoid false positives
            if name.len() < 4 {
                continue;
            }
            // Bidirectional substring check
            if name.contains(normalized) || normalized.contains(name.as_str()) {
                // Require significant overlap (at least 60% of shorter string)
                let shorter = name.len().min(normalized.len());
                let longer = name.len().max(normalized.len());
                if shorter as f64 / longer as f64 > 0.5 {
                    return Some(id);
                }
            }
            // Trigram similarity: count shared 3-char sequences
            let sim = trigram_similarity(name, normalized);
            if sim > 0.6 {
                return Some(id);
            }
        }
        None
    }

    /// Find entity by Wikidata QID.
    fn find_by_wikidata(&self, qid: &str) -> Option<Uuid> {
        self.entities
            .values()
            .find(|e| e.wikidata_id.as_deref() == Some(qid))
            .map(|e| e.id)
    }
}

impl Default for EntityResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize entity name for matching: Unicode NFC, lowercase, strip diacritics,
/// collapse whitespace, remove common suffixes.
pub fn normalize_name(name: &str) -> String {
    let mut s: String = name.nfc().collect();
    s = s.to_lowercase();
    // Collapse whitespace
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Strip common suffixes
    for suffix in &[
        " ltd", " llc", " co.", " inc", " corp", " gmbh", " s.a.", " sa", " ag", " plc", " pty",
        " bv",
    ] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
        }
    }
    s.trim().to_string()
}

/// Simple trigram similarity between two strings (Jaccard coefficient of 3-grams).
fn trigram_similarity(a: &str, b: &str) -> f64 {
    if a.len() < 3 || b.len() < 3 {
        return 0.0;
    }

    let trigrams_a: std::collections::HashSet<&str> = (0..a.len().saturating_sub(2))
        .filter_map(|i| a.get(i..i + 3))
        .collect();
    let trigrams_b: std::collections::HashSet<&str> = (0..b.len().saturating_sub(2))
        .filter_map(|i| b.get(i..i + 3))
        .collect();

    let intersection = trigrams_a.intersection(&trigrams_b).count();
    let union = trigrams_a.union(&trigrams_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("Hezbollah"), "hezbollah");
        assert_eq!(normalize_name("  IRGC  Corp  "), "irgc");
        assert_eq!(normalize_name("Company Ltd"), "company");
    }

    #[test]
    fn test_resolve_exact_match() {
        let mut resolver = EntityResolver::new();
        let mention1 = EntityMention {
            name: "Hezbollah".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: None,
            role: None,
        };
        let (id1, created1) = resolver.resolve(&mention1);
        assert!(created1);

        let mention2 = EntityMention {
            name: "hezbollah".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: None,
            role: None,
        };
        let (id2, created2) = resolver.resolve(&mention2);
        assert!(!created2);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_resolve_creates_new_for_different_entities() {
        let mut resolver = EntityResolver::new();
        let m1 = EntityMention {
            name: "Hamas".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: None,
            role: None,
        };
        let m2 = EntityMention {
            name: "IRGC".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: None,
            role: None,
        };
        let (id1, _) = resolver.resolve(&m1);
        let (id2, _) = resolver.resolve(&m2);
        assert_ne!(id1, id2);
        assert_eq!(resolver.len(), 2);
    }

    #[test]
    fn test_trigram_similarity() {
        let sim = trigram_similarity("hezbollah", "hizballah");
        assert!(
            sim > 0.1,
            "Similar transliterations should have nonzero similarity: {sim}"
        );

        let sim2 = trigram_similarity("russia", "france");
        assert!(
            sim2 < 0.3,
            "Different names should have low similarity: {sim2}"
        );
    }

    #[test]
    fn test_wikidata_resolution() {
        let mut resolver = EntityResolver::new();
        let m1 = EntityMention {
            name: "Hezbollah".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: Some("Q41548".to_string()),
            role: None,
        };
        let (id1, _) = resolver.resolve(&m1);

        // Different name but same Wikidata QID
        let m2 = EntityMention {
            name: "\u{62d}\u{632}\u{628} \u{627}\u{644}\u{644}\u{647}".to_string(),
            entity_type: Some("organization".to_string()),
            wikidata_qid: Some("Q41548".to_string()),
            role: None,
        };
        let (id2, created) = resolver.resolve(&m2);
        assert!(
            !created,
            "Should resolve to existing entity via Wikidata QID"
        );
        assert_eq!(id1, id2);
    }
}
