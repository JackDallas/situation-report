use std::collections::HashMap;

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use uuid::Uuid;

use super::model::{Entity, EntityRelationship, EntityType, RelationshipType};

/// Impact assessment for an entity reached via graph traversal.
#[derive(Debug, Clone)]
pub struct ImpactAssessment {
    pub affected_entity: Uuid,
    pub entity_name: String,
    pub entity_type: EntityType,
    pub relationship_path: Vec<RelationshipType>,
    pub hops: usize,
    pub impact_score: f64, // 1.0 at source, decays by 0.5 per hop
}

/// Stored metadata for an entity in the graph.
#[derive(Debug, Clone)]
struct EntityMeta {
    name: String,
    entity_type: EntityType,
}

/// In-memory entity graph backed by petgraph StableGraph.
/// Mirrors the database entity_relationships table for fast traversal.
pub struct EntityGraph {
    graph: StableGraph<Uuid, RelationshipType>,
    /// Map from entity UUID to petgraph node index.
    node_map: HashMap<Uuid, NodeIndex>,
    /// Optional metadata for entities (populated via `load` or `add_entity_with_meta`).
    entity_meta: HashMap<Uuid, EntityMeta>,
}

impl EntityGraph {
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            node_map: HashMap::new(),
            entity_meta: HashMap::new(),
        }
    }

    /// Add an entity node to the graph.
    pub fn add_entity(&mut self, entity_id: Uuid) -> NodeIndex {
        if let Some(&idx) = self.node_map.get(&entity_id) {
            return idx;
        }
        let idx = self.graph.add_node(entity_id);
        self.node_map.insert(entity_id, idx);
        idx
    }

    /// Add a relationship edge between two entities.
    pub fn add_relationship(&mut self, source: Uuid, target: Uuid, rel_type: RelationshipType) {
        let src_idx = self.add_entity(source);
        let tgt_idx = self.add_entity(target);
        // Check for existing edge
        if self.graph.find_edge(src_idx, tgt_idx).is_none() {
            self.graph.add_edge(src_idx, tgt_idx, rel_type);
        }
    }

    /// Add an entity node with metadata (name and type).
    pub fn add_entity_with_meta(
        &mut self,
        entity_id: Uuid,
        name: String,
        entity_type: EntityType,
    ) -> NodeIndex {
        self.entity_meta.insert(
            entity_id,
            EntityMeta {
                name,
                entity_type,
            },
        );
        self.add_entity(entity_id)
    }

    /// Load relationships from database on startup.
    pub fn load(&mut self, entities: &[Entity], relationships: &[EntityRelationship]) {
        for entity in entities {
            self.add_entity_with_meta(
                entity.id,
                entity.canonical_name.clone(),
                entity.entity_type.clone(),
            );
        }
        for rel in relationships {
            if rel.is_active {
                self.add_relationship(
                    rel.source_entity,
                    rel.target_entity,
                    rel.relationship.clone(),
                );
            }
        }
    }

    /// Get all entities directly connected to the given entity.
    pub fn neighbors(&self, entity_id: &Uuid) -> Vec<(Uuid, RelationshipType, Direction)> {
        let idx = match self.node_map.get(entity_id) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        // Outgoing edges
        for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
            if let Some(&target_id) = self.graph.node_weight(edge.target()) {
                result.push((target_id, edge.weight().clone(), Direction::Outgoing));
            }
        }
        // Incoming edges
        for edge in self.graph.edges_directed(idx, Direction::Incoming) {
            if let Some(&source_id) = self.graph.node_weight(edge.source()) {
                result.push((source_id, edge.weight().clone(), Direction::Incoming));
            }
        }
        result
    }

    /// Find all entities reachable within `max_hops` from the given entity (BFS).
    pub fn neighborhood(&self, entity_id: &Uuid, max_hops: usize) -> Vec<Uuid> {
        let start_idx = match self.node_map.get(entity_id) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        let mut visited = HashMap::new();
        visited.insert(start_idx, 0usize);
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((start_idx, 0));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }
            for neighbor in self.graph.neighbors_undirected(current) {
                if !visited.contains_key(&neighbor) {
                    visited.insert(neighbor, depth + 1);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }

        visited
            .iter()
            .filter(|&(&idx, _)| idx != start_idx)
            .filter_map(|(&idx, _)| self.graph.node_weight(idx).copied())
            .collect()
    }

    /// Propagate impact from a source entity through the graph using BFS.
    ///
    /// Returns entities reachable within `max_hops`, scored by distance decay (0.5^hops).
    /// Only includes entities of type Location, Facility, Organization, or MilitaryUnit.
    /// Results are sorted by impact_score descending and capped at 20.
    pub fn propagate_impact(&self, source_entity: Uuid, max_hops: usize) -> Vec<ImpactAssessment> {
        let start_idx = match self.node_map.get(&source_entity) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        // BFS tracking: node index -> (depth, relationship path to reach it)
        let mut visited: HashMap<NodeIndex, (usize, Vec<RelationshipType>)> = HashMap::new();
        visited.insert(start_idx, (0, Vec::new()));

        let mut queue = std::collections::VecDeque::new();
        queue.push_back((start_idx, 0usize, Vec::<RelationshipType>::new()));

        while let Some((current, depth, path)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }
            // Traverse outgoing edges
            for edge in self.graph.edges_directed(current, Direction::Outgoing) {
                let neighbor = edge.target();
                if !visited.contains_key(&neighbor) {
                    let mut new_path = path.clone();
                    new_path.push(edge.weight().clone());
                    visited.insert(neighbor, (depth + 1, new_path.clone()));
                    queue.push_back((neighbor, depth + 1, new_path));
                }
            }
            // Traverse incoming edges
            for edge in self.graph.edges_directed(current, Direction::Incoming) {
                let neighbor = edge.source();
                if !visited.contains_key(&neighbor) {
                    let mut new_path = path.clone();
                    new_path.push(edge.weight().clone());
                    visited.insert(neighbor, (depth + 1, new_path.clone()));
                    queue.push_back((neighbor, depth + 1, new_path));
                }
            }
        }

        // Entity types eligible for impact assessment
        let eligible = |et: &EntityType| {
            matches!(
                et,
                EntityType::Location
                    | EntityType::Facility
                    | EntityType::Organization
                    | EntityType::MilitaryUnit
            )
        };

        let mut assessments: Vec<ImpactAssessment> = visited
            .iter()
            .filter(|(idx, _)| **idx != start_idx)
            .filter_map(|(idx, (hops, path))| {
                let idx = *idx;
                let entity_id = self.graph.node_weight(idx).copied()?;
                let meta = self.entity_meta.get(&entity_id)?;
                if !eligible(&meta.entity_type) {
                    return None;
                }
                Some(ImpactAssessment {
                    affected_entity: entity_id,
                    entity_name: meta.name.clone(),
                    entity_type: meta.entity_type.clone(),
                    relationship_path: path.clone(),
                    hops: *hops,
                    impact_score: 0.5_f64.powi(*hops as i32),
                })
            })
            .collect();

        assessments.sort_by(|a, b| {
            b.impact_score
                .partial_cmp(&a.impact_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assessments.truncate(20);
        assessments
    }

    /// Format impact assessments into a human-readable summary string.
    ///
    /// Groups by direct (hop=1) and secondary (hop>=2) impacts.
    /// Returns empty string if there are no assessments.
    pub fn format_impact_summary(&self, assessments: &[ImpactAssessment]) -> String {
        if assessments.is_empty() {
            return String::new();
        }

        let direct: Vec<&ImpactAssessment> = assessments.iter().filter(|a| a.hops == 1).collect();
        let secondary: Vec<&ImpactAssessment> =
            assessments.iter().filter(|a| a.hops >= 2).collect();

        let format_entity = |a: &&ImpactAssessment| -> String {
            format!("{} ({})", a.entity_name, a.entity_type.as_str())
        };

        let mut parts = Vec::new();

        if !direct.is_empty() {
            let names: Vec<String> = direct.iter().map(format_entity).collect();
            parts.push(format!("Directly affects: {}", names.join(", ")));
        }

        if !secondary.is_empty() {
            let names: Vec<String> = secondary.iter().map(format_entity).collect();
            parts.push(format!("Secondary impact: {}", names.join(", ")));
        }

        format!("{}.", parts.join(". "))
    }

    /// Number of entity nodes.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of relationship edges.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Check if an entity exists in the graph.
    pub fn contains(&self, entity_id: &Uuid) -> bool {
        self.node_map.contains_key(entity_id)
    }
}

impl Default for EntityGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_query_neighbors() {
        let mut graph = EntityGraph::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        graph.add_relationship(a, b, RelationshipType::Alliance);
        graph.add_relationship(b, c, RelationshipType::Leadership);

        let neighbors = graph.neighbors(&a);
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0, b);

        let neighbors_b = graph.neighbors(&b);
        assert_eq!(neighbors_b.len(), 2); // a (incoming) + c (outgoing)
    }

    #[test]
    fn test_bfs_neighborhood() {
        let mut graph = EntityGraph::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let d = Uuid::new_v4();

        graph.add_relationship(a, b, RelationshipType::Alliance);
        graph.add_relationship(b, c, RelationshipType::Membership);
        graph.add_relationship(c, d, RelationshipType::Leadership);

        // 1 hop from a -> should find b
        let n1 = graph.neighborhood(&a, 1);
        assert_eq!(n1.len(), 1);

        // 2 hops from a -> should find b, c
        let n2 = graph.neighborhood(&a, 2);
        assert_eq!(n2.len(), 2);

        // 3 hops from a -> should find b, c, d
        let n3 = graph.neighborhood(&a, 3);
        assert_eq!(n3.len(), 3);
    }

    #[test]
    fn test_no_duplicate_edges() {
        let mut graph = EntityGraph::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        graph.add_relationship(a, b, RelationshipType::Alliance);
        graph.add_relationship(a, b, RelationshipType::Alliance);

        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_impact_propagation_two_hops() {
        use crate::entity_graph::model::EntityType;

        let mut graph = EntityGraph::new();

        // Create entities: Person -> Organization -> Facility -> Location
        let person_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let facility_id = Uuid::new_v4();
        let location_id = Uuid::new_v4();

        graph.add_entity_with_meta(
            person_id,
            "Commander Alpha".into(),
            EntityType::Person,
        );
        graph.add_entity_with_meta(
            org_id,
            "Northern Division".into(),
            EntityType::Organization,
        );
        graph.add_entity_with_meta(
            facility_id,
            "Base Omega".into(),
            EntityType::Facility,
        );
        graph.add_entity_with_meta(
            location_id,
            "Kharkiv Oblast".into(),
            EntityType::Location,
        );

        graph.add_relationship(person_id, org_id, RelationshipType::Leadership);
        graph.add_relationship(org_id, facility_id, RelationshipType::GeographicAssociation);
        graph.add_relationship(facility_id, location_id, RelationshipType::GeographicAssociation);

        // Propagate from person with max_hops=2
        let assessments = graph.propagate_impact(person_id, 2);

        // Person is filtered out (source). Organization at hop 1, Facility at hop 2.
        // Location is at hop 3 so excluded by max_hops=2.
        // Person type is also excluded from results.
        assert_eq!(assessments.len(), 2);

        // Sorted by impact_score descending: hop 1 first, then hop 2
        assert_eq!(assessments[0].affected_entity, org_id);
        assert_eq!(assessments[0].hops, 1);
        assert!((assessments[0].impact_score - 0.5).abs() < f64::EPSILON);
        assert_eq!(assessments[0].relationship_path.len(), 1);
        assert_eq!(
            assessments[0].relationship_path[0],
            RelationshipType::Leadership
        );

        assert_eq!(assessments[1].affected_entity, facility_id);
        assert_eq!(assessments[1].hops, 2);
        assert!((assessments[1].impact_score - 0.25).abs() < f64::EPSILON);
        assert_eq!(assessments[1].relationship_path.len(), 2);

        // Now test with max_hops=3 — should also pick up location
        let assessments_3 = graph.propagate_impact(person_id, 3);
        assert_eq!(assessments_3.len(), 3);
        assert_eq!(assessments_3[2].affected_entity, location_id);
        assert!((assessments_3[2].impact_score - 0.125).abs() < f64::EPSILON);

        // Test format_impact_summary
        let summary = graph.format_impact_summary(&assessments);
        assert!(summary.contains("Directly affects:"));
        assert!(summary.contains("Northern Division (organization)"));
        assert!(summary.contains("Secondary impact:"));
        assert!(summary.contains("Base Omega (facility)"));
        assert!(summary.ends_with('.'));

        // Empty assessments returns empty string
        let empty_summary = graph.format_impact_summary(&[]);
        assert!(empty_summary.is_empty());
    }
}
