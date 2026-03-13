use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader;
use regex::Regex;
use serde_json::json;
use tracing::{debug, info, warn};

use chrono::Utc;

use sr_types::{EventType, Severity, SourceType};

use crate::{DataSource, InsertableEvent, SourceContext};

// ---------------------------------------------------------------------------
// Provider URLs
// ---------------------------------------------------------------------------

/// NATS UK PIB XML feed — no auth, updated hourly, good for GPS jamming NOTAMs.
/// Note: pibs.nats.aero was decommissioned; new feed uses structured EAD-format XML.
const NATS_UK_URL: &str = "https://pibs.nats.co.uk/operational/pibs/PIB.xml";

/// FIRs where new TRA/Danger Area NOTAMs are classified as "critical".
const CRITICAL_FIRS: &[&str] = &["OIIX", "ORBB", "OBBB"];

/// High-priority Q-code prefixes we care about.
/// QR = restricted areas, QW = warnings, QA = airfield, QN = navigation.
/// Used by NATS conflict filtering tests; will be used by FAA provider when implemented.
#[allow(dead_code)]
const PRIORITY_QCODE_PREFIXES: &[&str] = &["QR", "QW", "QA", "QN"];

/// Conflict-relevant Q-codes to specifically filter for in the NATS UK feed.
/// QRALC = restricted area established, QRTCA = TRA/danger area created,
/// QFAHC = aerodrome facilities closed, QFALC = aerodrome closed.
const NATS_CONFLICT_QCODES: &[&str] = &["QRALC", "QRTCA", "QFAHC", "QFALC"];

/// How often we poll the NATS UK feed (hourly — the feed updates hourly).
const NATS_POLL_INTERVAL_SECS: u64 = 3600;

// ---------------------------------------------------------------------------
// Q-code decode table — human-readable NOTAM enrichment
// ---------------------------------------------------------------------------

/// Decoded information about a NOTAM Q-code.
#[derive(Debug, Clone)]
pub struct QcodeInfo {
    /// Human-readable explanation of the Q-code.
    pub description: String,
    /// Broad category for grouping/display.
    pub category: String,
    /// Whether this Q-code is typically routine (maintenance, scheduled exercises).
    pub routine: bool,
    /// Brief context explaining the significance.
    pub significance: String,
}

/// Decode a 5-character ICAO Q-code into a human-readable [`QcodeInfo`].
///
/// Covers the most common Q-codes encountered in conflict-relevant NOTAMs.
/// Unknown codes return a generic description based on the prefix.
pub fn decode_qcode(qcode: &str) -> QcodeInfo {
    let qcode = qcode.trim().to_uppercase();
    match qcode.as_str() {
        "QFALC" => QcodeInfo {
            description: "Aerodrome closed to all traffic".into(),
            category: "Airfield Closure".into(),
            routine: false,
            significance: "Common for night closures and maintenance. Unusual if during normal operating hours or without prior scheduling.".into(),
        },
        "QFAHC" => QcodeInfo {
            description: "Aerodrome facilities closed or restricted".into(),
            category: "Airfield Closure".into(),
            routine: true,
            significance: "Usually routine maintenance of terminal, apron, or ground services. Significant if affecting all facilities simultaneously.".into(),
        },
        "QRALC" => QcodeInfo {
            description: "Restricted area established — entry prohibited without authorization".into(),
            category: "Restricted Area".into(),
            routine: false,
            significance: "New airspace restriction. May indicate military activity, VIP movement, or security operation. Check FIR context.".into(),
        },
        "QRTCA" => QcodeInfo {
            description: "Temporary restricted area or danger area created".into(),
            category: "Restricted Area".into(),
            routine: false,
            significance: "Temporary danger zone activated. Often for military exercises, weapons testing, or emergency security perimeters.".into(),
        },
        "QWMLW" => QcodeInfo {
            description: "Military exercise warning — live or simulated operations in area".into(),
            category: "Military Warning".into(),
            routine: false,
            significance: "Active military exercise. Routine if pre-scheduled, significant if unannounced or in contested airspace.".into(),
        },
        "QWPLW" => QcodeInfo {
            description: "Parachute exercise or drop zone active".into(),
            category: "Military Warning".into(),
            routine: true,
            significance: "Standard airborne training exercise. Usually pre-scheduled with defined time windows.".into(),
        },
        "QWELW" => QcodeInfo {
            description: "Exercise warning — military or civil exercise in progress".into(),
            category: "Military Warning".into(),
            routine: false,
            significance: "General exercise activity. Check text for GPS jamming, live fire, or large-scale maneuvers.".into(),
        },
        "QWHLW" => QcodeInfo {
            description: "Hazardous operations warning — live firing, demolition, or similar".into(),
            category: "Military Warning".into(),
            routine: false,
            significance: "Hazardous military activity such as live firing or demolition. Potentially significant.".into(),
        },
        "QOBCE" => QcodeInfo {
            description: "New obstacle erected (crane, mast, or temporary structure)".into(),
            category: "Obstacle".into(),
            routine: true,
            significance: "Construction or temporary crane. Routine unless near active approach/departure paths.".into(),
        },
        "QNVAS" => QcodeInfo {
            description: "Navigation aid unserviceable (VOR, DME, NDB, or ILS component)".into(),
            category: "Navigation Warning".into(),
            routine: true,
            significance: "Nav aid outage. Usually scheduled maintenance. Significant if multiple aids fail simultaneously.".into(),
        },
        "QICAS" => QcodeInfo {
            description: "Instrument approach procedure unavailable".into(),
            category: "Navigation Warning".into(),
            routine: true,
            significance: "Approach procedure suspended, often due to nav aid maintenance. May impact operations in poor weather.".into(),
        },
        "QMRLC" => QcodeInfo {
            description: "Runway closed".into(),
            category: "Runway/Taxiway".into(),
            routine: true,
            significance: "Common for resurfacing, repairs, or snow clearing. Significant if the only runway at an aerodrome.".into(),
        },
        "QMAHC" => QcodeInfo {
            description: "Apron area closed".into(),
            category: "Runway/Taxiway".into(),
            routine: true,
            significance: "Aircraft parking/loading area closed. Usually maintenance or construction. Rarely operationally critical.".into(),
        },
        "QRDCA" => QcodeInfo {
            description: "Danger area activated".into(),
            category: "Restricted Area".into(),
            routine: false,
            significance: "Active danger zone. May involve live weapons, missile testing, or rocket launches.".into(),
        },
        "QRRCA" => QcodeInfo {
            description: "Restricted area activated".into(),
            category: "Restricted Area".into(),
            routine: false,
            significance: "Existing restricted airspace now active. Check for military operations or security events.".into(),
        },
        "QNMAS" => QcodeInfo {
            description: "VOR/DME station unserviceable".into(),
            category: "Navigation Warning".into(),
            routine: true,
            significance: "VOR/DME outage. Typically scheduled maintenance.".into(),
        },
        "QNMCT" => QcodeInfo {
            description: "TACAN facility unserviceable".into(),
            category: "Navigation Warning".into(),
            routine: true,
            significance: "Military TACAN outage. Usually maintenance.".into(),
        },
        "QFAAH" => QcodeInfo {
            description: "Aerodrome operating hours changed".into(),
            category: "Airfield Closure".into(),
            routine: true,
            significance: "Hours of operation adjusted. Common seasonal or staffing change.".into(),
        },
        "QFALT" => QcodeInfo {
            description: "Aerodrome lighting unserviceable or restricted".into(),
            category: "Airfield Closure".into(),
            routine: true,
            significance: "Lighting outage, often scheduled maintenance. May restrict night operations.".into(),
        },
        "QMTLC" => QcodeInfo {
            description: "Taxiway closed".into(),
            category: "Runway/Taxiway".into(),
            routine: true,
            significance: "Taxiway closure for maintenance or construction. Rarely critical.".into(),
        },
        _ => {
            // Prefix-based fallback for unknown specific codes
            let (desc, cat, routine, sig) = if qcode.starts_with("QFA") {
                ("Aerodrome status change", "Airfield Closure", false, "Aerodrome operational change. Check text for details.")
            } else if qcode.starts_with("QR") {
                ("Airspace restriction in effect", "Restricted Area", false, "Airspace restriction. May indicate security or military activity.")
            } else if qcode.starts_with("QW") {
                ("Airspace warning issued", "Airspace Warning", false, "Warning about hazardous activity in airspace.")
            } else if qcode.starts_with("QN") {
                ("Navigation aid or procedure affected", "Navigation Warning", true, "Navigation infrastructure change. Usually maintenance.")
            } else if qcode.starts_with("QO") {
                ("Obstacle or terrain notification", "Obstacle", true, "Obstacle information. Usually construction-related.")
            } else if qcode.starts_with("QM") {
                ("Movement area change (runway/taxiway)", "Runway/Taxiway", true, "Ground movement area change. Usually maintenance.")
            } else if qcode.starts_with("QF") {
                ("Facility operational change", "Facility", true, "Facility status update.")
            } else if qcode.starts_with("QI") {
                ("Instrument procedure change", "Navigation Warning", true, "Instrument procedure modification.")
            } else if qcode.starts_with("QL") {
                ("Lighting system change", "Facility", true, "Lighting infrastructure change.")
            } else {
                ("NOTAM — see text for details", "Other", true, "Unknown Q-code category. Review raw NOTAM text for context.")
            };
            QcodeInfo {
                description: desc.into(),
                category: cat.into(),
                routine,
                significance: sig.into(),
            }
        }
    }
}

/// Expand common ICAO/NOTAM abbreviations in NOTAM text to make it more readable.
///
/// Leaves the original structure intact but inserts expansions in parentheses
/// where abbreviations appear as whole words.
pub fn decode_notam_text(text: &str) -> String {
    // We do whole-word replacements to avoid mangling substrings.
    // The approach: split on word boundaries, replace known abbreviations.
    let abbreviations: &[(&str, &str)] = &[
        ("AD", "Aerodrome"),
        ("CLSD", "Closed"),
        ("ATS", "Air Traffic Services"),
        ("RWY", "Runway"),
        ("TWY", "Taxiway"),
        ("APCH", "Approach"),
        ("DEP", "Departure"),
        ("TFC", "Traffic"),
        ("OPS", "Operations"),
        ("SVC", "Service"),
        ("HR", "Hours"),
        ("HRS", "Hours"),
        ("AVBL", "Available"),
        ("BTN", "Between"),
        ("ABV", "Above"),
        ("BLW", "Below"),
        ("SFC", "Surface"),
        ("UNL", "Unlimited"),
        ("ACT", "Active"),
        ("TEMPO", "Temporary"),
        ("PERM", "Permanent"),
        ("TRA", "Temporary Reserved Area"),
        ("FIR", "Flight Information Region"),
        ("CTR", "Control Zone"),
        ("CTA", "Control Area"),
        ("UIR", "Upper Information Region"),
        ("NON-SKED", "Non-Scheduled"),
        ("SKED", "Scheduled"),
        ("MON", "Monday"),
        ("TUE", "Tuesday"),
        ("WED", "Wednesday"),
        ("THU", "Thursday"),
        ("FRI", "Friday"),
        ("SAT", "Saturday"),
        ("SUN", "Sunday"),
        ("H24", "24 Hours"),
        ("HJ", "Sunrise to Sunset"),
        ("HN", "Sunset to Sunrise"),
        ("NOTAMN", "New NOTAM"),
        ("NOTAMR", "Replacement NOTAM"),
        ("NOTAMC", "Cancellation NOTAM"),
        ("VOR", "VHF Omnidirectional Range"),
        ("DME", "Distance Measuring Equipment"),
        ("NDB", "Non-Directional Beacon"),
        ("ILS", "Instrument Landing System"),
        ("ATIS", "Automatic Terminal Information Service"),
        ("PSN", "Position"),
        ("OBST", "Obstacle"),
        ("LGT", "Light/Lighting"),
        ("LGTD", "Lighted"),
        ("UNLGTD", "Unlighted"),
        ("INOP", "Inoperative"),
        ("U/S", "Unserviceable"),
        ("WI", "Within"),
        ("NM", "Nautical Miles"),
        ("AGL", "Above Ground Level"),
        ("AMSL", "Above Mean Sea Level"),
        ("EXC", "Except"),
        ("EMERG", "Emergency"),
        ("MIL", "Military"),
        ("CIV", "Civil"),
        ("VMC", "Visual Met Conditions"),
        ("IMC", "Instrument Met Conditions"),
        ("VFR", "Visual Flight Rules"),
        ("IFR", "Instrument Flight Rules"),
        ("TFR", "Temporary Flight Restriction"),
        ("RQRD", "Required"),
        ("AUTH", "Authorization"),
        ("PPR", "Prior Permission Required"),
        ("ARR", "Arrival"),
        ("MAINT", "Maintenance"),
        ("EXER", "Exercise"),
        ("GND", "Ground"),
        ("INTL", "International"),
        ("NAV", "Navigation"),
        ("GNSS", "Global Navigation Satellite System"),
        ("RNAV", "Area Navigation"),
        ("RNP", "Required Navigation Performance"),
    ];

    // Use FL separately since it's commonly followed by a number
    // e.g. "FL100" should become "Flight Level 100"
    let fl_re = regex::Regex::new(r"\bFL(\d+)\b").unwrap();

    let mut result = text.to_string();

    // Replace FL### pattern first
    result = fl_re.replace_all(&result, "Flight Level $1").to_string();

    // Build a regex for each abbreviation and replace whole words
    for &(abbr, expansion) in abbreviations {
        // Escape special regex chars in abbreviation (for "U/S" etc.)
        let escaped = regex::escape(abbr);
        if let Ok(re) = regex::Regex::new(&format!(r"\b{}\b", escaped)) {
            result = re.replace_all(&result, expansion).to_string();
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Classification helpers
// ---------------------------------------------------------------------------

/// Human-readable labels for FIR codes.
fn fir_label(code: &str) -> &'static str {
    match code {
        "OIIX" => "Tehran FIR (Iran)",
        "ORBB" => "Baghdad FIR (Iraq)",
        "OBBB" => "Bahrain FIR (Persian Gulf)",
        "OMAE" => "Emirates FIR (UAE)",
        "OOMM" => "Muscat FIR (Oman)",
        "OEJD" => "Jeddah FIR (Saudi Arabia)",
        "LTBB" => "Ankara FIR (Turkey)",
        "LCCC" => "Nicosia FIR (Cyprus)",
        "OSTT" => "Damascus FIR (Syria)",
        "OJAC" => "Amman FIR (Jordan)",
        "LLLL" => "Tel Aviv FIR (Israel)",
        // UK FIRs from NATS feed
        "EGTT" => "London FIR (UK)",
        "EGPX" => "Scottish FIR (UK)",
        _ => "Unknown FIR",
    }
}

/// Classify a NOTAM Q-code prefix into a human-readable type.
fn qcode_type(qcode: &str) -> &'static str {
    if qcode.starts_with("QR") {
        "Restricted Area"
    } else if qcode.starts_with("QW") {
        "Warning"
    } else if qcode.starts_with("QA") {
        "Airfield"
    } else if qcode.starts_with("QN") {
        "Navigation"
    } else if qcode.starts_with("QF") {
        "Facility"
    } else {
        "Other"
    }
}

/// Map a NOTAM Q-code to a human-readable topic slug suitable for use as a tag.
/// Specific well-known codes take priority; falls back to prefix-based categories.
fn qcode_topic_tag(qcode: &str) -> &'static str {
    match qcode {
        "QRTCA" | "QRALC" => "airspace-restriction",
        "QRDCA" => "danger-area",
        "QWELW" | "QWHLW" | "QWPLW" => "airspace-warning",
        "QFALC" | "QFAAH" => "aerodrome-closed",
        "QFAHC" => "aerodrome-facility",
        "QNMAS" | "QNMCT" => "navigation-aid",
        _ if qcode.starts_with("QR") => "airspace-restriction",
        _ if qcode.starts_with("QW") => "airspace-warning",
        _ if qcode.starts_with("QA") => "airfield",
        _ if qcode.starts_with("QN") => "navigation",
        _ if qcode.starts_with("QF") => "facility",
        _ if qcode.starts_with("QO") => "obstacle",
        _ => "notam",
    }
}

/// Map a FIR code to a human-readable slug suitable for use as a tag.
/// Well-known FIRs get named slugs; everything else becomes "fir-<lowercase>".
fn fir_topic_tag(fir: &str) -> String {
    match fir {
        "EGTT" => "london-fir".to_string(),
        "EGPX" => "scottish-fir".to_string(),
        "OIIX" => "tehran-fir".to_string(),
        "ORBB" => "baghdad-fir".to_string(),
        "OBBB" => "bahrain-fir".to_string(),
        "OSTT" => "damascus-fir".to_string(),
        "LLLL" => "tel-aviv-fir".to_string(),
        "UKBV" | "UKDV" | "UKLV" | "UKFV" | "UKOV" => "ukraine-fir".to_string(),
        "UMMV" => "minsk-fir".to_string(),
        "UUWV" => "moscow-fir".to_string(),
        _ => format!("fir-{}", fir.to_lowercase()),
    }
}

/// Determine severity: "critical" for TRA/Danger Areas (QR/QW) in critical FIRs,
/// "warning" for everything else that passes the filter.
fn classify_severity(qcode: &str, fir: &str) -> &'static str {
    let is_critical_fir = CRITICAL_FIRS.contains(&fir);
    let is_tra_danger = qcode.starts_with("QR") || qcode.starts_with("QW");

    if is_critical_fir && is_tra_danger {
        "critical"
    } else {
        "warning"
    }
}

/// Approximate center coordinates (lat, lon) for well-known FIR codes.
/// Used as a fallback when Q-line coordinate parsing fails.
fn fir_center(fir: &str) -> Option<(f64, f64)> {
    match fir {
        "EGTT" => Some((51.5, -0.1)),    // London FIR
        "EGPX" => Some((56.5, -4.0)),    // Scottish FIR
        "OIIX" => Some((35.7, 51.4)),    // Tehran FIR
        "LLLL" => Some((32.0, 34.9)),    // Tel Aviv FIR
        "OSTT" => Some((33.5, 36.3)),    // Damascus FIR
        "UKBV" => Some((50.4, 30.5)),    // Kyiv FIR
        "UUWV" => Some((55.8, 37.6)),    // Moscow FIR
        "LFFF" => Some((48.9, 2.3)),     // Paris FIR
        "EDGG" => Some((50.0, 8.5)),     // Langen/Germany FIR
        "ORBB" => Some((33.3, 44.4)),    // Baghdad FIR
        "OBBB" => Some((26.1, 50.5)),    // Bahrain FIR
        "OMAE" => Some((24.5, 54.7)),    // Emirates FIR
        "OOMM" => Some((23.6, 58.5)),    // Muscat FIR
        "OEJD" => Some((21.5, 39.2)),    // Jeddah FIR
        "LTBB" => Some((39.9, 32.9)),    // Ankara FIR
        "LCCC" => Some((35.2, 33.4)),    // Nicosia FIR
        "OJAC" => Some((31.9, 35.9)),    // Amman FIR
        _ => None,
    }
}

/// ICAO airport code → (lat, lon, name) for coordinate resolution when Q-line
/// coordinates are missing. Covers UK aerodromes plus key Middle East/European
/// airports referenced in conflict-relevant NOTAMs.
fn airport_coords(icao: &str) -> Option<(f64, f64, &'static str)> {
    match icao {
        // UK major
        "EGLL" => Some((51.4775, -0.4614, "Heathrow")),
        "EGKK" => Some((51.1481, -0.1903, "Gatwick")),
        "EGSS" => Some((51.8850, 0.2350, "Stansted")),
        "EGGW" => Some((51.8747, -0.3684, "Luton")),
        "EGCC" => Some((53.3537, -2.2750, "Manchester")),
        "EGBB" => Some((52.4539, -1.7480, "Birmingham")),
        "EGPH" => Some((55.9500, -3.3725, "Edinburgh")),
        "EGPF" => Some((55.8719, -4.4331, "Glasgow")),
        "EGNT" => Some((55.0375, -1.6917, "Newcastle")),
        "EGNM" => Some((53.8659, -1.6606, "Leeds Bradford")),
        "EGGD" => Some((51.3827, -2.7191, "Bristol")),
        "EGHI" => Some((50.9503, -1.3568, "Southampton")),
        "EGNX" => Some((52.8311, -1.3281, "East Midlands")),
        "EGAA" => Some((54.6575, -6.2158, "Belfast Intl")),
        "EGJJ" => Some((49.2078, -2.1956, "Jersey")),
        "EGJB" => Some((49.4350, -2.6020, "Guernsey")),
        "EGNC" => Some((54.9375, -2.8092, "Carlisle")),
        "EGNO" => Some((53.7450, -2.8831, "Warton")),
        "EGDR" => Some((50.0861, -5.2556, "Culdrose")),
        "EGSU" => Some((52.0906, 0.1317, "Duxford")),
        "EGTC" => Some((51.7722, -1.0928, "Cranfield")),
        // UK military
        "EGXC" => Some((53.0931, -0.4828, "Coningsby")),
        "EGUL" => Some((52.4093, 0.5614, "Lakenheath")),
        "EGYM" => Some((52.3617, 0.7728, "Marham")),
        "EGVA" => Some((51.6822, -1.7903, "Fairford")),
        "EGVN" => Some((51.7500, -1.5839, "Brize Norton")),
        "EGDM" => Some((51.1522, -1.7478, "Boscombe Down")),
        "EGQS" => Some((57.7053, -3.3211, "Lossiemouth")),
        "EGXE" => Some((54.2925, -1.5353, "Leeming")),
        "EGOV" => Some((53.2481, -4.5353, "Valley")),
        "EGDY" => Some((51.0094, -2.6386, "Yeovilton")),
        // Middle East
        "OIIE" => Some((35.4161, 51.1522, "Tehran IKA")),
        "OIII" => Some((35.6894, 51.3133, "Tehran Mehrabad")),
        "OLBA" => Some((33.8209, 35.4884, "Beirut")),
        "ORBI" => Some((33.2625, 44.2346, "Baghdad")),
        "LLBG" => Some((32.0094, 34.8867, "Ben Gurion")),
        "OSDI" => Some((33.4114, 36.5156, "Damascus")),
        _ => None,
    }
}

/// Check if a Q-code line contains a high-priority code.
/// Will be used by FAA provider when implemented.
#[allow(dead_code)]
fn is_priority_qcode(qcode: &str) -> bool {
    let upper = qcode.to_uppercase();
    PRIORITY_QCODE_PREFIXES
        .iter()
        .any(|prefix| upper.contains(prefix))
}

/// Check if a Q-code matches conflict-relevant codes for the NATS feed.
fn is_nats_conflict_qcode(qcode: &str) -> bool {
    let upper = qcode.to_uppercase();
    NATS_CONFLICT_QCODES.iter().any(|code| upper.contains(code))
}

/// Check NOTAM text for GPS jamming / GNSS interference patterns.
fn is_gps_jamming_text(text: &str) -> bool {
    let upper = text.to_uppercase();
    // Common phrases used in GPS jamming exercise NOTAMs
    upper.contains("GPS JAMMING")
        || upper.contains("GPS JAM")
        || upper.contains("GNSS INTERFERENCE")
        || upper.contains("GNSS TESTING")
        || upper.contains("GNSS JAMMING")
        || upper.contains("GPS UNRELIABLE")
        || upper.contains("GNSS UNRELIABLE")
        || upper.contains("NAVIGATION SIGNAL")
        || upper.contains("GPS INTF")
}

/// Extract the Q-code portion from the NOTAM Q-line (field Q).
/// The Q-code is typically the 2nd element in a slash-separated Q-line:
/// e.g. "OIIX/QRRCA/IV/NBO/AE/000/999/3548N05132E005"
fn extract_qcode(q_line: &str) -> String {
    q_line
        .split('/')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_uppercase()
}

/// Extract the FIR code from the Q-line (first element).
fn extract_fir_from_qline(q_line: &str) -> String {
    q_line
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_uppercase()
}

/// Parse coordinates from a Q-line coordinate field like "5130N00027W025"
/// and return (lat, lon) in decimal degrees.
///
/// Supports two ICAO coordinate formats found in Q-lines:
///
/// 1. **Standard** (DDMM[NS]DDDMM[EW]) — 4-digit lat, 5-digit lon
///    e.g. `5130N00027W025` → 51°30'N 000°27'W
///
/// 2. **Extended** (DDMMd[NS]DDDMMd[EW]) — 5-digit lat, 6-digit lon
///    with tenths-of-minutes (EAD/SDO European feeds)
///    e.g. `51305N000273W025` → 51°30.5'N 000°27.3'W
fn parse_qline_coords(q_line: &str) -> Option<(f64, f64)> {
    // The coordinate part is the last slash-separated field:
    // e.g. "EGTT/QWELW/IV/BO/W/000/100/5130N00027W025" → "5130N00027W025"
    let coord_part = q_line.split('/').next_back()?;
    if coord_part.len() < 11 {
        return None;
    }

    // Try extended format first (DDMMd[NS]DDDMMd[EW], with optional radius),
    // then fall back to standard (DDMM[NS]DDDMM[EW], with optional radius).
    // Extended: 5 lat digits + N/S + 6 lon digits + E/W
    let re_ext =
        Regex::new(r"(\d{2})(\d{2})(\d)([NS])(\d{3})(\d{2})(\d)([EW])").ok()?;
    // Standard: 4 lat digits + N/S + 5 lon digits + E/W
    let re_std =
        Regex::new(r"(\d{2})(\d{2})([NS])(\d{3})(\d{2})([EW])").ok()?;

    if let Some(caps) = re_ext.captures(coord_part) {
        // Extended format: DDMMd N DDDMMd E/W
        let lat_deg: f64 = caps.get(1)?.as_str().parse().ok()?;
        let lat_min: f64 = caps.get(2)?.as_str().parse().ok()?;
        let lat_tenth: f64 = caps.get(3)?.as_str().parse().ok()?;
        let lat_dir = caps.get(4)?.as_str();
        let lon_deg: f64 = caps.get(5)?.as_str().parse().ok()?;
        let lon_min: f64 = caps.get(6)?.as_str().parse().ok()?;
        let lon_tenth: f64 = caps.get(7)?.as_str().parse().ok()?;
        let lon_dir = caps.get(8)?.as_str();

        let mut lat = lat_deg + (lat_min + lat_tenth / 10.0) / 60.0;
        let mut lon = lon_deg + (lon_min + lon_tenth / 10.0) / 60.0;

        if lat_dir == "S" {
            lat = -lat;
        }
        if lon_dir == "W" {
            lon = -lon;
        }
        Some((lat, lon))
    } else if let Some(caps) = re_std.captures(coord_part) {
        // Standard format: DDMM N DDDMM E/W
        let lat_deg: f64 = caps.get(1)?.as_str().parse().ok()?;
        let lat_min: f64 = caps.get(2)?.as_str().parse().ok()?;
        let lat_dir = caps.get(3)?.as_str();
        let lon_deg: f64 = caps.get(4)?.as_str().parse().ok()?;
        let lon_min: f64 = caps.get(5)?.as_str().parse().ok()?;
        let lon_dir = caps.get(6)?.as_str();

        let mut lat = lat_deg + lat_min / 60.0;
        let mut lon = lon_deg + lon_min / 60.0;

        if lat_dir == "S" {
            lat = -lat;
        }
        if lon_dir == "W" {
            lon = -lon;
        }
        Some((lat, lon))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// NATS UK XML parsing
// ---------------------------------------------------------------------------

/// A NOTAM parsed from the NATS UK XML feed.
#[derive(Debug, Clone)]
struct NatsNotam {
    /// NOTAM series/number identifier
    id: String,
    /// Full NOTAM text (E-field / body)
    text: String,
    /// Q-line
    q_line: String,
    /// Start validity
    start_validity: Option<String>,
    /// End validity
    end_validity: Option<String>,
    /// Location code
    location: Option<String>,
}

/// Parse the NATS UK XML feed into individual NOTAM records.
///
/// Supports two XML formats:
///
/// **Legacy format** (pibs.nats.aero, decommissioned):
/// ```xml
/// <NOTAMS>
///   <NOTAM>
///     <ID>A0001/26</ID>
///     <ITEME>...</ITEME>
///     <ITEMQ>EGTT/QWELW/IV/BO/W/000/100/5130N00027W025</ITEMQ>
///   </NOTAM>
/// </NOTAMS>
/// ```
///
/// **New format** (pibs.nats.co.uk/operational/pibs/PIB.xml):
/// ```xml
/// <Notam PIBSection="WAR">
///   <Series>J</Series>
///   <Number>413</Number>
///   <Year>26</Year>
///   <ItemE>GPS JAMMING EXERCISE...</ItemE>
///   <QLine>
///     <FIR>EGTT</FIR>
///     <Code23>WE</Code23>
///     <Code45>LW</Code45>
///     <Coordinates>5224N00131W</Coordinates>
///     <Radius>2</Radius>
///   </QLine>
///   <StartValidity>2601010800</StartValidity>
///   <EndValidity>2601011600</EndValidity>
///   <ItemA>EGTT</ItemA>
/// </Notam>
/// ```
///
/// Format detection: if we see `<NOTAM>` (all-caps) we use legacy parsing;
/// if we see `<Notam` (CamelCase, possibly with attributes) we use the new format.
/// Both can coexist in the same parse pass.
fn parse_nats_xml(xml_body: &str) -> Vec<NatsNotam> {
    let mut reader = Reader::from_str(xml_body);
    let mut buf = Vec::new();
    let mut notams = Vec::new();

    // Current parsing state
    let mut in_notam = false;
    // Whether we are inside a `<QLine>` block (new format only).
    let mut in_qline = false;
    // Tracks which format the current `<Notam>` element uses.
    let mut is_new_format = false;
    let mut current_tag = String::new();

    // Fields collected while parsing a single NOTAM element
    let mut id = String::new();
    let mut text = String::new();
    let mut q_line = String::new();
    let mut start_validity = String::new();
    let mut end_validity = String::new();
    let mut location = String::new();

    // New-format specific accumulators used to reconstruct id + q_line
    let mut series = String::new();
    let mut number = String::new();
    let mut year = String::new();
    let mut qline_fir = String::new();
    let mut qline_code23 = String::new();
    let mut qline_code45 = String::new();
    let mut qline_coords = String::new();
    let mut qline_radius = String::new();

    // Reset all per-NOTAM accumulators.
    macro_rules! reset_fields {
        () => {
            id.clear();
            text.clear();
            q_line.clear();
            start_validity.clear();
            end_validity.clear();
            location.clear();
            series.clear();
            number.clear();
            year.clear();
            qline_fir.clear();
            qline_code23.clear();
            qline_code45.clear();
            qline_coords.clear();
            qline_radius.clear();
        };
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let tag_upper = tag_name.to_uppercase();

                if tag_upper == "NOTAM" {
                    in_notam = true;
                    in_qline = false;
                    // Detect format: the raw tag is "NOTAM" (all-caps) for legacy,
                    // "Notam" (CamelCase) for the new format.
                    is_new_format = tag_name == "Notam";
                    reset_fields!();
                }

                if in_notam && tag_upper == "QLINE" {
                    in_qline = true;
                }

                if in_notam {
                    current_tag = tag_upper;
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_notam => {
                let content = e.unescape().unwrap_or_default().to_string();
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    // Skip whitespace-only text nodes
                } else if in_qline {
                    // Inside <QLine> — dispatch on the child tag name
                    match current_tag.as_str() {
                        "FIR" => qline_fir.push_str(trimmed),
                        "CODE23" => qline_code23.push_str(trimmed),
                        "CODE45" => qline_code45.push_str(trimmed),
                        "COORDINATES" => qline_coords.push_str(trimmed),
                        "RADIUS" => qline_radius.push_str(trimmed),
                        _ => {}
                    }
                } else {
                    match current_tag.as_str() {
                        // ---- New format fields (CamelCase tags uppercased) ----
                        "SERIES" => series.push_str(trimmed),
                        "NUMBER" => number.push_str(trimmed),
                        "YEAR" => year.push_str(trimmed),

                        // ---- Legacy format fields ----
                        "ID" => {
                            if !id.is_empty() {
                                id.push('/');
                            }
                            id.push_str(trimmed);
                        }

                        // ---- Shared / union fields ----
                        // Text body: <ITEME> (legacy) or <ItemE> (new) both uppercase to ITEME
                        "ITEME" | "E" | "FREETEXT" | "TEXT" | "BODY" => {
                            text.push_str(trimmed);
                        }
                        // Q-line as a flat string (legacy only; new format uses <QLine> children)
                        "ITEMQ" | "Q" | "QUALIFICATIONLINE" => {
                            q_line.push_str(trimmed);
                        }
                        // Start validity
                        "ITEMB" | "B" | "STARTVALIDITY" | "FROM" | "VALIDFROM" => {
                            start_validity.push_str(trimmed);
                        }
                        // End validity
                        "ITEMC" | "C" | "ENDVALIDITY" | "TO" | "VALIDTO" => {
                            end_validity.push_str(trimmed);
                        }
                        // Location / FIR
                        "ITEMA" | "A" | "LOCATION" => {
                            location.push_str(trimmed);
                        }
                        // Legacy combined text field
                        "ALL" => {
                            if text.is_empty() {
                                text.push_str(trimmed);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let tag_upper = tag_name.to_uppercase();

                if tag_upper == "QLINE" {
                    in_qline = false;
                }

                if tag_upper == "NOTAM" && in_notam {
                    in_notam = false;

                    // --- New format: reconstruct ID from Series/Number/Year ---
                    if is_new_format && id.is_empty() && !series.is_empty() && !number.is_empty()
                    {
                        // Zero-pad the number to 4 digits (e.g. "413" -> "0413")
                        let padded_num = format!("{:0>4}", number.trim());
                        if year.is_empty() {
                            id = format!("{}{}", series.trim(), padded_num);
                        } else {
                            id = format!("{}{}/{}", series.trim(), padded_num, year.trim());
                        }
                    }

                    // --- New format: reconstruct Q-line from QLine children ---
                    if is_new_format && q_line.is_empty() {
                        // Reconstruct: FIR/QCODE23CODE45/IV/BO/W/000/999/COORDSRADIUS
                        // The Q-code is "Q" + Code23 + Code45 (e.g. "Q" + "WE" + "LW" = "QWELW")
                        let fir = if !qline_fir.is_empty() {
                            qline_fir.trim().to_string()
                        } else if !location.is_empty() {
                            location.trim().to_string()
                        } else {
                            String::new()
                        };

                        if !qline_code23.is_empty() || !qline_code45.is_empty() {
                            let qcode = format!(
                                "Q{}{}",
                                qline_code23.trim().to_uppercase(),
                                qline_code45.trim().to_uppercase()
                            );
                            // Build coordinate suffix: "5224N00131W002"
                            let coord_suffix = if !qline_coords.is_empty() {
                                let radius = if !qline_radius.is_empty() {
                                    format!("{:0>3}", qline_radius.trim())
                                } else {
                                    "999".to_string()
                                };
                                format!("{}{}", qline_coords.trim(), radius)
                            } else {
                                String::new()
                            };

                            // Assemble: FIR/QCODE/IV/BO/W/000/999/COORDS
                            q_line = format!(
                                "{}/{}/IV/BO/W/000/999/{}",
                                fir, qcode, coord_suffix
                            );
                        }
                    }

                    // If we still don't have a Q-line, try to extract from text
                    if q_line.is_empty()
                        && !text.is_empty()
                        && let Some(q) = extract_qline_from_text(&text)
                    {
                        q_line = q;
                    }

                    // Only add if we have some identifying content
                    if !id.is_empty() || !text.is_empty() {
                        notams.push(NatsNotam {
                            id: if id.is_empty() {
                                // Generate a hash-based ID
                                use std::collections::hash_map::DefaultHasher;
                                use std::hash::{Hash, Hasher};
                                let mut hasher = DefaultHasher::new();
                                text.hash(&mut hasher);
                                format!("NATS_{}", hasher.finish())
                            } else {
                                id.clone()
                            },
                            text: text.clone(),
                            q_line: q_line.clone(),
                            start_validity: if start_validity.is_empty() {
                                None
                            } else {
                                Some(start_validity.clone())
                            },
                            end_validity: if end_validity.is_empty() {
                                None
                            } else {
                                Some(end_validity.clone())
                            },
                            location: if location.is_empty() {
                                None
                            } else {
                                Some(location.clone())
                            },
                        });
                    }
                }
                current_tag.clear();
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => {
                warn!(error = %e, "Error parsing NATS XML at position {}", reader.error_position());
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    notams
}

/// Try to extract a Q-line from the full NOTAM text body.
/// Q-lines start with "Q)" and contain slash-separated fields.
fn extract_qline_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Q)") {
            return Some(trimmed.trim_start_matches("Q)").trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Source implementation
// ---------------------------------------------------------------------------

pub struct NotamSource {
    /// NOTAM IDs we have already emitted so we don't broadcast duplicates.
    seen: Mutex<HashSet<String>>,

    /// Epoch timestamp of the last successful NATS poll (seconds).
    /// We only poll NATS once per hour since the feed updates hourly.
    last_nats_poll_epoch: Mutex<u64>,
}

impl NotamSource {
    pub fn new() -> Self {
        // Check for FAA API credentials at startup (for future integration)
        if let (Ok(client_id), Ok(client_secret)) = (
            std::env::var("FAA_NOTAM_CLIENT_ID"),
            std::env::var("FAA_NOTAM_CLIENT_SECRET"),
        )
            && !client_id.is_empty()
            && !client_secret.is_empty()
        {
            info!("FAA NOTAM API credentials found — FAA provider not yet implemented");
            // TODO: Implement FAA REST API client using api.faa.gov/s/notam/api/v4/notams
            // Authentication: client_id + client_secret headers
            // Supports US NOTAMs + international via KZZZ designator
            // See docs/NOTAM_APIS.md for full API documentation
        }

        Self {
            seen: Mutex::new(HashSet::new()),
            last_nats_poll_epoch: Mutex::new(0),
        }
    }

    /// Poll the NATS UK XML feed for conflict-relevant NOTAMs.
    /// Only runs once per hour since the feed is updated hourly.
    async fn poll_nats_uk(
        &self,
        ctx: &SourceContext,
    ) -> anyhow::Result<Vec<InsertableEvent>> {
        // Rate-limit: only poll NATS once per hour
        {
            let mut last_epoch = self.last_nats_poll_epoch.lock().unwrap_or_else(|e| e.into_inner());
            let now_epoch = Utc::now().timestamp() as u64;
            if now_epoch - *last_epoch < NATS_POLL_INTERVAL_SECS {
                debug!(
                    seconds_since_last = now_epoch - *last_epoch,
                    "Skipping NATS UK poll — too soon (hourly interval)"
                );
                return Ok(Vec::new());
            }
            *last_epoch = now_epoch;
        }

        debug!("Polling NATS UK XML feed for conflict-relevant NOTAMs");

        let resp = ctx.http.get(NATS_UK_URL).send().await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to fetch NATS UK NOTAM feed");
                // Reset the timer so we retry next poll cycle rather than waiting an hour
                let mut last_epoch = self.last_nats_poll_epoch.lock().unwrap_or_else(|e| e.into_inner());
                *last_epoch = 0;
                return Ok(Vec::new());
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            warn!(%status, "NATS UK NOTAM feed returned error status");
            let mut last_epoch = self.last_nats_poll_epoch.lock().unwrap_or_else(|e| e.into_inner());
            *last_epoch = 0;
            return Ok(Vec::new());
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Failed to read NATS UK response body");
                let mut last_epoch = self.last_nats_poll_epoch.lock().unwrap_or_else(|e| e.into_inner());
                *last_epoch = 0;
                return Ok(Vec::new());
            }
        };

        let notams = parse_nats_xml(&body);
        debug!(total_parsed = notams.len(), "Parsed NOTAMs from NATS UK feed");

        let mut events = Vec::new();
        for notam in notams {
            if let Some(event) = self.process_nats_notam(&notam) {
                events.push(event);
            }
        }

        if !events.is_empty() {
            info!(
                count = events.len(),
                "Conflict-relevant NOTAMs from NATS UK feed"
            );
        }

        Ok(events)
    }

    /// Process a single NATS UK NOTAM, filtering for conflict-relevant content.
    /// Only emits events that match conflict Q-codes or GPS jamming text patterns.
    fn process_nats_notam(&self, notam: &NatsNotam) -> Option<InsertableEvent> {
        let qcode = extract_qcode(&notam.q_line);
        let fir = extract_fir_from_qline(&notam.q_line);

        // Filter: must match a conflict Q-code OR contain GPS jamming text
        let is_conflict_qcode = is_nats_conflict_qcode(&qcode);
        let is_gps = is_gps_jamming_text(&notam.text);

        if !is_conflict_qcode && !is_gps {
            return None;
        }

        // Deduplication (prefix NATS IDs to avoid collision with Autorouter)
        let dedup_key = format!("nats_{}", notam.id);
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.contains(&dedup_key) {
                return None;
            }
            seen.insert(dedup_key);
        }

        let severity_str = if is_gps {
            "warning" // GPS jamming exercises are important but usually planned
        } else {
            classify_severity(&qcode, &fir)
        };
        let severity = Severity::from_str_lossy(severity_str);

        let notam_type_label = if is_gps {
            "GPS/GNSS Interference"
        } else {
            qcode_type(&qcode)
        };

        // Try to extract coordinates: Q-line → airport code → FIR center
        let loc_code = notam.location.as_deref().unwrap_or("");
        let airport_info = airport_coords(loc_code);
        let (lat, lon) = parse_qline_coords(&notam.q_line)
            .or_else(|| airport_info.map(|(lat, lon, _)| (lat, lon)))
            .or_else(|| fir_center(&fir))
            .map(|(lat, lon)| (Some(lat), Some(lon)))
            .unwrap_or((None, None));
        let airport_name = airport_info.map(|(_, _, name)| name);

        // Extract radius from Q-line (last 3 digits after coords, in nautical miles)
        let radius_nm: Option<u32> = notam.q_line.split('/').next_back()
            .and_then(|s| {
                let s = s.trim();
                if s.len() >= 3 { s[s.len()-3..].parse().ok() } else { None }
            })
            .filter(|&r: &u32| r > 0 && r < 999);

        let fir_display = if fir.is_empty() { "UK" } else { &fir };

        // Decode the Q-code for human-readable enrichment
        let decode_info = decode_qcode(&qcode);
        let decoded_text = decode_notam_text(&notam.text);

        let data = json!({
            "notam_id": notam.id,
            "fir": fir_display,
            "fir_label": fir_label(fir_display),
            "text": notam.text,
            "q_line": notam.q_line,
            "qcode": qcode,
            "notam_type": notam_type_label,
            "severity": severity_str,
            "start_validity": notam.start_validity,
            "end_validity": notam.end_validity,
            "location": notam.location,
            "airport_name": airport_name,
            "radius_nm": radius_nm,
            "provider": "nats_uk",
            "is_gps_jamming": is_gps,
            // Decoded enrichment fields
            "qcode_description": decode_info.description,
            "qcode_category": decode_info.category,
            "is_routine": decode_info.routine,
            "significance": decode_info.significance,
            "decoded_text": decoded_text,
        });

        // Build a descriptive title — prefer airport name over generic FIR label
        let location_label = if let Some(name) = airport_name {
            format!("{} ({})", name, loc_code)
        } else {
            fir_label(fir_display).to_string()
        };
        let title = if is_gps {
            format!("NOTAM: GPS/GNSS Interference in {}", fir_label(fir_display))
        } else {
            format!("{}: {}", location_label, decode_info.description)
        };

        let mut tags = vec![qcode_topic_tag(&qcode).to_string()];
        if !fir.is_empty() {
            tags.push(fir_topic_tag(&fir));
        }
        if is_gps {
            tags.push("gps-jamming".to_string());
        }

        Some(InsertableEvent {
            event_time: Utc::now(),
            source_type: SourceType::Notam,
            source_id: Some("notam".to_string()),
            longitude: lon,
            latitude: lat,
            region_code: Some("western-europe".to_string()),
            entity_id: Some(notam.id.clone()),
            entity_name: Some(fir_label(fir_display).to_string()),
            event_type: EventType::NotamEvent,
            severity,
            confidence: None,
            tags,
            title: Some(title),
            description: None,
            payload: data,
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

impl Default for NotamSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for NotamSource {
    fn id(&self) -> &str {
        "notam"
    }

    fn name(&self) -> &str {
        "NOTAMs / Airspace"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(600) // 10 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        // 1. NATS UK XML feed (hourly)
        let events = self.poll_nats_uk(ctx).await?;

        // 2. TODO: FAA REST API (future — requires FAA_NOTAM_CLIENT_ID + FAA_NOTAM_CLIENT_SECRET)
        // When implemented, poll api.faa.gov/s/notam/api/v4/notams with:
        //   - Headers: client_id, client_secret
        //   - Query KZZZ designator for international NOTAMs
        //   - Query specific US locations for military/TFR NOTAMs
        //   - Free-text search for "GPS UNRELIABLE", "AIRSPACE CLOSED"

        if !events.is_empty() {
            debug!(count = events.len(), "NOTAM events from NATS UK");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_qcode() {
        assert_eq!(
            extract_qcode("OIIX/QRRCA/IV/NBO/AE/000/999/3548N05132E005"),
            "QRRCA"
        );
        assert_eq!(
            extract_qcode("OBBB/QWELW/IV/BO/W/000/999/2600N05200E100"),
            "QWELW"
        );
        assert_eq!(extract_qcode(""), "");
        assert_eq!(extract_qcode("OIIX"), "");
    }

    #[test]
    fn test_is_priority_qcode() {
        assert!(is_priority_qcode("QRRCA"));
        assert!(is_priority_qcode("QWELW"));
        assert!(is_priority_qcode("QAFXX"));
        assert!(is_priority_qcode("QNMAS"));
        assert!(!is_priority_qcode("QMXXX")); // Not a priority prefix
        assert!(!is_priority_qcode("QXXXX"));
    }

    #[test]
    fn test_classify_severity() {
        // Critical FIR + TRA/Danger (QR)
        assert_eq!(classify_severity("QRRCA", "OIIX"), "critical");
        assert_eq!(classify_severity("QRRCA", "ORBB"), "critical");
        assert_eq!(classify_severity("QRRCA", "OBBB"), "critical");

        // Critical FIR + Warning (QW)
        assert_eq!(classify_severity("QWELW", "OIIX"), "critical");

        // Non-critical FIR + QR
        assert_eq!(classify_severity("QRRCA", "LTBB"), "warning");

        // Critical FIR + non-TRA code
        assert_eq!(classify_severity("QNMAS", "OIIX"), "warning");

        // Non-critical FIR + non-TRA code
        assert_eq!(classify_severity("QAFXX", "LCCC"), "warning");
    }

    #[test]
    fn test_qcode_type() {
        assert_eq!(qcode_type("QRRCA"), "Restricted Area");
        assert_eq!(qcode_type("QWELW"), "Warning");
        assert_eq!(qcode_type("QAFXX"), "Airfield");
        assert_eq!(qcode_type("QNMAS"), "Navigation");
        assert_eq!(qcode_type("QXXXX"), "Other");
    }

    #[test]
    fn test_fir_label() {
        assert_eq!(fir_label("OIIX"), "Tehran FIR (Iran)");
        assert_eq!(fir_label("ORBB"), "Baghdad FIR (Iraq)");
        assert_eq!(fir_label("ZZZZ"), "Unknown FIR");
    }

    #[test]
    fn test_deduplication() {
        let source = NotamSource::new();
        {
            let mut seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
            seen.insert("A1234/24".to_string());
        }
        let seen = source.seen.lock().unwrap_or_else(|e| e.into_inner());
        assert!(seen.contains("A1234/24"));
        assert!(!seen.contains("B5678/24"));
    }

    // -------------------------------------------------------------------
    // NATS UK XML parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_is_nats_conflict_qcode() {
        assert!(is_nats_conflict_qcode("QRALC"));
        assert!(is_nats_conflict_qcode("QRTCA"));
        assert!(is_nats_conflict_qcode("QFAHC"));
        assert!(is_nats_conflict_qcode("QFALC"));
        assert!(!is_nats_conflict_qcode("QRRCA")); // priority but not in NATS conflict list
        assert!(!is_nats_conflict_qcode("QXXXX"));
    }

    #[test]
    fn test_is_gps_jamming_text() {
        assert!(is_gps_jamming_text("GPS JAMMING EXERCISE IN PROGRESS"));
        assert!(is_gps_jamming_text("GNSS INTERFERENCE EXPECTED"));
        assert!(is_gps_jamming_text("GPS UNRELIABLE WITHIN 50NM"));
        assert!(is_gps_jamming_text("gnss testing in progress")); // case insensitive
        assert!(is_gps_jamming_text("GPS INTF POSSIBLE"));
        assert!(!is_gps_jamming_text("RUNWAY CLOSED FOR MAINTENANCE"));
        assert!(!is_gps_jamming_text("NORMAL OPERATIONS"));
    }

    #[test]
    fn test_parse_qline_coords() {
        // Standard format: 5130N00027W (DDMM N DDDMM W)
        let coords = parse_qline_coords("EGTT/QWELW/IV/BO/W/000/100/5130N00027W025");
        assert!(coords.is_some());
        let (lat, lon) = coords.unwrap();
        assert!((lat - 51.5).abs() < 0.01);
        assert!((lon - (-0.45)).abs() < 0.01);

        // Tehran: 3548N05132E
        let coords2 = parse_qline_coords("OIIX/QRRCA/IV/NBO/AE/000/999/3548N05132E005");
        assert!(coords2.is_some());
        let (lat2, lon2) = coords2.unwrap();
        assert!((lat2 - 35.8).abs() < 0.1);
        assert!((lon2 - 51.53).abs() < 0.1);

        // Extended format with tenths of minutes: 51305N000273W (DDMMd N DDDMMd W)
        // 51 deg 30.5 min N, 000 deg 27.3 min W
        let coords3 =
            parse_qline_coords("EGTT/QWELW/IV/BO/W/000/100/51305N000273W025");
        assert!(
            coords3.is_some(),
            "Should parse extended format with tenths of minutes"
        );
        let (lat3, lon3) = coords3.unwrap();
        // 51 + 30.5/60 = 51.50833...
        assert!(
            (lat3 - 51.5083).abs() < 0.01,
            "lat3={lat3}, expected ~51.5083"
        );
        // -(0 + 27.3/60) = -0.455
        assert!(
            (lon3 - (-0.455)).abs() < 0.01,
            "lon3={lon3}, expected ~-0.455"
        );

        // Extended format eastern hemisphere: 35485N051323E
        // 35 deg 48.5 min N, 051 deg 32.3 min E
        let coords4 =
            parse_qline_coords("OIIX/QRRCA/IV/NBO/AE/000/999/35485N051323E005");
        assert!(
            coords4.is_some(),
            "Should parse extended format eastern hemisphere"
        );
        let (lat4, lon4) = coords4.unwrap();
        assert!(
            (lat4 - 35.8083).abs() < 0.01,
            "lat4={lat4}, expected ~35.808"
        );
        assert!(
            (lon4 - 51.538).abs() < 0.01,
            "lon4={lon4}, expected ~51.538"
        );

        // Extended format southern hemisphere: 33595S018302E
        // 33 deg 59.5 min S, 018 deg 30.2 min E
        let coords5 =
            parse_qline_coords("FACT/QRALC/IV/NBO/AE/000/200/33595S018302E010");
        assert!(coords5.is_some(), "Should parse southern hemisphere extended");
        let (lat5, lon5) = coords5.unwrap();
        assert!(lat5 < 0.0, "Southern hemisphere should be negative");
        assert!(
            (lat5 - (-33.9917)).abs() < 0.01,
            "lat5={lat5}, expected ~-33.992"
        );
        assert!(
            (lon5 - 18.5033).abs() < 0.01,
            "lon5={lon5}, expected ~18.503"
        );

        // Too short
        assert!(parse_qline_coords("short").is_none());

        // Empty
        assert!(parse_qline_coords("").is_none());
    }

    #[test]
    fn test_extract_fir_from_qline() {
        assert_eq!(
            extract_fir_from_qline("EGTT/QWELW/IV/BO/W/000/100/5130N00027W025"),
            "EGTT"
        );
        assert_eq!(
            extract_fir_from_qline("OIIX/QRRCA/IV/NBO/AE/000/999/3548N05132E005"),
            "OIIX"
        );
        assert_eq!(extract_fir_from_qline(""), "");
    }

    #[test]
    fn test_extract_qline_from_text() {
        let text = "A1234/24 NOTAMN\nQ) EGTT/QWELW/IV/BO/W/000/100\nE) GPS JAMMING EXERCISE";
        let q = extract_qline_from_text(text);
        assert_eq!(q, Some("EGTT/QWELW/IV/BO/W/000/100".to_string()));

        let no_q = extract_qline_from_text("No Q-line in this text");
        assert!(no_q.is_none());
    }

    // -------------------------------------------------------------------
    // Legacy format XML parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_nats_xml_legacy_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<NOTAMS>
    <NOTAM>
        <ID>A0001/26</ID>
        <ITEME>GPS JAMMING EXERCISE IN AREA BOUNDED BY 5200N 00100W</ITEME>
        <ITEMQ>EGTT/QWELW/IV/BO/W/000/100/5130N00027W025</ITEMQ>
        <ITEMB>2601010800</ITEMB>
        <ITEMC>2601011600</ITEMC>
        <ITEMA>EGTT</ITEMA>
    </NOTAM>
    <NOTAM>
        <ID>B0002/26</ID>
        <ITEME>RUNWAY 09/27 CLOSED FOR RESURFACING</ITEME>
        <ITEMQ>EGTT/QMRLC/IV/NBO/A/000/999/5130N00027W005</ITEMQ>
        <ITEMA>EGLL</ITEMA>
    </NOTAM>
    <NOTAM>
        <ID>C0003/26</ID>
        <ITEME>RESTRICTED AREA ESTABLISHED DUE TO MILITARY EXERCISE</ITEME>
        <ITEMQ>EGPX/QRALC/IV/NBO/AE/000/200/5700N00300W030</ITEMQ>
        <ITEMB>2601150600</ITEMB>
        <ITEMC>2601151800</ITEMC>
        <ITEMA>EGPX</ITEMA>
    </NOTAM>
</NOTAMS>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 3);

        // First NOTAM: GPS jamming
        assert_eq!(notams[0].id, "A0001/26");
        assert!(notams[0].text.contains("GPS JAMMING"));
        assert!(notams[0].q_line.contains("QWELW"));
        assert_eq!(notams[0].location.as_deref(), Some("EGTT"));

        // Second NOTAM: runway closure (not conflict-relevant)
        assert_eq!(notams[1].id, "B0002/26");
        assert!(notams[1].text.contains("RUNWAY"));

        // Third NOTAM: restricted area (conflict Q-code QRALC)
        assert_eq!(notams[2].id, "C0003/26");
        assert!(notams[2].q_line.contains("QRALC"));
    }

    // -------------------------------------------------------------------
    // New format XML parsing tests (pibs.nats.co.uk)
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_nats_xml_new_format_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PIB>
    <Notam PIBSection="WAR">
        <Series>J</Series>
        <Number>413</Number>
        <Year>26</Year>
        <ItemE>GPS JAMMING EXERCISE IN AREA BOUNDED BY 5224N 00131W</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>WE</Code23>
            <Code45>LW</Code45>
            <Coordinates>5224N00131W</Coordinates>
            <Radius>2</Radius>
        </QLine>
        <StartValidity>2601010800</StartValidity>
        <EndValidity>2601011600</EndValidity>
        <ItemA>EGTT</ItemA>
    </Notam>
    <Notam PIBSection="AD">
        <Series>H</Series>
        <Number>52</Number>
        <Year>26</Year>
        <ItemE>RUNWAY 09/27 CLOSED FOR RESURFACING</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>MR</Code23>
            <Code45>LC</Code45>
            <Coordinates>5130N00027W</Coordinates>
            <Radius>5</Radius>
        </QLine>
        <ItemA>EGLL</ItemA>
    </Notam>
    <Notam PIBSection="WAR">
        <Series>J</Series>
        <Number>99</Number>
        <Year>26</Year>
        <ItemE>RESTRICTED AREA ESTABLISHED DUE TO MILITARY EXERCISE</ItemE>
        <QLine>
            <FIR>EGPX</FIR>
            <Code23>RA</Code23>
            <Code45>LC</Code45>
            <Coordinates>5700N00300W</Coordinates>
            <Radius>30</Radius>
        </QLine>
        <StartValidity>2601150600</StartValidity>
        <EndValidity>2601151800</EndValidity>
        <ItemA>EGPX</ItemA>
    </Notam>
</PIB>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 3);

        // First NOTAM: GPS jamming — ID reconstructed from Series+Number+Year
        assert_eq!(notams[0].id, "J0413/26");
        assert!(notams[0].text.contains("GPS JAMMING"));
        assert_eq!(notams[0].location.as_deref(), Some("EGTT"));
        assert_eq!(
            notams[0].start_validity.as_deref(),
            Some("2601010800")
        );
        assert_eq!(
            notams[0].end_validity.as_deref(),
            Some("2601011600")
        );

        // Q-line reconstructed: should contain QWELW and coordinates
        let qcode0 = extract_qcode(&notams[0].q_line);
        assert_eq!(qcode0, "QWELW");
        let fir0 = extract_fir_from_qline(&notams[0].q_line);
        assert_eq!(fir0, "EGTT");

        // Coordinates should be parseable from the reconstructed Q-line
        let coords0 = parse_qline_coords(&notams[0].q_line);
        assert!(coords0.is_some(), "Should parse coordinates from new-format Q-line");
        let (lat, lon) = coords0.unwrap();
        assert!((lat - 52.4).abs() < 0.1);
        assert!((lon - (-1.517)).abs() < 0.1);

        // Second NOTAM: runway closure — Number zero-padded
        assert_eq!(notams[1].id, "H0052/26");
        let qcode1 = extract_qcode(&notams[1].q_line);
        assert_eq!(qcode1, "QMRLC");

        // Third NOTAM: restricted area
        assert_eq!(notams[2].id, "J0099/26");
        let qcode2 = extract_qcode(&notams[2].q_line);
        assert_eq!(qcode2, "QRALC");
        assert!(is_nats_conflict_qcode(&qcode2));
        let fir2 = extract_fir_from_qline(&notams[2].q_line);
        assert_eq!(fir2, "EGPX");
    }

    #[test]
    fn test_parse_nats_xml_new_format_qcode_reconstruction() {
        // Test that the Q-code reconstruction works correctly for various code pairs
        let xml = r#"<PIB>
    <Notam PIBSection="FIR">
        <Series>A</Series>
        <Number>1</Number>
        <Year>26</Year>
        <ItemE>AERODROME CLOSED</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>FA</Code23>
            <Code45>LC</Code45>
            <Coordinates>5130N00027W</Coordinates>
            <Radius>5</Radius>
        </QLine>
        <ItemA>EGLL</ItemA>
    </Notam>
</PIB>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 1);
        assert_eq!(notams[0].id, "A0001/26");

        let qcode = extract_qcode(&notams[0].q_line);
        assert_eq!(qcode, "QFALC");
        assert!(is_nats_conflict_qcode(&qcode), "QFALC is a NATS conflict Q-code");
    }

    #[test]
    fn test_parse_nats_xml_new_format_no_coords() {
        // Some NOTAMs might not have coordinates in QLine
        let xml = r#"<PIB>
    <Notam PIBSection="FIR">
        <Series>B</Series>
        <Number>7</Number>
        <Year>26</Year>
        <ItemE>GPS UNRELIABLE IN LONDON FIR</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>WE</Code23>
            <Code45>LW</Code45>
        </QLine>
        <StartValidity>2603010000</StartValidity>
        <EndValidity>2603012359</EndValidity>
        <ItemA>EGTT</ItemA>
    </Notam>
</PIB>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 1);
        assert_eq!(notams[0].id, "B0007/26");

        // Q-line should still have the Q-code even without coordinates
        let qcode = extract_qcode(&notams[0].q_line);
        assert_eq!(qcode, "QWELW");

        // GPS jamming detection should still work on the text
        assert!(is_gps_jamming_text(&notams[0].text));
    }

    #[test]
    fn test_parse_nats_xml_new_format_end_to_end() {
        // Full end-to-end test: parse new-format XML and process through NotamSource
        let source = NotamSource::new();

        let xml = r#"<PIB>
    <Notam PIBSection="WAR">
        <Series>J</Series>
        <Number>413</Number>
        <Year>26</Year>
        <ItemE>GPS JAMMING EXERCISE IN AREA BOUNDED BY 5224N 00131W</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>WE</Code23>
            <Code45>LW</Code45>
            <Coordinates>5224N00131W</Coordinates>
            <Radius>2</Radius>
        </QLine>
        <StartValidity>2601010800</StartValidity>
        <EndValidity>2601011600</EndValidity>
        <ItemA>EGTT</ItemA>
    </Notam>
</PIB>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 1);

        let event = source.process_nats_notam(&notams[0]);
        assert!(event.is_some(), "GPS jamming NOTAM should produce an event");

        let event = event.unwrap();
        assert_eq!(event.source_type, SourceType::Notam);
        assert_eq!(event.event_type, EventType::NotamEvent);
        assert!(event.title.as_ref().unwrap().contains("GPS/GNSS Interference"));
        assert!(event.tags.contains(&"gps-jamming".to_string()));
        assert!(event.latitude.is_some());
        assert!(event.longitude.is_some());

        let payload = &event.payload;
        assert_eq!(payload["provider"], "nats_uk");
        assert_eq!(payload["is_gps_jamming"], true);
        assert_eq!(payload["notam_id"], "J0413/26");
        assert_eq!(payload["fir"], "EGTT");
    }

    #[test]
    fn test_parse_nats_xml_extended_coords_end_to_end() {
        // End-to-end test with extended coordinate format (tenths of minutes)
        // as used by EAD/SDO European feeds
        let source = NotamSource::new();

        let xml = r#"<PIB>
    <Notam PIBSection="WAR">
        <Series>J</Series>
        <Number>800</Number>
        <Year>26</Year>
        <ItemE>GPS JAMMING EXERCISE IN AREA BOUNDED BY 5230N 00145W</ItemE>
        <QLine>
            <FIR>EGTT</FIR>
            <Code23>WE</Code23>
            <Code45>LW</Code45>
            <Coordinates>52305N001453W</Coordinates>
            <Radius>25</Radius>
        </QLine>
        <StartValidity>2603010800</StartValidity>
        <EndValidity>2603011600</EndValidity>
        <ItemA>EGTT</ItemA>
    </Notam>
</PIB>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 1);

        // Coordinates should be parseable from the reconstructed Q-line (extended format)
        let coords = parse_qline_coords(&notams[0].q_line);
        assert!(
            coords.is_some(),
            "Should parse extended coords from Q-line: {}",
            notams[0].q_line
        );
        let (lat, lon) = coords.unwrap();
        // 52 deg 30.5 min N = 52.5083..., 001 deg 45.3 min W = -1.755
        assert!(
            (lat - 52.5083).abs() < 0.01,
            "lat={lat}, expected ~52.508"
        );
        assert!(
            (lon - (-1.755)).abs() < 0.01,
            "lon={lon}, expected ~-1.755"
        );

        // Process into event and verify lat/lon are set
        let event = source.process_nats_notam(&notams[0]);
        assert!(event.is_some(), "GPS jamming NOTAM should produce an event");

        let event = event.unwrap();
        assert!(
            event.latitude.is_some(),
            "Extended-format NOTAM should have latitude"
        );
        assert!(
            event.longitude.is_some(),
            "Extended-format NOTAM should have longitude"
        );
        assert!(
            (event.latitude.unwrap() - 52.5083).abs() < 0.01,
            "Event latitude should match parsed value"
        );
        assert!(
            (event.longitude.unwrap() - (-1.755)).abs() < 0.01,
            "Event longitude should match parsed value"
        );
    }

    #[test]
    fn test_parse_nats_xml_mixed_formats() {
        // Both legacy and new format in the same document (unlikely but we handle it)
        let xml = r#"<root>
    <NOTAM>
        <ID>A0001/26</ID>
        <ITEME>GPS JAMMING EXERCISE LEGACY FORMAT</ITEME>
        <ITEMQ>EGTT/QWELW/IV/BO/W/000/100/5130N00027W025</ITEMQ>
        <ITEMA>EGTT</ITEMA>
    </NOTAM>
    <Notam PIBSection="WAR">
        <Series>J</Series>
        <Number>500</Number>
        <Year>26</Year>
        <ItemE>GPS JAMMING EXERCISE NEW FORMAT</ItemE>
        <QLine>
            <FIR>EGPX</FIR>
            <Code23>WE</Code23>
            <Code45>LW</Code45>
            <Coordinates>5700N00300W</Coordinates>
            <Radius>10</Radius>
        </QLine>
        <ItemA>EGPX</ItemA>
    </Notam>
</root>"#;

        let notams = parse_nats_xml(xml);
        assert_eq!(notams.len(), 2);

        // Legacy
        assert_eq!(notams[0].id, "A0001/26");
        assert!(notams[0].text.contains("LEGACY FORMAT"));

        // New format
        assert_eq!(notams[1].id, "J0500/26");
        assert!(notams[1].text.contains("NEW FORMAT"));
        let fir = extract_fir_from_qline(&notams[1].q_line);
        assert_eq!(fir, "EGPX");
    }

    // -------------------------------------------------------------------
    // Process / filter tests (work with both formats since they produce
    // the same NatsNotam struct)
    // -------------------------------------------------------------------

    #[test]
    fn test_process_nats_notam_gps_jamming() {
        let source = NotamSource::new();

        let notam = NatsNotam {
            id: "A0001/26".to_string(),
            text: "GPS JAMMING EXERCISE IN AREA BOUNDED BY 5200N 00100W".to_string(),
            q_line: "EGTT/QWELW/IV/BO/W/000/100/5130N00027W025".to_string(),
            start_validity: Some("2601010800".to_string()),
            end_validity: Some("2601011600".to_string()),
            location: Some("EGTT".to_string()),
        };

        let event = source.process_nats_notam(&notam);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.source_type, SourceType::Notam);
        assert_eq!(event.event_type, EventType::NotamEvent);
        assert!(event.title.as_ref().unwrap().contains("GPS/GNSS Interference"));
        assert!(event.tags.contains(&"gps-jamming".to_string()));
        assert_eq!(event.region_code.as_deref(), Some("western-europe"));
        assert!(event.latitude.is_some());
        assert!(event.longitude.is_some());

        // Check payload
        let payload = &event.payload;
        assert_eq!(payload["provider"], "nats_uk");
        assert_eq!(payload["is_gps_jamming"], true);
    }

    #[test]
    fn test_process_nats_notam_conflict_qcode() {
        let source = NotamSource::new();

        let notam = NatsNotam {
            id: "C0003/26".to_string(),
            text: "RESTRICTED AREA ESTABLISHED DUE TO MILITARY EXERCISE".to_string(),
            q_line: "EGPX/QRALC/IV/NBO/AE/000/200/5700N00300W030".to_string(),
            start_validity: Some("2601150600".to_string()),
            end_validity: Some("2601151800".to_string()),
            location: Some("EGPX".to_string()),
        };

        let event = source.process_nats_notam(&notam);
        assert!(event.is_some());

        let event = event.unwrap();
        assert!(event.title.as_ref().unwrap().to_lowercase().contains("restricted area"));
        assert!(!event.tags.contains(&"gps-jamming".to_string()));
    }

    #[test]
    fn test_process_nats_notam_filtered_out() {
        let source = NotamSource::new();

        // Runway closure — not a conflict Q-code and no GPS text
        let notam = NatsNotam {
            id: "B0002/26".to_string(),
            text: "RUNWAY 09/27 CLOSED FOR RESURFACING".to_string(),
            q_line: "EGTT/QMRLC/IV/NBO/A/000/999/5130N00027W005".to_string(),
            start_validity: None,
            end_validity: None,
            location: Some("EGLL".to_string()),
        };

        let event = source.process_nats_notam(&notam);
        assert!(event.is_none(), "Runway closure should be filtered out");
    }

    #[test]
    fn test_nats_deduplication() {
        let source = NotamSource::new();

        let notam = NatsNotam {
            id: "A0001/26".to_string(),
            text: "GPS JAMMING EXERCISE".to_string(),
            q_line: "EGTT/QWELW/IV/BO/W/000/100/5130N00027W025".to_string(),
            start_validity: None,
            end_validity: None,
            location: Some("EGTT".to_string()),
        };

        // First call should produce an event
        let event1 = source.process_nats_notam(&notam);
        assert!(event1.is_some());

        // Second call with same ID should be deduplicated
        let event2 = source.process_nats_notam(&notam);
        assert!(event2.is_none(), "Duplicate NATS NOTAM should be filtered");
    }

    #[test]
    fn test_qcode_type_facility() {
        assert_eq!(qcode_type("QFAHC"), "Facility");
        assert_eq!(qcode_type("QFALC"), "Facility");
    }

    #[test]
    fn test_fir_label_uk() {
        assert_eq!(fir_label("EGTT"), "London FIR (UK)");
        assert_eq!(fir_label("EGPX"), "Scottish FIR (UK)");
    }

    #[test]
    fn test_parse_nats_xml_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><NOTAMS></NOTAMS>"#;
        let notams = parse_nats_xml(xml);
        assert!(notams.is_empty());
    }

    #[test]
    fn test_parse_nats_xml_new_format_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><PIB></PIB>"#;
        let notams = parse_nats_xml(xml);
        assert!(notams.is_empty());
    }

    #[test]
    fn test_parse_nats_xml_malformed() {
        // Malformed XML should not panic — just return what we can parse
        let xml = r#"<NOTAMS><NOTAM><ID>X001</ID><ITEME>test</ITEME></NOTAM><broken"#;
        let notams = parse_nats_xml(xml);
        // Should get at least the first valid NOTAM
        assert_eq!(notams.len(), 1);
        assert_eq!(notams[0].id, "X001");
    }

    // -------------------------------------------------------------------
    // Q-code decode tests
    // -------------------------------------------------------------------

    #[test]
    fn test_decode_qcode_known_codes() {
        let info = decode_qcode("QFALC");
        assert_eq!(info.category, "Airfield Closure");
        assert!(info.description.contains("Aerodrome closed"));
        assert!(!info.routine);

        let info = decode_qcode("QFAHC");
        assert_eq!(info.category, "Airfield Closure");
        assert!(info.routine);

        let info = decode_qcode("QRALC");
        assert_eq!(info.category, "Restricted Area");
        assert!(!info.routine);
        assert!(info.description.contains("Restricted area"));

        let info = decode_qcode("QRTCA");
        assert_eq!(info.category, "Restricted Area");
        assert!(!info.routine);

        let info = decode_qcode("QWMLW");
        assert_eq!(info.category, "Military Warning");
        assert!(!info.routine);
        assert!(info.description.contains("Military exercise"));

        let info = decode_qcode("QWPLW");
        assert_eq!(info.category, "Military Warning");
        assert!(info.routine); // parachute exercises are routine

        let info = decode_qcode("QOBCE");
        assert_eq!(info.category, "Obstacle");
        assert!(info.routine);

        let info = decode_qcode("QNVAS");
        assert_eq!(info.category, "Navigation Warning");
        assert!(info.routine);

        let info = decode_qcode("QICAS");
        assert_eq!(info.category, "Navigation Warning");
        assert!(info.routine);

        let info = decode_qcode("QMRLC");
        assert_eq!(info.category, "Runway/Taxiway");
        assert!(info.routine);

        let info = decode_qcode("QMAHC");
        assert_eq!(info.category, "Runway/Taxiway");
        assert!(info.routine);
    }

    #[test]
    fn test_decode_qcode_prefix_fallback() {
        // Unknown Q-code with QR prefix should fall back to "Restricted Area"
        let info = decode_qcode("QRZZY");
        assert_eq!(info.category, "Restricted Area");
        assert!(!info.routine);

        // Unknown QW code
        let info = decode_qcode("QWZZY");
        assert_eq!(info.category, "Airspace Warning");
        assert!(!info.routine);

        // Completely unknown
        let info = decode_qcode("QXXXX");
        assert_eq!(info.category, "Other");
        assert!(info.routine);
    }

    #[test]
    fn test_decode_qcode_case_insensitive() {
        let info = decode_qcode("qfalc");
        assert_eq!(info.category, "Airfield Closure");
        assert!(info.description.contains("Aerodrome closed"));
    }

    #[test]
    fn test_decode_notam_text_basic() {
        let decoded = decode_notam_text("AD CLSD TO NON-SKED TFC");
        assert!(decoded.contains("Aerodrome"));
        assert!(decoded.contains("Closed"));
        assert!(decoded.contains("Non-Scheduled"));
        assert!(decoded.contains("Traffic"));
    }

    #[test]
    fn test_decode_notam_text_flight_level() {
        let decoded = decode_notam_text("TRA ACT FL100 TO FL250");
        assert!(decoded.contains("Temporary Reserved Area"));
        assert!(decoded.contains("Active"));
        assert!(decoded.contains("Flight Level 100"));
        assert!(decoded.contains("Flight Level 250"));
    }

    #[test]
    fn test_decode_notam_text_days_and_hours() {
        let decoded = decode_notam_text("MON TO FRI H24");
        assert!(decoded.contains("Monday"));
        assert!(decoded.contains("Friday"));
        assert!(decoded.contains("24 Hours"));
    }

    #[test]
    fn test_decode_notam_text_preserves_unknown() {
        // Text that has no known abbreviations should pass through unchanged
        let original = "SPECIAL NOTICE 12345";
        let decoded = decode_notam_text(original);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_notam_text_unserviceable() {
        let decoded = decode_notam_text("VOR U/S FOR MAINT");
        assert!(decoded.contains("VHF Omnidirectional Range"));
        assert!(decoded.contains("Unserviceable"));
        assert!(decoded.contains("Maintenance"));
    }

    #[test]
    fn test_payload_contains_decoded_fields() {
        let source = NotamSource::new();

        let notam = NatsNotam {
            id: "D0010/26".to_string(),
            text: "AD CLSD TO NON-SKED TFC".to_string(),
            q_line: "EGTT/QFALC/IV/NBO/AE/000/999/5130N00027W005".to_string(),
            start_validity: None,
            end_validity: None,
            location: Some("EGTT".to_string()),
        };

        let event = source.process_nats_notam(&notam);
        assert!(event.is_some());
        let event = event.unwrap();
        let p = &event.payload;

        assert_eq!(p["qcode_category"], "Airfield Closure");
        assert!(p["qcode_description"].as_str().unwrap().contains("Aerodrome closed"));
        assert_eq!(p["is_routine"], false);
        assert!(p["significance"].as_str().unwrap().contains("Common for night closures"));
        assert!(p["decoded_text"].as_str().unwrap().contains("Aerodrome"));
        assert!(p["decoded_text"].as_str().unwrap().contains("Closed"));
    }
}
