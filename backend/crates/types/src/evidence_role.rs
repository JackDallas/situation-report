use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Role of an evidence item within a correlated incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRole {
    Trigger,
    Corroboration,
    Context,
}

impl EvidenceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trigger => "trigger",
            Self::Corroboration => "corroboration",
            Self::Context => "context",
        }
    }
}

impl std::fmt::Display for EvidenceRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All variants of [`EvidenceRole`] for exhaustive testing.
pub const ALL_EVIDENCE_ROLES: [EvidenceRole; 3] = [
    EvidenceRole::Trigger,
    EvidenceRole::Corroboration,
    EvidenceRole::Context,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn roundtrip_serde() {
        for role in ALL_EVIDENCE_ROLES {
            let json = serde_json::to_string(&role).unwrap();
            let back: EvidenceRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn display_matches_as_str() {
        for role in ALL_EVIDENCE_ROLES {
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    #[test]
    fn all_evidence_roles_have_unique_str() {
        let strs: Vec<&str> = ALL_EVIDENCE_ROLES.iter().map(|r| r.as_str()).collect();
        let unique: HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(strs.len(), unique.len(), "Duplicate as_str() values found");
    }

    #[test]
    fn serde_json_string_matches_as_str() {
        for role in ALL_EVIDENCE_ROLES {
            let json = serde_json::to_string(&role).unwrap();
            let expected = format!("\"{}\"", role.as_str());
            assert_eq!(json, expected, "serde JSON for {:?} should match as_str()", role);
        }
    }

    #[test]
    fn variant_count() {
        assert_eq!(ALL_EVIDENCE_ROLES.len(), 3, "Expected 3 evidence role variants");
    }
}
