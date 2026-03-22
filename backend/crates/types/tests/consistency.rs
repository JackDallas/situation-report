//! Cross-crate consistency tests for sr-types.
//!
//! These tests verify that enum variant counts haven't changed unexpectedly,
//! catching cases where someone adds a source but forgets to update the
//! corresponding type enum (or vice versa).

use sr_types::*;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Variant count guards
// ---------------------------------------------------------------------------

#[test]
fn severity_variant_count() {
    assert_eq!(
        ALL_SEVERITIES.len(),
        5,
        "Severity variant count changed! Update ALL_SEVERITIES and this test.",
    );
}

#[test]
fn event_type_variant_count() {
    assert_eq!(
        ALL_EVENT_TYPES.len(),
        24,
        "EventType variant count changed! Update ALL_EVENT_TYPES and this test.",
    );
}

#[test]
fn source_type_variant_count() {
    assert_eq!(
        ALL_SOURCE_TYPES.len(),
        31,
        "SourceType variant count changed! Update ALL_SOURCE_TYPES and this test.",
    );
}

#[test]
fn evidence_role_variant_count() {
    assert_eq!(
        ALL_EVIDENCE_ROLES.len(),
        3,
        "EvidenceRole variant count changed! Update ALL_EVIDENCE_ROLES and this test.",
    );
}

#[test]
fn situation_phase_variant_count() {
    assert_eq!(
        ALL_SITUATION_PHASES.len(),
        6,
        "SituationPhase variant count changed! Update ALL_SITUATION_PHASES and this test.",
    );
}

// ---------------------------------------------------------------------------
// Cross-enum uniqueness: no two enums should produce the same as_str() values
// ---------------------------------------------------------------------------

#[test]
fn no_as_str_collisions_across_enums() {
    let mut all_strings: Vec<(&str, &str)> = Vec::new();

    for v in ALL_SEVERITIES {
        all_strings.push(("Severity", v.as_str()));
    }
    for v in ALL_EVENT_TYPES {
        all_strings.push(("EventType", v.as_str()));
    }
    for v in ALL_SOURCE_TYPES {
        all_strings.push(("SourceType", v.as_str()));
    }
    for v in ALL_EVIDENCE_ROLES {
        all_strings.push(("EvidenceRole", v.as_str()));
    }
    for v in ALL_SITUATION_PHASES {
        all_strings.push(("SituationPhase", v.as_str()));
    }

    // Check for collisions within each enum (already tested per-module, but belt-and-suspenders)
    let enum_names = ["Severity", "EventType", "SourceType", "EvidenceRole", "SituationPhase"];
    for name in enum_names {
        let strs: Vec<&str> = all_strings
            .iter()
            .filter(|(e, _)| *e == name)
            .map(|(_, s)| *s)
            .collect();
        let unique: HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(
            strs.len(),
            unique.len(),
            "Duplicate as_str() values found within {}",
            name,
        );
    }
}

// ---------------------------------------------------------------------------
// All serde roundtrips via the public re-exports
// ---------------------------------------------------------------------------

#[test]
fn all_types_serde_roundtrip_from_public_api() {
    for sev in ALL_SEVERITIES {
        let json = serde_json::to_string(&sev).unwrap();
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(sev, back);
    }

    for et in ALL_EVENT_TYPES {
        let json = serde_json::to_string(&et).unwrap();
        let back: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(et, back);
    }

    for st in ALL_SOURCE_TYPES {
        let json = serde_json::to_string(&st).unwrap();
        let back: SourceType = serde_json::from_str(&json).unwrap();
        assert_eq!(st, back);
    }

    for role in ALL_EVIDENCE_ROLES {
        let json = serde_json::to_string(&role).unwrap();
        let back: EvidenceRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }

    for phase in ALL_SITUATION_PHASES {
        let json = serde_json::to_string(&phase).unwrap();
        let back: SituationPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(phase, back);
    }
}

// ---------------------------------------------------------------------------
// Display/as_str consistency across all enums
// ---------------------------------------------------------------------------

#[test]
fn all_types_display_matches_as_str() {
    for sev in ALL_SEVERITIES {
        assert_eq!(sev.to_string(), sev.as_str());
    }
    for et in ALL_EVENT_TYPES {
        assert_eq!(et.to_string(), et.as_str());
    }
    for st in ALL_SOURCE_TYPES {
        assert_eq!(st.to_string(), st.as_str());
    }
    for role in ALL_EVIDENCE_ROLES {
        assert_eq!(role.to_string(), role.as_str());
    }
    for phase in ALL_SITUATION_PHASES {
        assert_eq!(phase.to_string(), phase.as_str());
    }
}

// ---------------------------------------------------------------------------
// as_str() values are valid snake_case or kebab-case identifiers
// ---------------------------------------------------------------------------

#[test]
fn as_str_values_are_valid_identifiers() {
    let check = |name: &str, s: &str| {
        assert!(!s.is_empty(), "{} has empty as_str()", name);
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'),
            "{} as_str() {:?} contains invalid characters (expected lowercase alphanum, _, or -)",
            name,
            s,
        );
        assert!(
            !s.starts_with('-') && !s.starts_with('_'),
            "{} as_str() {:?} starts with separator",
            name,
            s,
        );
        assert!(
            !s.ends_with('-') && !s.ends_with('_'),
            "{} as_str() {:?} ends with separator",
            name,
            s,
        );
    };

    for v in ALL_SEVERITIES {
        check("Severity", v.as_str());
    }
    for v in ALL_EVENT_TYPES {
        check("EventType", v.as_str());
    }
    for v in ALL_SOURCE_TYPES {
        check("SourceType", v.as_str());
    }
    for v in ALL_EVIDENCE_ROLES {
        check("EvidenceRole", v.as_str());
    }
    for v in ALL_SITUATION_PHASES {
        check("SituationPhase", v.as_str());
    }
}

// ---------------------------------------------------------------------------
// from_str_lossy coverage: Severity and SituationPhase
// ---------------------------------------------------------------------------

#[test]
fn severity_from_str_lossy_covers_all() {
    for sev in ALL_SEVERITIES {
        assert_eq!(Severity::from_str_lossy(sev.as_str()), sev);
    }
}

#[test]
fn situation_phase_from_str_lossy_covers_all() {
    for phase in ALL_SITUATION_PHASES {
        assert_eq!(SituationPhase::from_str_lossy(phase.as_str()), phase);
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn severity_default_is_low() {
    assert_eq!(Severity::default(), Severity::Low);
}

#[test]
fn situation_phase_default_is_emerging() {
    assert_eq!(SituationPhase::default(), SituationPhase::Emerging);
}
