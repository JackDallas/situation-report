use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Event severity level, ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "critical" => Self::Critical,
            "high" => Self::High,
            "medium" | "warning" => Self::Medium,
            "low" => Self::Low,
            "info" => Self::Info,
            _ => Self::Low,
        }
    }

    pub fn max(self, other: Self) -> Self {
        if self.rank() >= other.rank() { self } else { other }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[allow(clippy::derivable_impls)]
impl Default for Severity {
    fn default() -> Self {
        Self::Low
    }
}

/// All variants of [`Severity`] for exhaustive testing.
pub const ALL_SEVERITIES: [Severity; 5] = [
    Severity::Info,
    Severity::Low,
    Severity::Medium,
    Severity::High,
    Severity::Critical,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Info);
    }

    #[test]
    fn roundtrip_serde() {
        for sev in ALL_SEVERITIES {
            let json = serde_json::to_string(&sev).unwrap();
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(sev, back);
        }
    }

    #[test]
    fn from_str_lossy_handles_warning() {
        assert_eq!(Severity::from_str_lossy("warning"), Severity::Medium);
        assert_eq!(Severity::from_str_lossy("garbage"), Severity::Low);
    }

    #[test]
    fn max_works() {
        assert_eq!(Severity::Low.max(Severity::High), Severity::High);
        assert_eq!(Severity::Critical.max(Severity::Low), Severity::Critical);
    }

    #[test]
    fn all_severities_have_distinct_ranks() {
        let ranks: Vec<u8> = ALL_SEVERITIES.iter().map(|s| s.rank()).collect();
        // Verify strictly increasing
        for (i, w) in ranks.windows(2).enumerate() {
            assert!(
                w[0] < w[1],
                "{:?} (rank {}) should have lower rank than {:?} (rank {})",
                ALL_SEVERITIES[i],
                w[0],
                ALL_SEVERITIES[i + 1],
                w[1],
            );
        }
    }

    #[test]
    fn display_matches_as_str() {
        for sev in ALL_SEVERITIES {
            assert_eq!(sev.to_string(), sev.as_str());
        }
    }

    #[test]
    fn from_str_lossy_covers_all_variants() {
        for sev in ALL_SEVERITIES {
            assert_eq!(
                Severity::from_str_lossy(sev.as_str()),
                sev,
                "from_str_lossy({:?}) should roundtrip",
                sev.as_str(),
            );
        }
    }

    #[test]
    fn serde_json_string_matches_as_str() {
        // Verify that serde's rename_all = "snake_case" produces the same strings as as_str()
        for sev in ALL_SEVERITIES {
            let json = serde_json::to_string(&sev).unwrap();
            let expected = format!("\"{}\"", sev.as_str());
            assert_eq!(json, expected, "serde JSON for {:?} should match as_str()", sev);
        }
    }

    #[test]
    fn variant_count() {
        assert_eq!(ALL_SEVERITIES.len(), 5, "Expected 5 severity variants");
    }
}
