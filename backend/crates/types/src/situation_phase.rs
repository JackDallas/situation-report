use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lifecycle phase of a situation cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum SituationPhase {
    Emerging,
    Developing,
    Active,
    Declining,
    Resolved,
    Historical,
}

impl SituationPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Emerging => "emerging",
            Self::Developing => "developing",
            Self::Active => "active",
            Self::Declining => "declining",
            Self::Resolved => "resolved",
            Self::Historical => "historical",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "emerging" => Self::Emerging,
            "developing" => Self::Developing,
            "active" => Self::Active,
            "declining" => Self::Declining,
            "resolved" => Self::Resolved,
            "historical" => Self::Historical,
            _ => Self::Emerging,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for SituationPhase {
    fn default() -> Self {
        Self::Emerging
    }
}

impl std::fmt::Display for SituationPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All variants of [`SituationPhase`] for exhaustive testing.
pub const ALL_SITUATION_PHASES: [SituationPhase; 6] = [
    SituationPhase::Emerging,
    SituationPhase::Developing,
    SituationPhase::Active,
    SituationPhase::Declining,
    SituationPhase::Resolved,
    SituationPhase::Historical,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn roundtrip_serde() {
        for phase in ALL_SITUATION_PHASES {
            let json = serde_json::to_string(&phase).unwrap();
            let back: SituationPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(phase, back);
        }
    }

    #[test]
    fn from_str_lossy_covers_all_variants() {
        for phase in ALL_SITUATION_PHASES {
            assert_eq!(
                SituationPhase::from_str_lossy(phase.as_str()),
                phase,
                "from_str_lossy({:?}) should roundtrip",
                phase.as_str(),
            );
        }
    }

    #[test]
    fn display_matches_as_str() {
        for phase in ALL_SITUATION_PHASES {
            assert_eq!(phase.to_string(), phase.as_str());
        }
    }

    #[test]
    fn all_situation_phases_have_unique_str() {
        let strs: Vec<&str> = ALL_SITUATION_PHASES.iter().map(|p| p.as_str()).collect();
        let unique: HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(strs.len(), unique.len(), "Duplicate as_str() values found");
    }

    #[test]
    fn serde_json_string_matches_as_str() {
        for phase in ALL_SITUATION_PHASES {
            let json = serde_json::to_string(&phase).unwrap();
            let expected = format!("\"{}\"", phase.as_str());
            assert_eq!(json, expected, "serde JSON for {:?} should match as_str()", phase);
        }
    }

    #[test]
    fn from_str_lossy_unknown_returns_default() {
        assert_eq!(SituationPhase::from_str_lossy("unknown_garbage"), SituationPhase::Emerging);
        assert_eq!(SituationPhase::from_str_lossy(""), SituationPhase::Emerging);
    }

    #[test]
    fn variant_count() {
        assert_eq!(ALL_SITUATION_PHASES.len(), 6, "Expected 6 situation phase variants");
    }
}
