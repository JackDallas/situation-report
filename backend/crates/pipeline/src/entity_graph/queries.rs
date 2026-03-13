use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::model::{
    Certainty, Entity, EntityRelationship, EntityStateChange, EntityType, RelationshipType,
    StateChangeType,
};

/// Insert or update an entity in the database.
pub async fn upsert_entity(pool: &PgPool, entity: &Entity) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO entities (id, entity_type, canonical_name, aliases, wikidata_id,
            properties, status, confidence, mention_count, first_seen_at, last_seen_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (id) DO UPDATE SET
            canonical_name = EXCLUDED.canonical_name,
            aliases = EXCLUDED.aliases,
            wikidata_id = COALESCE(EXCLUDED.wikidata_id, entities.wikidata_id),
            mention_count = entities.mention_count + 1,
            last_seen_at = EXCLUDED.last_seen_at,
            confidence = GREATEST(entities.confidence, EXCLUDED.confidence)
        "#,
    )
    .bind(entity.id)
    .bind(entity.entity_type.as_str())
    .bind(&entity.canonical_name)
    .bind(&entity.aliases)
    .bind(&entity.wikidata_id)
    .bind(&entity.properties)
    .bind(&entity.status)
    .bind(entity.confidence)
    .bind(entity.mention_count)
    .bind(entity.first_seen_at)
    .bind(entity.last_seen_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a relationship between two entities.
pub async fn upsert_relationship(pool: &PgPool, rel: &EntityRelationship) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO entity_relationships (id, source_entity, target_entity, relationship,
            properties, confidence, evidence_count, first_seen_at, last_seen_at, is_active)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (source_entity, target_entity, relationship) DO UPDATE SET
            evidence_count = entity_relationships.evidence_count + 1,
            last_seen_at = EXCLUDED.last_seen_at,
            confidence = GREATEST(entity_relationships.confidence, EXCLUDED.confidence)
        "#,
    )
    .bind(rel.id)
    .bind(rel.source_entity)
    .bind(rel.target_entity)
    .bind(rel.relationship.as_str())
    .bind(&rel.properties)
    .bind(rel.confidence)
    .bind(rel.evidence_count)
    .bind(rel.first_seen_at)
    .bind(rel.last_seen_at)
    .bind(rel.is_active)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert an entity state change.
pub async fn insert_state_change(pool: &PgPool, change: &EntityStateChange) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO entity_state_changes (id, entity_id, change_type, previous_state,
            new_state, certainty, source_reliability, info_credibility,
            triggering_event_id, detected_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(change.id)
    .bind(change.entity_id)
    .bind(change.change_type.as_str())
    .bind(&change.previous_state)
    .bind(&change.new_state)
    .bind(change.certainty.as_str())
    .bind(change.source_reliability.to_string())
    .bind(change.info_credibility.to_string())
    .bind(change.triggering_event_id)
    .bind(change.detected_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Link an entity to an event via the event_entities join table.
/// Uses the events table composite key (source_type, source_id, event_time).
pub async fn link_event_entity(
    pool: &PgPool,
    source_type: &str,
    source_id: &str,
    event_time: DateTime<Utc>,
    entity_id: Uuid,
    role: &str,
    confidence: f32,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO event_entities (source_type, source_id, event_time, entity_id, role, confidence)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (source_type, source_id, event_time, entity_id) DO UPDATE SET
            role = EXCLUDED.role,
            confidence = GREATEST(event_entities.confidence, EXCLUDED.confidence)
        "#,
    )
    .bind(source_type)
    .bind(source_id)
    .bind(event_time)
    .bind(entity_id)
    .bind(role)
    .bind(confidence)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load all active entities from the database.
pub async fn load_all_entities(pool: &PgPool) -> Result<Vec<Entity>> {
    let rows = sqlx::query_as::<_, EntityRow>(
        "SELECT id, entity_type, canonical_name, aliases, wikidata_id, properties,
         status, confidence, mention_count, first_seen_at, last_seen_at, last_enriched_at
         FROM entities WHERE status = 'active' ORDER BY mention_count DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Load all active relationships from the database.
pub async fn load_all_relationships(pool: &PgPool) -> Result<Vec<EntityRelationship>> {
    let rows = sqlx::query_as::<_, RelationshipRow>(
        "SELECT id, source_entity, target_entity, relationship, properties,
         confidence, evidence_count, first_seen_at, last_seen_at, is_active
         FROM entity_relationships WHERE is_active = true",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Get top entities by mention count.
pub async fn get_top_entities(pool: &PgPool, limit: i64) -> Result<Vec<Entity>> {
    let rows = sqlx::query_as::<_, EntityRow>(
        "SELECT id, entity_type, canonical_name, aliases, wikidata_id, properties,
         status, confidence, mention_count, first_seen_at, last_seen_at, last_enriched_at
         FROM entities WHERE status = 'active'
         ORDER BY mention_count DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Get entity by ID with its relationships and recent state changes.
pub async fn get_entity_detail(
    pool: &PgPool,
    entity_id: Uuid,
) -> Result<Option<EntityDetail>> {
    let entity_row = sqlx::query_as::<_, EntityRow>(
        "SELECT id, entity_type, canonical_name, aliases, wikidata_id, properties,
         status, confidence, mention_count, first_seen_at, last_seen_at, last_enriched_at
         FROM entities WHERE id = $1",
    )
    .bind(entity_id)
    .fetch_optional(pool)
    .await?;

    let entity: Entity = match entity_row {
        Some(r) => r.into(),
        None => return Ok(None),
    };

    let relationships = sqlx::query_as::<_, RelationshipRow>(
        "SELECT id, source_entity, target_entity, relationship, properties,
         confidence, evidence_count, first_seen_at, last_seen_at, is_active
         FROM entity_relationships
         WHERE (source_entity = $1 OR target_entity = $1) AND is_active = true
         ORDER BY evidence_count DESC",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| r.into())
    .collect();

    let state_changes = sqlx::query_as::<_, StateChangeRow>(
        "SELECT id, entity_id, change_type, previous_state, new_state,
         certainty, source_reliability, info_credibility, triggering_event_id, detected_at
         FROM entity_state_changes WHERE entity_id = $1
         ORDER BY detected_at DESC LIMIT 20",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| r.into())
    .collect();

    Ok(Some(EntityDetail {
        entity,
        relationships,
        state_changes,
    }))
}

/// Get recent state changes across all entities.
pub async fn get_recent_state_changes(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<EntityStateChange>> {
    let rows = sqlx::query_as::<_, StateChangeRow>(
        "SELECT id, entity_id, change_type, previous_state, new_state,
         certainty, source_reliability, info_credibility, triggering_event_id, detected_at
         FROM entity_state_changes ORDER BY detected_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

// ---------------------------------------------------------------------------
// Row types for sqlx mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct EntityRow {
    id: Uuid,
    entity_type: String,
    canonical_name: String,
    aliases: Vec<String>,
    wikidata_id: Option<String>,
    properties: serde_json::Value,
    status: String,
    confidence: f32,
    mention_count: i32,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    last_enriched_at: Option<DateTime<Utc>>,
}

impl From<EntityRow> for Entity {
    fn from(r: EntityRow) -> Self {
        Entity {
            id: r.id,
            entity_type: EntityType::from_str(&r.entity_type).unwrap_or(EntityType::Organization),
            canonical_name: r.canonical_name,
            aliases: r.aliases,
            wikidata_id: r.wikidata_id,
            properties: r.properties,
            latitude: None,
            longitude: None,
            status: r.status,
            confidence: r.confidence,
            mention_count: r.mention_count,
            first_seen_at: r.first_seen_at,
            last_seen_at: r.last_seen_at,
            last_enriched_at: r.last_enriched_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct RelationshipRow {
    id: Uuid,
    source_entity: Uuid,
    target_entity: Uuid,
    relationship: String,
    properties: serde_json::Value,
    confidence: f32,
    evidence_count: i32,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    is_active: bool,
}

impl From<RelationshipRow> for EntityRelationship {
    fn from(r: RelationshipRow) -> Self {
        EntityRelationship {
            id: r.id,
            source_entity: r.source_entity,
            target_entity: r.target_entity,
            relationship: RelationshipType::from_str(&r.relationship)
                .unwrap_or(RelationshipType::Alliance),
            properties: r.properties,
            confidence: r.confidence,
            evidence_count: r.evidence_count,
            first_seen_at: r.first_seen_at,
            last_seen_at: r.last_seen_at,
            is_active: r.is_active,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StateChangeRow {
    id: Uuid,
    entity_id: Uuid,
    change_type: String,
    previous_state: Option<serde_json::Value>,
    new_state: serde_json::Value,
    certainty: String,
    source_reliability: String,
    info_credibility: String,
    triggering_event_id: Option<Uuid>,
    detected_at: DateTime<Utc>,
}

impl From<StateChangeRow> for EntityStateChange {
    fn from(r: StateChangeRow) -> Self {
        EntityStateChange {
            id: r.id,
            entity_id: r.entity_id,
            change_type: StateChangeType::from_str(&r.change_type)
                .unwrap_or(StateChangeType::Killed),
            previous_state: r.previous_state,
            new_state: r.new_state,
            certainty: Certainty::from_str(&r.certainty),
            source_reliability: r.source_reliability.chars().next().unwrap_or('F'),
            info_credibility: r.info_credibility.chars().next().unwrap_or('6'),
            triggering_event_id: r.triggering_event_id,
            detected_at: r.detected_at,
        }
    }
}

/// Full entity detail with relationships and state changes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EntityDetail {
    pub entity: Entity,
    pub relationships: Vec<EntityRelationship>,
    pub state_changes: Vec<EntityStateChange>,
}
