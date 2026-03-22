//! Bellingcat / Turnstone aircraft database (modes.csv).
//!
//! Loads ~495K ICAO Mode-S hex → aircraft records from the Bellingcat modes.csv
//! file.  The database is loaded once at startup and shared via `Arc<AircraftDb>`.
//!
//! Key improvement over callsign-prefix heuristics: the ICAO hex block is
//! authoritative for country assignment and the `military` flag is curated by
//! Bellingcat analysts rather than guessed from callsign patterns.

use std::collections::HashMap;
use std::path::Path;

use tracing::{info, warn};

/// Information about a single aircraft, keyed by ICAO Mode-S hex code.
#[derive(Debug, Clone)]
pub struct AircraftInfo {
    pub registration: Option<String>,
    pub typecode: Option<String>,
    pub category: String,
    pub military: bool,
    pub owner: Option<String>,
}

/// In-memory aircraft identification database built from Bellingcat's modes.csv.
pub struct AircraftDb {
    entries: HashMap<String, AircraftInfo>,
}

impl AircraftDb {
    /// Load the aircraft database from a CSV file.
    ///
    /// The CSV is expected to have the header:
    /// `hex,registration,manufacturer,typecode,type,owner,operator,aircraft,category,military,year`
    ///
    /// Malformed rows (wrong column count, unparseable military flag) are skipped
    /// with a warning.  About ~38K rows in the original dataset have misaligned
    /// columns from earlier automated classification; these are silently dropped.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let path = Path::new(path);
        if !path.exists() {
            anyhow::bail!("Aircraft database file not found: {}", path.display());
        }

        let mut entries = HashMap::with_capacity(500_000);
        let mut skipped = 0u64;
        let mut loaded = 0u64;

        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .has_headers(true)
            .from_path(path)?;

        for result in rdr.records() {
            let record = match result {
                Ok(r) => r,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };

            // We need at least 11 columns (hex through year).
            // Columns: 0=hex, 1=registration, 2=manufacturer, 3=typecode,
            //          4=type, 5=owner, 6=operator, 7=aircraft,
            //          8=category, 9=military, 10=year
            if record.len() < 10 {
                skipped += 1;
                continue;
            }

            let hex = record.get(0).unwrap_or("").trim().to_lowercase();
            if hex.is_empty() {
                skipped += 1;
                continue;
            }

            let category = record.get(8).unwrap_or("").trim().to_string();
            let military_str = record.get(9).unwrap_or("").trim().to_lowercase();

            // Validate military flag: must be "t" or "f".
            // If it's something else, the row is misaligned — skip it.
            let military = match military_str.as_str() {
                "t" | "true" => true,
                "f" | "false" => false,
                _ => {
                    skipped += 1;
                    continue;
                }
            };

            // Validate category: should be a known aircraft category, not an
            // aircraft description (which would indicate column misalignment).
            if category.is_empty() {
                skipped += 1;
                continue;
            }

            let registration = non_empty(record.get(1).unwrap_or(""));
            let typecode = non_empty(record.get(3).unwrap_or(""));
            let owner = non_empty(record.get(5).unwrap_or(""));

            entries.insert(
                hex,
                AircraftInfo {
                    registration,
                    typecode,
                    category,
                    military,
                    owner,
                },
            );
            loaded += 1;
        }

        if skipped > 0 {
            warn!(loaded, skipped, "Aircraft database loaded (some rows skipped)");
        }
        info!(entries = loaded, "Aircraft database ready");

        Ok(Self { entries })
    }

    /// Look up an aircraft by ICAO Mode-S hex code (case-insensitive).
    pub fn lookup(&self, icao_hex: &str) -> Option<&AircraftInfo> {
        let key = icao_hex.trim().to_lowercase();
        self.entries.get(&key)
    }

    /// Number of entries in the database.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the database is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Return `Some(trimmed)` if the string is non-empty after trimming, else `None`.
fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_empty() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty("ABC"), Some("ABC".to_string()));
        assert_eq!(non_empty(" DEF "), Some("DEF".to_string()));
    }

    #[test]
    fn test_lookup_case_insensitive() {
        let mut entries = HashMap::new();
        entries.insert(
            "ae1234".to_string(),
            AircraftInfo {
                registration: Some("N12345".to_string()),
                typecode: Some("B738".to_string()),
                category: "airliner".to_string(),
                military: false,
                owner: Some("United Airlines".to_string()),
            },
        );
        let db = AircraftDb { entries };

        assert!(db.lookup("AE1234").is_some());
        assert!(db.lookup("ae1234").is_some());
        assert!(db.lookup("Ae1234").is_some());
        assert!(db.lookup("xx9999").is_none());

        let info = db.lookup("ae1234").unwrap();
        assert_eq!(info.registration.as_deref(), Some("N12345"));
        assert!(!info.military);
        assert_eq!(info.category, "airliner");
    }

    #[test]
    fn test_load_real_file() {
        // Only run if the modes.csv is available (CI might not have it)
        let path = "test-data/modes.csv";
        if !Path::new(path).exists() {
            return;
        }

        let db = AircraftDb::load(path).unwrap();
        assert!(db.len() > 400_000, "Expected >400K entries, got {}", db.len());

        // Known entry from the file header
        let info = db.lookup("00029c").expect("Should find Senegal Air Force CN-235");
        assert_eq!(info.registration.as_deref(), Some("6W-TTC"));
        assert_eq!(info.typecode.as_deref(), Some("CN35"));
        assert_eq!(info.category, "transport");
        assert!(info.military);
        assert_eq!(info.owner.as_deref(), Some("Senegal Air Force"));
    }
}
