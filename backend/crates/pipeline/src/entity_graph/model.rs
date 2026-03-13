use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Entity types tracked in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Organization,
    Location,
    WeaponSystem,
    MilitaryUnit,
    Facility,
}

impl EntityType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Person => "person",
            Self::Organization => "organization",
            Self::Location => "location",
            Self::WeaponSystem => "weapon_system",
            Self::MilitaryUnit => "military_unit",
            Self::Facility => "facility",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "person" => Some(Self::Person),
            "organization" | "org" => Some(Self::Organization),
            "location" => Some(Self::Location),
            "weapon_system" => Some(Self::WeaponSystem),
            "military_unit" => Some(Self::MilitaryUnit),
            "facility" => Some(Self::Facility),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Relationship types between entities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    Leadership,
    Membership,
    Alliance,
    Rivalry,
    GeographicAssociation,
    SupplyChain,
    Family,
    Sponsorship,
}

impl RelationshipType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Leadership => "leadership",
            Self::Membership => "membership",
            Self::Alliance => "alliance",
            Self::Rivalry => "rivalry",
            Self::GeographicAssociation => "geographic_association",
            Self::SupplyChain => "supply_chain",
            Self::Family => "family",
            Self::Sponsorship => "sponsorship",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "leadership" => Some(Self::Leadership),
            "membership" => Some(Self::Membership),
            "alliance" => Some(Self::Alliance),
            "rivalry" => Some(Self::Rivalry),
            "geographic_association" => Some(Self::GeographicAssociation),
            "supply_chain" => Some(Self::SupplyChain),
            "family" => Some(Self::Family),
            "sponsorship" => Some(Self::Sponsorship),
            _ => None,
        }
    }
}

impl std::fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// State change types for entity tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChangeType {
    Killed,
    Arrested,
    Promoted,
    Resigned,
    Sanctioned,
    Relocated,
    Captured,
    Appointed,
    Detained,
}

impl StateChangeType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Killed => "killed",
            Self::Arrested => "arrested",
            Self::Promoted => "promoted",
            Self::Resigned => "resigned",
            Self::Sanctioned => "sanctioned",
            Self::Relocated => "relocated",
            Self::Captured => "captured",
            Self::Appointed => "appointed",
            Self::Detained => "detained",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "killed" | "assassinated" => Some(Self::Killed),
            "arrested" => Some(Self::Arrested),
            "promoted" => Some(Self::Promoted),
            "resigned" => Some(Self::Resigned),
            "sanctioned" => Some(Self::Sanctioned),
            "relocated" => Some(Self::Relocated),
            "captured" => Some(Self::Captured),
            "appointed" => Some(Self::Appointed),
            "detained" => Some(Self::Detained),
            _ => None,
        }
    }

    /// Keywords that trigger state change detection in text.
    pub fn trigger_keywords() -> &'static [(&'static str, StateChangeType)] {
        &[
            ("killed", StateChangeType::Killed),
            ("assassinated", StateChangeType::Killed),
            ("died", StateChangeType::Killed),
            ("detained", StateChangeType::Detained),
            ("captured", StateChangeType::Captured),
            ("arrested", StateChangeType::Arrested),
            ("appointed", StateChangeType::Appointed),
            ("promoted", StateChangeType::Promoted),
            ("resigned", StateChangeType::Resigned),
            ("sanctioned", StateChangeType::Sanctioned),
            ("relocated", StateChangeType::Relocated),
        ]
    }
}

impl std::fmt::Display for StateChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Certainty level for state changes (intelligence assessment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    Confirmed,
    Alleged,
    Denied,
    Rumored,
}

impl Certainty {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Alleged => "alleged",
            Self::Denied => "denied",
            Self::Rumored => "rumored",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "confirmed" => Self::Confirmed,
            "denied" => Self::Denied,
            "rumored" => Self::Rumored,
            _ => Self::Alleged,
        }
    }
}

/// Core entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uuid,
    pub entity_type: EntityType,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub wikidata_id: Option<String>,
    pub properties: serde_json::Value,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub status: String,
    pub confidence: f32,
    pub mention_count: i32,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_enriched_at: Option<DateTime<Utc>>,
}

impl Entity {
    pub fn new(canonical_name: String, entity_type: EntityType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            entity_type,
            canonical_name,
            aliases: Vec::new(),
            wikidata_id: None,
            properties: serde_json::json!({}),
            latitude: None,
            longitude: None,
            status: "active".to_string(),
            confidence: 0.5,
            mention_count: 1,
            first_seen_at: now,
            last_seen_at: now,
            last_enriched_at: None,
        }
    }
}

/// Relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationship {
    pub id: Uuid,
    pub source_entity: Uuid,
    pub target_entity: Uuid,
    pub relationship: RelationshipType,
    pub properties: serde_json::Value,
    pub confidence: f32,
    pub evidence_count: i32,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_active: bool,
}

impl EntityRelationship {
    pub fn new(source: Uuid, target: Uuid, rel_type: RelationshipType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            source_entity: source,
            target_entity: target,
            relationship: rel_type,
            properties: serde_json::json!({}),
            confidence: 0.5,
            evidence_count: 1,
            first_seen_at: now,
            last_seen_at: now,
            is_active: true,
        }
    }
}

/// State change for an entity (e.g., leader killed, official arrested).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityStateChange {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub change_type: StateChangeType,
    pub previous_state: Option<serde_json::Value>,
    pub new_state: serde_json::Value,
    pub certainty: Certainty,
    pub source_reliability: char,
    pub info_credibility: char,
    pub triggering_event_id: Option<Uuid>,
    pub detected_at: DateTime<Utc>,
}

impl EntityStateChange {
    pub fn new(entity_id: Uuid, change_type: StateChangeType, certainty: Certainty) -> Self {
        Self {
            id: Uuid::new_v4(),
            entity_id,
            change_type: change_type.clone(),
            previous_state: None,
            new_state: serde_json::json!({"status": change_type.as_str()}),
            certainty,
            source_reliability: 'F',
            info_credibility: '6',
            triggering_event_id: None,
            detected_at: Utc::now(),
        }
    }
}

/// Extracted entity mention from enrichment (before resolution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub name: String,
    pub entity_type: Option<String>,
    pub wikidata_qid: Option<String>,
    pub role: Option<String>,
}

/// Extracted relationship from enrichment (before resolution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipMention {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub rel_type: String,
    pub confidence: Option<f32>,
}

/// Extracted state change from enrichment (before resolution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChangeMention {
    pub entity: String,
    pub attribute: String,
    pub from: Option<String>,
    pub to: String,
    pub certainty: Option<String>,
}
