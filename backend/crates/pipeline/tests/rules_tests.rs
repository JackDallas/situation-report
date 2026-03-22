use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use sr_pipeline::rules::infra_attack::InfraAttackRule;
use sr_pipeline::rules::military_strike::MilitaryStrikeRule;
use sr_pipeline::rules::confirmed_strike::ConfirmedStrikeRule;
use sr_pipeline::rules::coordinated_shutdown::CoordinatedShutdownRule;
use sr_pipeline::rules::conflict_thermal::ConflictThermalClusterRule;
use sr_pipeline::rules::gps_military::GpsMilitaryRule;
use sr_pipeline::rules::osint_strike::OsintStrikeRule;
use sr_pipeline::rules::CorrelationRule;
use sr_pipeline::window::CorrelationWindow;
use sr_sources::InsertableEvent;
use sr_types::{EventType, Severity, SourceType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(
    event_type: EventType,
    source_type: SourceType,
    region: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> InsertableEvent {
    InsertableEvent {
        event_type,
        source_type,
        severity: Severity::Medium,
        latitude: lat,
        longitude: lon,
        region_code: region.map(|s| s.to_string()),
        ..Default::default()
    }
}

/// Like make_event but includes a 2-letter country code in the payload
/// (required by country-level correlation rules like InfraAttack and CoordinatedShutdown).
fn make_event_with_country(
    event_type: EventType,
    source_type: SourceType,
    region: Option<&str>,
    country: &str,
) -> InsertableEvent {
    InsertableEvent {
        event_type,
        source_type,
        severity: Severity::Medium,
        region_code: region.map(|s| s.to_string()),
        payload: json!({"country": country}),
        ..Default::default()
    }
}

fn make_military_flight(
    callsign: &str,
    region: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> InsertableEvent {
    InsertableEvent {
        source_type: SourceType::AirplanesLive,
        event_type: EventType::FlightPosition,
        severity: Severity::Medium,
        latitude: lat,
        longitude: lon,
        region_code: region.map(|s| s.to_string()),
        entity_id: Some(callsign.to_string()),
        entity_name: Some(callsign.to_string()),
        tags: vec!["military".to_string()],
        ..Default::default()
    }
}

// ===========================================================================
// 1. InfraAttackRule
// ===========================================================================

#[test]
fn test_infra_attack_triggers_with_full_evidence() {
    let rule = InfraAttackRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let region = "eastern-europe";
    let country = "UA";

    // 25 ShodanBanner events (threshold: 20)
    for _ in 0..25 {
        window.push(make_event_with_country(EventType::ShodanBanner, SourceType::Shodan, Some(region), country));
    }
    // 55 BgpAnomaly events (threshold: 50)
    for _ in 0..55 {
        window.push(make_event_with_country(EventType::BgpAnomaly, SourceType::Bgp, Some(region), country));
    }
    // 6 InternetOutage events (threshold: 5)
    for _ in 0..6 {
        window.push(make_event_with_country(EventType::InternetOutage, SourceType::Ioda, Some(region), country));
    }

    // Trigger on an InternetOutage
    let trigger = make_event_with_country(EventType::InternetOutage, SourceType::Ioda, Some(region), country);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "InfraAttackRule should fire with full evidence");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "infra_attack");
    assert_eq!(incident.severity, Severity::High);
    assert_eq!(incident.region_code.as_deref(), Some(region));
}

#[test]
fn test_infra_attack_no_fire_missing_shodan() {
    let rule = InfraAttackRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Only BGP + outage, no Shodan
    for _ in 0..3 {
        window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    }
    for _ in 0..2 {
        window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));
    }

    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "InfraAttackRule should not fire without Shodan evidence");
}

#[test]
fn test_infra_attack_no_fire_insufficient_bgp() {
    let rule = InfraAttackRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // 2 Shodan + only 2 BGP (needs 3) + 2 outage
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));

    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "InfraAttackRule should not fire with only 2 BGP anomalies (needs 3)");
}

#[test]
fn test_infra_attack_no_fire_wrong_region() {
    let rule = InfraAttackRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Shodan in one region, BGP in another
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("western-europe"), None, None));
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("western-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));

    // Trigger in eastern-europe: missing Shodan in that region
    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "InfraAttackRule should not fire when events are in different regions");
}

#[test]
fn test_infra_attack_no_fire_existing_active() {
    let rule = InfraAttackRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Full evidence
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::ShodanBanner, SourceType::Shodan, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None));

    // Existing active incident in the same region
    let existing = sr_pipeline::Incident {
        rule_id: "infra_attack".into(),
        title: "Existing incident".into(),
        description: "Already active".into(),
        severity: Severity::High,
        confidence: 0.75,
        region_code: Some("eastern-europe".into()),
        ..Default::default()
    };

    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[existing]);

    assert!(result.is_none(), "InfraAttackRule should not fire when active incident exists in same region");
}

// ===========================================================================
// 2. MilitaryStrikeRule
// ===========================================================================

#[test]
fn test_military_strike_triggers_geo() {
    let rule = MilitaryStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let lat = 34.5;
    let lon = 36.3;

    // Military flight near the location
    window.push(make_military_flight("DUKE01", Some("middle-east"), Some(lat + 0.1), Some(lon + 0.1)));

    // NOTAM near the location
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some("middle-east"), Some(lat + 0.05), Some(lon - 0.05)));

    // Seismic event near the location
    window.push(make_event(EventType::SeismicEvent, SourceType::Usgs, Some("middle-east"), Some(lat - 0.1), Some(lon + 0.05)));

    // Trigger on a seismic event at the center
    let trigger = make_event(EventType::SeismicEvent, SourceType::Usgs, Some("middle-east"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "MilitaryStrikeRule should fire with flight + notam + seismic");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "military_strike");
    assert_eq!(incident.severity, Severity::Critical);
}

#[test]
fn test_military_strike_no_fire_missing_seismic() {
    let rule = MilitaryStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let lat = 34.5;
    let lon = 36.3;

    // Military flight + NOTAM, but no seismic
    window.push(make_military_flight("DUKE01", Some("middle-east"), Some(lat + 0.1), Some(lon + 0.1)));
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some("middle-east"), Some(lat + 0.05), Some(lon - 0.05)));

    // Trigger on a flight position (no seismic in window)
    let trigger = make_military_flight("DUKE02", Some("middle-east"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "MilitaryStrikeRule should not fire without seismic evidence");
}

#[test]
fn test_military_strike_no_fire_no_lat_lon() {
    let rule = MilitaryStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Push corroborating events with coordinates
    window.push(make_military_flight("DUKE01", Some("middle-east"), Some(34.5), Some(36.3)));
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some("middle-east"), Some(34.5), Some(36.3)));
    window.push(make_event(EventType::SeismicEvent, SourceType::Usgs, Some("middle-east"), Some(34.5), Some(36.3)));

    // Trigger without coordinates — rule requires lat/lon early return
    let trigger = make_event(EventType::SeismicEvent, SourceType::Usgs, Some("middle-east"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "MilitaryStrikeRule should not fire when trigger has no lat/lon");
}

// ===========================================================================
// 3. GpsMilitaryRule
// ===========================================================================

#[test]
fn test_gps_military_triggers_region_based() {
    let rule = GpsMilitaryRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let region = "eastern-europe";

    // GPS interference, flight, and NOTAM all in the same region (no lat/lon)
    window.push(make_event(EventType::GpsInterference, SourceType::Gpsjam, Some(region), None, None));
    window.push(make_event(EventType::FlightPosition, SourceType::AirplanesLive, Some(region), None, None));
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some(region), None, None));

    // Trigger on GPS interference without coordinates — falls back to region-based
    let trigger = make_event(EventType::GpsInterference, SourceType::Gpsjam, Some(region), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "GpsMilitaryRule should fire with region-based correlation");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "gps_military");
    assert_eq!(incident.severity, Severity::High);
}

#[test]
fn test_gps_military_triggers_geo_based() {
    let rule = GpsMilitaryRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let lat = 50.4;
    let lon = 30.5;

    // GPS interference, flight, and NOTAM near the same location
    window.push(make_event(EventType::GpsInterference, SourceType::Gpsjam, Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));
    window.push(make_military_flight("MIL01", Some("eastern-europe"), Some(lat - 0.1), Some(lon + 0.05)));
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some("eastern-europe"), Some(lat + 0.05), Some(lon - 0.1)));

    // Trigger with coordinates
    let trigger = make_event(EventType::GpsInterference, SourceType::Gpsjam, Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "GpsMilitaryRule should fire with geo-based correlation");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "gps_military");
    assert_eq!(incident.severity, Severity::High);
}

#[test]
fn test_gps_military_no_fire_missing_gps() {
    let rule = GpsMilitaryRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let region = "eastern-europe";

    // Flight + NOTAM only, no GPS interference
    window.push(make_event(EventType::FlightPosition, SourceType::AirplanesLive, Some(region), None, None));
    window.push(make_event(EventType::NotamEvent, SourceType::Notam, Some(region), None, None));

    // Trigger on flight position — GPS interference missing from window
    let trigger = make_event(EventType::FlightPosition, SourceType::AirplanesLive, Some(region), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "GpsMilitaryRule should not fire without GPS interference");
}

// ===========================================================================
// 4. ConfirmedStrikeRule
// ===========================================================================

#[test]
fn test_confirmed_strike_triggers_with_conflict_and_thermal() {
    let rule = ConfirmedStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let lat = 48.5;
    let lon = 37.8;

    // Conflict event near location
    window.push(make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(lat + 0.05), Some(lon + 0.05)));

    // Thermal anomaly near location
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat - 0.05), Some(lon - 0.05)));

    // Trigger on a conflict event at the center
    let mut trigger = make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(lat), Some(lon));
    trigger.payload = json!({"location": "Donetsk"});
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "ConfirmedStrikeRule should fire with conflict + thermal");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "confirmed_strike");
    assert_eq!(incident.severity, Severity::High);
    assert!(incident.title.contains("Donetsk"));
}

#[test]
fn test_confirmed_strike_no_fire_thermal_only() {
    let rule = ConfirmedStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let lat = 48.5;
    let lon = 37.8;

    // Only thermal anomalies, no conflict events
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat + 0.05), Some(lon + 0.05)));
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat - 0.05), Some(lon - 0.05)));

    // Trigger on thermal — conflict will be empty in window
    let trigger = make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "ConfirmedStrikeRule should not fire with only thermal events");
}

#[test]
fn test_confirmed_strike_no_fire_no_coordinates() {
    let rule = ConfirmedStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Events with coordinates in window
    window.push(make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(48.5), Some(37.8)));
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(48.5), Some(37.8)));

    // Trigger without coordinates
    let trigger = make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "ConfirmedStrikeRule should not fire when trigger has no lat/lon");
}

// ===========================================================================
// 5. CoordinatedShutdownRule
// ===========================================================================

#[test]
fn test_coordinated_shutdown_triggers_with_full_evidence() {
    let rule = CoordinatedShutdownRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let region = "middle-east";
    let country = "IR";

    // 6 InternetOutage events (threshold: 5)
    for _ in 0..6 {
        window.push(make_event_with_country(EventType::InternetOutage, SourceType::Ioda, Some(region), country));
    }
    // 55 BgpAnomaly events (threshold: 50)
    for _ in 0..55 {
        window.push(make_event_with_country(EventType::BgpAnomaly, SourceType::Bgp, Some(region), country));
    }
    // 12 CensorshipEvent events (threshold: 10)
    for _ in 0..12 {
        window.push(make_event_with_country(EventType::CensorshipEvent, SourceType::Ooni, Some(region), country));
    }

    // Trigger on an outage
    let trigger = make_event_with_country(EventType::InternetOutage, SourceType::Ioda, Some(region), country);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "CoordinatedShutdownRule should fire with outage + bgp + censorship");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "coordinated_shutdown");
    assert_eq!(incident.severity, Severity::High);
    assert!(incident.title.contains(country));
}

#[test]
fn test_coordinated_shutdown_no_fire_missing_censorship() {
    let rule = CoordinatedShutdownRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    let region = "middle-east";

    // Outage + BGP but no censorship
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some(region), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some(region), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some(region), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some(region), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some(region), None, None));

    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, Some(region), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "CoordinatedShutdownRule should not fire without censorship events");
}

#[test]
fn test_coordinated_shutdown_no_fire_no_region() {
    let rule = CoordinatedShutdownRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(3600));

    // Events in window but trigger has no region
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("middle-east"), None, None));
    window.push(make_event(EventType::InternetOutage, SourceType::Ioda, Some("middle-east"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("middle-east"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("middle-east"), None, None));
    window.push(make_event(EventType::BgpAnomaly, SourceType::Bgp, Some("middle-east"), None, None));
    window.push(make_event(EventType::CensorshipEvent, SourceType::Ooni, Some("middle-east"), None, None));
    window.push(make_event(EventType::CensorshipEvent, SourceType::Ooni, Some("middle-east"), None, None));

    // Trigger without region — rule requires region early return
    let trigger = make_event(EventType::InternetOutage, SourceType::Ioda, None, None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "CoordinatedShutdownRule should not fire when trigger has no region");
}

// ===========================================================================
// 6. ConflictThermalClusterRule
// ===========================================================================

#[test]
fn test_conflict_thermal_cluster_triggers() {
    let rule = ConflictThermalClusterRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // 3 thermal anomalies near the same location (rule requires >= 3)
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat + 0.05), Some(lon + 0.05)));
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat - 0.05), Some(lon - 0.05)));
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat + 0.02), Some(lon - 0.02)));

    // 1 conflict event nearby (rule requires >= 1)
    window.push(make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger on a thermal anomaly
    let trigger = make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "ConflictThermalClusterRule should fire with 3+ thermal + 1+ conflict");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "conflict_thermal_cluster");
    assert_eq!(incident.severity, Severity::High);
}

#[test]
fn test_conflict_thermal_cluster_no_fire_insufficient_thermal() {
    let rule = ConflictThermalClusterRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // Only 2 thermal anomalies (needs 3)
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat + 0.05), Some(lon + 0.05)));
    window.push(make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat - 0.05), Some(lon - 0.05)));

    // 1 conflict event
    window.push(make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger on conflict
    let trigger = make_event(EventType::ConflictEvent, SourceType::Acled, Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "ConflictThermalClusterRule should not fire with only 2 thermal anomalies");
}

#[test]
fn test_conflict_thermal_cluster_no_fire_no_conflict() {
    let rule = ConflictThermalClusterRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // 4 thermal anomalies but no conflict events
    for i in 0..4 {
        let offset = i as f64 * 0.03;
        window.push(make_event(
            EventType::ThermalAnomaly,
            SourceType::Firms,
            Some("eastern-europe"),
            Some(lat + offset),
            Some(lon + offset),
        ));
    }

    // Trigger on thermal — conflicts will be empty
    let trigger = make_event(EventType::ThermalAnomaly, SourceType::Firms, Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "ConflictThermalClusterRule should not fire without conflict events");
}

// ===========================================================================
// 7. RuleRegistry integration
// ===========================================================================

#[test]
fn test_default_rules_returns_all_eight_rules() {
    let registry = sr_pipeline::rules::default_rules();

    // Verify all trigger types are registered
    let infra_rules = registry.rules_for(EventType::ShodanBanner);
    assert!(!infra_rules.is_empty(), "ShodanBanner should have at least one rule");

    let strike_rules = registry.rules_for(EventType::FlightPosition);
    assert!(!strike_rules.is_empty(), "FlightPosition should have at least one rule");

    let gps_rules = registry.rules_for(EventType::GpsInterference);
    assert!(!gps_rules.is_empty(), "GpsInterference should have at least one rule");

    let thermal_rules = registry.rules_for(EventType::ThermalAnomaly);
    assert!(!thermal_rules.is_empty(), "ThermalAnomaly should have at least one rule");

    let telegram_rules = registry.rules_for(EventType::TelegramMessage);
    assert!(!telegram_rules.is_empty(), "TelegramMessage should have at least one rule (osint_strike)");
}

#[test]
fn test_rule_ids_are_unique() {
    let registry = sr_pipeline::rules::default_rules();

    let mut seen_ids = std::collections::HashSet::new();
    // Check all event types for registered rules
    for et in sr_types::ALL_EVENT_TYPES {
        for rule in registry.rules_for(et) {
            seen_ids.insert(rule.id().to_string());
        }
    }

    // We expect 10 unique rule IDs
    assert_eq!(seen_ids.len(), 10, "Expected 10 unique rule IDs, got: {:?}", seen_ids);
}

// ===========================================================================
// 8. OsintStrikeRule
// ===========================================================================

fn make_telegram_strike(
    region: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> InsertableEvent {
    InsertableEvent {
        event_type: EventType::TelegramMessage,
        source_type: SourceType::Telegram,
        severity: Severity::Medium,
        latitude: lat,
        longitude: lon,
        region_code: region.map(|s| s.to_string()),
        tags: vec!["strike".to_string(), "military".to_string()],
        title: Some("Missile strike reported in the area".to_string()),
        ..Default::default()
    }
}

fn make_news_article(
    region: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> InsertableEvent {
    InsertableEvent {
        event_type: EventType::NewsArticle,
        source_type: SourceType::Gdelt,
        severity: Severity::Medium,
        latitude: lat,
        longitude: lon,
        region_code: region.map(|s| s.to_string()),
        title: Some("Reports of military strike confirmed".to_string()),
        ..Default::default()
    }
}

#[test]
fn test_osint_strike_telegram_plus_news_fires_high() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News article corroborating in the area
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger: Telegram message with strike keywords
    let trigger = make_telegram_strike(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "OsintStrikeRule should fire with telegram + news");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "osint_strike");
    assert_eq!(incident.severity, Severity::High);
    assert!(incident.tags.contains(&"osint".to_string()));
    assert!(!incident.tags.contains(&"satellite-confirmed".to_string()));
}

#[test]
fn test_osint_strike_telegram_plus_news_plus_thermal_fires_critical() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(13 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News article corroborating in the area
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Thermal anomaly nearby (satellite confirmation)
    window.push(make_event(
        EventType::ThermalAnomaly,
        SourceType::Firms,
        Some("eastern-europe"),
        Some(lat - 0.05),
        Some(lon + 0.05),
    ));

    // Trigger: Telegram message with strike keywords
    let trigger = make_telegram_strike(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "OsintStrikeRule should fire with telegram + news + thermal");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "osint_strike");
    assert_eq!(incident.severity, Severity::Critical);
    assert!(incident.tags.contains(&"satellite-confirmed".to_string()));
}

#[test]
fn test_osint_strike_news_trigger_with_telegram_in_window() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // Telegram message already in the window (with strike keywords + severity >= Medium)
    window.push(make_telegram_strike(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger: News article arriving later
    let trigger = make_news_article(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "OsintStrikeRule should fire when news triggers with qualifying telegram in window");
    let incident = result.unwrap();
    assert_eq!(incident.rule_id, "osint_strike");
    assert_eq!(incident.severity, Severity::High);
}

#[test]
fn test_osint_strike_no_fire_telegram_only() {
    let rule = OsintStrikeRule;
    let window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // Trigger: Telegram message with strike keywords, but no news in window
    let trigger = make_telegram_strike(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "OsintStrikeRule should not fire with only Telegram (no news corroboration)");
}

#[test]
fn test_osint_strike_no_fire_low_severity_telegram() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News in window
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger: Low-severity Telegram (below Medium threshold)
    let mut trigger = make_telegram_strike(Some("eastern-europe"), Some(lat), Some(lon));
    trigger.severity = Severity::Low;
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "OsintStrikeRule should not fire when Telegram severity is below Medium");
}

#[test]
fn test_osint_strike_no_fire_no_keywords() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News in window
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger: Telegram without strike keywords
    let trigger = InsertableEvent {
        event_type: EventType::TelegramMessage,
        source_type: SourceType::Telegram,
        severity: Severity::Medium,
        latitude: Some(lat),
        longitude: Some(lon),
        region_code: Some("eastern-europe".to_string()),
        tags: vec!["weather".to_string()],
        title: Some("Good weather expected tomorrow".to_string()),
        ..Default::default()
    };
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "OsintStrikeRule should not fire when Telegram has no strike keywords");
}

#[test]
fn test_osint_strike_no_fire_no_coordinates() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    // News in window with coordinates
    window.push(make_news_article(Some("eastern-europe"), Some(48.5), Some(37.8)));

    // Trigger: Telegram without coordinates
    let trigger = make_telegram_strike(Some("eastern-europe"), None, None);
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "OsintStrikeRule should not fire when trigger has no lat/lon");
}

#[test]
fn test_osint_strike_no_fire_existing_active_nearby() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News in window
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Existing active incident nearby
    let existing = sr_pipeline::Incident {
        rule_id: "osint_strike".into(),
        title: "Existing OSINT strike".into(),
        severity: Severity::High,
        latitude: Some(lat + 0.2),
        longitude: Some(lon + 0.2),
        ..Default::default()
    };

    let trigger = make_telegram_strike(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[existing]);

    assert!(result.is_none(), "OsintStrikeRule should not fire when active incident exists nearby");
}

#[test]
fn test_osint_strike_no_fire_news_trigger_without_qualifying_telegram() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // Telegram in window but without strike keywords (non-qualifying)
    window.push(InsertableEvent {
        event_type: EventType::TelegramMessage,
        source_type: SourceType::Telegram,
        severity: Severity::Medium,
        latitude: Some(lat + 0.1),
        longitude: Some(lon + 0.1),
        region_code: Some("eastern-europe".to_string()),
        tags: vec!["traffic".to_string()],
        title: Some("Traffic update for Kyiv".to_string()),
        ..Default::default()
    });

    // Trigger: News article
    let trigger = make_news_article(Some("eastern-europe"), Some(lat), Some(lon));
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_none(), "OsintStrikeRule should not fire when news triggers but no qualifying Telegram in window");
}

#[test]
fn test_osint_strike_fires_on_enrichment_state_changes() {
    let rule = OsintStrikeRule;
    let mut window = CorrelationWindow::new(Duration::from_secs(7 * 3600));

    let lat = 48.5;
    let lon = 37.8;

    // News in window
    window.push(make_news_article(Some("eastern-europe"), Some(lat + 0.1), Some(lon + 0.1)));

    // Trigger: Telegram without explicit strike keywords, but with enrichment state_changes
    let trigger = InsertableEvent {
        event_type: EventType::TelegramMessage,
        source_type: SourceType::Telegram,
        severity: Severity::Medium,
        latitude: Some(lat),
        longitude: Some(lon),
        region_code: Some("eastern-europe".to_string()),
        tags: vec!["update".to_string()],
        title: Some("Situation update from the front".to_string()),
        payload: json!({
            "enrichment": {
                "state_changes": [
                    {"type": "killed", "entity": "soldiers", "certainty": "confirmed"}
                ]
            }
        }),
        ..Default::default()
    };
    let result = rule.evaluate(&trigger, &window, &[]);

    assert!(result.is_some(), "OsintStrikeRule should fire when Telegram has enrichment state_changes");
    let incident = result.unwrap();
    assert_eq!(incident.severity, Severity::High);
}
