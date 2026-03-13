pub mod infra_attack;
pub mod military_strike;
pub mod confirmed_strike;
pub mod coordinated_shutdown;
pub mod maritime_enforcement;
pub mod apt_staging;
pub mod conflict_thermal;
pub mod gps_military;
pub mod osint_strike;

use std::collections::HashMap;

use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole};
use crate::types::{EvidenceRef, Incident};
use crate::window::CorrelationWindow;

/// Sort a slice of event references by severity descending (highest first).
/// Used across all correlation rules to prioritize evidence.
pub fn sort_by_severity(events: &mut [&InsertableEvent]) {
    events.sort_by(|a, b| b.severity.rank().cmp(&a.severity.rank()));
}

/// Build an `EvidenceRef` from an `InsertableEvent` and a role.
/// This is the standard way every rule constructs evidence entries.
pub fn evidence_from(event: &InsertableEvent, role: EvidenceRole) -> EvidenceRef {
    EvidenceRef {
        source_type: event.source_type,
        event_type: event.event_type,
        event_time: event.event_time,
        entity_id: event.entity_id.clone(),
        title: event.title.clone(),
        role,
    }
}

/// A correlation rule that can detect cross-source patterns.
pub trait CorrelationRule: Send + Sync {
    /// Unique identifier for this rule.
    fn id(&self) -> &str;

    /// Event types that can trigger evaluation of this rule.
    fn trigger_types(&self) -> &[EventType];

    /// Evaluate whether the trigger event, combined with the window and active
    /// incidents, produces a new or updated incident.
    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident>;
}

/// Registry of correlation rules, indexed by trigger event type for O(1) lookup.
pub struct RuleRegistry {
    rules: Vec<Box<dyn CorrelationRule>>,
    /// Maps event_type → indices into `rules`
    by_trigger: HashMap<EventType, Vec<usize>>,
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            by_trigger: HashMap::new(),
        }
    }

    pub fn register(&mut self, rule: Box<dyn CorrelationRule>) {
        let idx = self.rules.len();
        for &trigger in rule.trigger_types() {
            self.by_trigger
                .entry(trigger)
                .or_default()
                .push(idx);
        }
        self.rules.push(rule);
    }

    /// Get all rules that should be evaluated for a given event type.
    pub fn rules_for(&self, event_type: EventType) -> Vec<&dyn CorrelationRule> {
        self.by_trigger
            .get(&event_type)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| self.rules[idx].as_ref())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a RuleRegistry with all built-in rules registered.
pub fn default_rules() -> RuleRegistry {
    let mut reg = RuleRegistry::new();
    reg.register(Box::new(infra_attack::InfraAttackRule));
    reg.register(Box::new(military_strike::MilitaryStrikeRule));
    reg.register(Box::new(confirmed_strike::ConfirmedStrikeRule));
    reg.register(Box::new(coordinated_shutdown::CoordinatedShutdownRule));
    reg.register(Box::new(maritime_enforcement::MaritimeEnforcementRule));
    reg.register(Box::new(apt_staging::AptStagingRule));
    reg.register(Box::new(conflict_thermal::ConflictThermalClusterRule));
    reg.register(Box::new(gps_military::GpsMilitaryRule));
    reg.register(Box::new(osint_strike::OsintStrikeRule));
    reg
}
