use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use regex::Regex;
use serde_json::json;
use tracing::{debug, info};

use sr_types::{EventType, Severity, SourceType};

use crate::common::region_from_coords;
use crate::{DataSource, InsertableEvent, SourceContext};

/// MSCIO mirror of UKMTO warnings — lists maritime security warning PDFs.
///
/// The UKMTO (UK Maritime Trade Operations) publishes structured maritime
/// security warnings covering attacks, suspicious approaches, hijackings,
/// and other threats to commercial shipping.
///
/// The primary ukmto.org site returns HTTP 403 for automated clients.
/// MSCIO (Maritime Security Centre — Indian Ocean) mirrors all UKMTO
/// warnings as downloadable PDFs at a publicly accessible folder listing.
const FOLDER_URL: &str = "https://mscio.eu/folder/documents/UKMTO%20Warnings/";

/// Base URL for constructing full PDF download links from relative hrefs.
const MSCIO_BASE: &str = "https://mscio.eu";

/// Warning types extracted from UKMTO reference lines.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum WarningType {
    Attack,
    SuspiciousApproach,
    Hijack,
    Boarding,
    Firing,
    Warning,
    Information,
    Update,
    Other(String),
}

impl WarningType {
    #[allow(dead_code)]
    fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "ATTACK" => Self::Attack,
            "SUSPICIOUS APPROACH" | "SUSPICIOUS_APPROACH" => Self::SuspiciousApproach,
            "HIJACK" | "HIJACKING" => Self::Hijack,
            "BOARDING" | "ATTEMPTED BOARDING" => Self::Boarding,
            "FIRING" | "FIRED UPON" => Self::Firing,
            "WARNING" => Self::Warning,
            "INFORMATION" | "INFO" => Self::Information,
            "UPDATE" => Self::Update,
            other => Self::Other(other.to_string()),
        }
    }

    fn severity(&self) -> Severity {
        match self {
            Self::Hijack => Severity::Critical,
            Self::Attack | Self::Firing => Severity::High,
            Self::Boarding | Self::SuspiciousApproach => Severity::Medium,
            Self::Warning | Self::Update => Severity::Medium,
            Self::Information => Severity::Low,
            Self::Other(_) => Severity::Low,
        }
    }

    fn as_tag(&self) -> &str {
        match self {
            Self::Attack => "attack",
            Self::SuspiciousApproach => "suspicious-approach",
            Self::Hijack => "hijack",
            Self::Boarding => "boarding",
            Self::Firing => "firing",
            Self::Warning => "warning",
            Self::Information => "information",
            Self::Update => "update",
            Self::Other(_) => "other",
        }
    }
}

/// A parsed UKMTO warning entry.
#[derive(Debug, Clone)]
struct UkmtoWarning {
    /// Reference number, e.g. "019-26"
    reference: String,
    /// Warning type, e.g. ATTACK, SUSPICIOUS APPROACH
    warning_type: WarningType,
    /// Update number if this is an update (e.g. "UPDATE 001")
    update_number: Option<String>,
    /// Date of the report
    report_date: Option<NaiveDate>,
    /// Time of the report (UTC)
    report_time: Option<String>,
    /// Body text / description
    body: String,
    /// Extracted latitude (if found in body text)
    latitude: Option<f64>,
    /// Extracted longitude (if found in body text)
    longitude: Option<f64>,
    /// Location description from body text
    location_text: Option<String>,
    /// URL to the source PDF
    pdf_url: Option<String>,
}

/// Metadata extracted from an MSCIO PDF filename.
#[derive(Debug, Clone)]
struct FilenameMetadata {
    /// The PDF filename (e.g. "20260311-UKMTO_WARNING_020-26.pdf")
    #[allow(dead_code)]
    filename: String,
    /// Full download URL
    pdf_url: String,
    /// Warning reference (e.g. "020-26" or "039")
    reference: String,
    /// Date extracted from filename prefix (YYYYMMDD)
    date: Option<NaiveDate>,
    /// Update number if present (e.g. "001")
    update_number: Option<String>,
    /// Source ID for dedup (constructed from filename without extension)
    source_id: String,
}

/// Parse a UKMTO reference line like "019-26 – ATTACK – UPDATE 002".
///
/// Returns (reference, warning_type, update_number).
#[allow(dead_code)]
fn parse_reference_line(line: &str) -> Option<(String, WarningType, Option<String>)> {
    // Pattern: NNN-YY followed by separator and type
    let re = Regex::new(
        r"(?i)(\d{3}-\d{2})\s*[–\-]\s*(ATTACK|SUSPICIOUS\s*APPROACH|HIJACK(?:ING)?|BOARDING|ATTEMPTED\s*BOARDING|FIRING|FIRED\s*UPON|WARNING|INFORMATION|INFO|UPDATE)(?:\s*[–\-]\s*(UPDATE\s*\d+))?"
    ).ok()?;

    let caps = re.captures(line)?;
    let reference = caps.get(1)?.as_str().to_string();
    let wtype = WarningType::from_str(caps.get(2)?.as_str());
    let update = caps.get(3).map(|m| m.as_str().to_string());

    Some((reference, wtype, update))
}

/// Extract decimal degree coordinates from text.
///
/// Looks for patterns like:
/// - "12.3456N 045.6789E"
/// - "12° 34.5' N, 045° 67.8' E"
/// - "Lat: 12.345 Lon: 45.678"
#[allow(dead_code)]
fn extract_coordinates(text: &str) -> Option<(f64, f64)> {
    // Pattern 1: Decimal degrees with N/S/E/W suffixes
    // e.g. "12.3456N 045.6789E" or "12.3456 N, 45.6789 E"
    let re_decimal = Regex::new(
        r"(\d{1,3}(?:\.\d+)?)\s*°?\s*([NS])\s*[,/]?\s*(\d{1,3}(?:\.\d+)?)\s*°?\s*([EW])"
    ).ok()?;
    if let Some(caps) = re_decimal.captures(text) {
        let mut lat: f64 = caps.get(1)?.as_str().parse().ok()?;
        let mut lon: f64 = caps.get(3)?.as_str().parse().ok()?;
        if caps.get(2)?.as_str() == "S" {
            lat = -lat;
        }
        if caps.get(4)?.as_str() == "W" {
            lon = -lon;
        }
        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            return Some((lat, lon));
        }
    }

    // Pattern 2: Degrees and decimal minutes — "12° 34.567' N 045° 12.345' E"
    let re_dm = Regex::new(
        r"(\d{1,3})°\s*(\d{1,2}(?:\.\d+)?)[''′]\s*([NS])\s*[,/]?\s*(\d{1,3})°\s*(\d{1,2}(?:\.\d+)?)[''′]\s*([EW])"
    ).ok()?;
    if let Some(caps) = re_dm.captures(text) {
        let lat_deg: f64 = caps.get(1)?.as_str().parse().ok()?;
        let lat_min: f64 = caps.get(2)?.as_str().parse().ok()?;
        let mut lat = lat_deg + lat_min / 60.0;
        let lon_deg: f64 = caps.get(4)?.as_str().parse().ok()?;
        let lon_min: f64 = caps.get(5)?.as_str().parse().ok()?;
        let mut lon = lon_deg + lon_min / 60.0;
        if caps.get(3)?.as_str() == "S" {
            lat = -lat;
        }
        if caps.get(6)?.as_str() == "W" {
            lon = -lon;
        }
        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            return Some((lat, lon));
        }
    }

    // Pattern 3: "Lat: 12.345 Lon: 45.678" or "latitude 12.345 longitude 45.678"
    let re_latlon = Regex::new(
        r"(?i)lat(?:itude)?[:\s]+(-?\d{1,3}(?:\.\d+)?)\s*[,/]?\s*lon(?:gitude)?[:\s]+(-?\d{1,3}(?:\.\d+)?)"
    ).ok()?;
    if let Some(caps) = re_latlon.captures(text) {
        let lat: f64 = caps.get(1)?.as_str().parse().ok()?;
        let lon: f64 = caps.get(2)?.as_str().parse().ok()?;
        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            return Some((lat, lon));
        }
    }

    None
}

/// Extract a location description from the body text.
///
/// Looks for common UKMTO patterns like "in the Straits of Hormuz",
/// "Red Sea", "Gulf of Aden", "Gulf of Oman", "Arabian Sea", etc.
#[allow(dead_code)]
fn extract_location_text(text: &str) -> Option<String> {
    let text_lower = text.to_lowercase();

    // Try to find "NNnm [direction] of [place]" patterns
    let re_distance = Regex::new(
        r"(?i)(\d+\s*(?:nm|nautical miles?)\s+(?:north|south|east|west|NE|NW|SE|SW|north-?east|north-?west|south-?east|south-?west)\s+of\s+[A-Za-z\s,]+)"
    ).ok()?;
    if let Some(caps) = re_distance.captures(text) {
        return Some(caps.get(1)?.as_str().trim().to_string());
    }

    // Check for well-known maritime areas
    let areas = [
        ("strait of hormuz", "Strait of Hormuz"),
        ("straits of hormuz", "Strait of Hormuz"),
        ("gulf of oman", "Gulf of Oman"),
        ("gulf of aden", "Gulf of Aden"),
        ("red sea", "Red Sea"),
        ("arabian sea", "Arabian Sea"),
        ("bab el-mandeb", "Bab el-Mandeb"),
        ("bab al-mandab", "Bab el-Mandeb"),
        ("gulf of guinea", "Gulf of Guinea"),
        ("malacca strait", "Malacca Strait"),
        ("singapore strait", "Singapore Strait"),
        ("indian ocean", "Indian Ocean"),
        ("persian gulf", "Persian Gulf"),
        ("arabian gulf", "Arabian Gulf"),
        ("mozambique channel", "Mozambique Channel"),
        ("somali basin", "Somali Basin"),
        ("south china sea", "South China Sea"),
    ];

    for (pattern, label) in &areas {
        if text_lower.contains(pattern) {
            return Some(label.to_string());
        }
    }

    None
}

/// Detect maritime-relevant tags from body text.
fn extract_tags_from_body(text: &str) -> Vec<String> {
    let text_lower = text.to_lowercase();
    let mut tags = Vec::new();

    if text_lower.contains("missile") || text_lower.contains("ballistic") {
        tags.push("missile".to_string());
    }
    if text_lower.contains("drone") || text_lower.contains("uav") || text_lower.contains("unmanned") {
        tags.push("drone".to_string());
    }
    if text_lower.contains("houthi") {
        tags.push("actor:houthi".to_string());
    }
    if text_lower.contains("piracy") || text_lower.contains("pirate") {
        tags.push("piracy".to_string());
    }
    if text_lower.contains("explosion") {
        tags.push("explosion".to_string());
    }
    if text_lower.contains("torpedo") {
        tags.push("torpedo".to_string());
    }
    if text_lower.contains("red sea") {
        tags.push("red-sea".to_string());
    }
    if text_lower.contains("gulf of aden") {
        tags.push("gulf-of-aden".to_string());
    }
    if text_lower.contains("strait") {
        tags.push("strait".to_string());
    }
    if text_lower.contains("arabian sea") {
        tags.push("arabian-sea".to_string());
    }

    tags
}

/// Parse a date string in common UKMTO formats.
/// Used by `parse_warning_text` for PDF-extracted content.
#[allow(dead_code)]
fn parse_report_date(s: &str) -> Option<NaiveDate> {
    // Try "DD MMM YYYY" (e.g. "11 Mar 2026")
    if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%d %b %Y") {
        return Some(d);
    }
    // Try "DD/MM/YYYY"
    if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%d/%m/%Y") {
        return Some(d);
    }
    // Try "YYYY-MM-DD"
    if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
        return Some(d);
    }
    None
}

/// Parse structured fields from a UKMTO warning text block.
///
/// Expected format (from PDF text extraction or page scraping):
/// ```text
/// Reference: 019-26 – ATTACK – UPDATE 002
/// Report Date: 11 Mar 2026
/// Report Time: 0845 UTC
/// Source: UKMTO
///
/// Body text describing the incident...
/// ```
#[allow(dead_code)]
fn parse_warning_text(text: &str) -> Option<UkmtoWarning> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut reference = String::new();
    let mut warning_type = WarningType::Warning;
    let mut update_number = None;
    let mut report_date = None;
    let mut report_time = None;
    let mut body_lines = Vec::new();
    let mut in_body = false;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !reference.is_empty() {
                in_body = true;
            }
            continue;
        }

        // Try to extract reference from a "Reference:" prefixed line
        if let Some(ref_val) = trimmed.strip_prefix("Reference:").or_else(|| trimmed.strip_prefix("Ref:")) {
            if let Some((r, wt, un)) = parse_reference_line(ref_val.trim()) {
                reference = r;
                warning_type = wt;
                update_number = un;
                continue;
            }
        }

        // Also try the line itself as a reference (some formats omit the prefix)
        if reference.is_empty() {
            if let Some((r, wt, un)) = parse_reference_line(trimmed) {
                reference = r;
                warning_type = wt;
                update_number = un;
                continue;
            }
        }

        if let Some(date_val) = trimmed.strip_prefix("Report Date:").or_else(|| trimmed.strip_prefix("Date:")) {
            report_date = parse_report_date(date_val.trim());
            continue;
        }

        if let Some(time_val) = trimmed.strip_prefix("Report Time:").or_else(|| trimmed.strip_prefix("Time:")) {
            report_time = Some(time_val.trim().to_string());
            continue;
        }

        // Skip "Source:" and "Issue Date:" header lines
        if trimmed.starts_with("Source:") || trimmed.starts_with("Issue Date:") {
            continue;
        }

        // Anything else after we found the reference is body
        if !reference.is_empty() {
            in_body = true;
        }
        if in_body {
            body_lines.push(trimmed);
        }
    }

    if reference.is_empty() {
        return None;
    }

    let body = body_lines.join(" ").trim().to_string();
    let (latitude, longitude) = extract_coordinates(&body).unzip();
    let location_text = extract_location_text(&body);

    Some(UkmtoWarning {
        reference,
        warning_type,
        update_number,
        report_date,
        report_time,
        body,
        latitude,
        longitude,
        location_text,
        pdf_url: None,
    })
}

/// Parse the MSCIO folder listing HTML to extract PDF links.
///
/// The MSCIO document folder is a simple HTML page with links like:
///   `<a href="/media/documents/20260311-UKMTO_WARNING_020-26.pdf">...</a>`
///
/// Returns a list of `FilenameMetadata` for each PDF found.
fn parse_mscio_folder(html: &str) -> Vec<FilenameMetadata> {
    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();

    // Match any link to a UKMTO warning PDF in /media/documents/
    let re_link = Regex::new(
        r#"href="(/media/documents/((\d{8})-UKMTO_WARNING_([^"]+)\.pdf))""#
    ).unwrap();

    // Match update number from filename fragments like:
    //   UPDATE_001, Update_001, Update-001, -UPDATE_001, _-UPDATE_001, _-_UPDATE_001
    let re_update = Regex::new(
        r"(?i)[_\-]*UPDATE[_\-]?(\d{3})"
    ).unwrap();

    // Match reference number: NNN-YY or NNN_YY or just NNN
    let re_ref = Regex::new(
        r"^(\d{3})[_\-](\d{2})"
    ).unwrap();

    for caps in re_link.captures_iter(html) {
        let href = caps.get(1).unwrap().as_str();
        let filename = caps.get(2).unwrap().as_str();
        let date_str = caps.get(3).unwrap().as_str();
        let after_warning = caps.get(4).unwrap().as_str(); // e.g. "020-26", "016-26-UK-OS-UPDATE_001"

        let pdf_url = format!("{}{}", MSCIO_BASE, href);

        // Parse date from YYYYMMDD prefix
        let date = NaiveDate::parse_from_str(date_str, "%Y%m%d").ok();

        // Extract reference number (NNN-YY or NNN_YY or bare NNN)
        let reference = if let Some(ref_caps) = re_ref.captures(after_warning) {
            format!("{}-{}", &ref_caps[1], &ref_caps[2])
        } else {
            // Bare number like "039" without year suffix
            after_warning.split(|c: char| !c.is_ascii_digit()).next()
                .unwrap_or(after_warning)
                .to_string()
        };

        // Extract update number if present
        let update_number = re_update.captures(after_warning)
            .map(|c| c.get(1).unwrap().as_str().to_string());

        // Use the full filename (without .pdf) as the dedup key to distinguish
        // base warnings from their updates
        let source_id = format!("ukmto:{}", filename.trim_end_matches(".pdf"));

        // Skip exact duplicates (shouldn't happen but defensive)
        if !seen_ids.insert(source_id.clone()) {
            continue;
        }

        entries.push(FilenameMetadata {
            filename: format!("{}.pdf", filename.trim_end_matches(".pdf")),
            pdf_url,
            reference,
            date,
            update_number,
            source_id,
        });
    }

    entries
}

/// UKMTO Maritime Security Warnings source (via MSCIO mirror).
///
/// Polls the MSCIO document folder for new maritime security warning PDFs.
/// These are structured reports covering attacks on ships, suspicious
/// approaches, hijackings, and other threats to commercial shipping --
/// primarily in the Red Sea, Gulf of Aden, Strait of Hormuz, and Indian Ocean.
pub struct UkmtoWarningsSource {
    /// Source IDs we have already processed (dedup).
    seen: Mutex<HashSet<String>>,
}

impl UkmtoWarningsSource {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Build an InsertableEvent from a parsed UKMTO warning.
    fn warning_to_event(warning: &UkmtoWarning) -> InsertableEvent {
        let severity = warning.warning_type.severity();

        // Override severity for keywords in body text
        let severity = if warning.body.to_lowercase().contains("missile")
            || warning.body.to_lowercase().contains("ballistic")
            || warning.body.to_lowercase().contains("torpedo")
        {
            Severity::Critical
        } else if warning.body.to_lowercase().contains("explosion")
            || warning.body.to_lowercase().contains("fired upon")
            || warning.body.to_lowercase().contains("rpg")
        {
            Severity::High.max(severity)
        } else {
            severity
        };

        // Build event time from report date + time
        let event_time = warning.report_date
            .and_then(|d| {
                if let Some(ref time_str) = warning.report_time {
                    // Parse "0845 UTC" or "08:45 UTC"
                    let clean = time_str.replace("UTC", "").replace(':', "").trim().to_string();
                    if clean.len() >= 4 {
                        let hours: u32 = clean[..2].parse().ok()?;
                        let minutes: u32 = clean[2..4].parse().ok()?;
                        d.and_hms_opt(hours, minutes, 0).map(|dt| dt.and_utc())
                    } else {
                        d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc())
                    }
                } else {
                    d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc())
                }
            })
            .unwrap_or_else(Utc::now);

        // Build title
        let type_label = match &warning.warning_type {
            WarningType::Attack => "ATTACK",
            WarningType::SuspiciousApproach => "SUSPICIOUS APPROACH",
            WarningType::Hijack => "HIJACK",
            WarningType::Boarding => "BOARDING",
            WarningType::Firing => "FIRING",
            WarningType::Warning => "WARNING",
            WarningType::Information => "INFORMATION",
            WarningType::Update => "UPDATE",
            WarningType::Other(s) => s.as_str(),
        };

        let update_suffix = warning.update_number
            .as_ref()
            .map(|u| format!(" (UPDATE {})", u))
            .unwrap_or_default();

        let location_suffix = warning.location_text
            .as_ref()
            .map(|l| format!(" -- {}", l))
            .unwrap_or_default();

        let title = format!(
            "UKMTO {} {}{}{}",
            type_label,
            warning.reference,
            update_suffix,
            location_suffix,
        );

        // First sentence of body for description preview
        let description = if warning.body.len() > 500 {
            format!("{}...", &warning.body[..497])
        } else {
            warning.body.clone()
        };

        // Region from coordinates
        let region_code = warning.latitude
            .zip(warning.longitude)
            .and_then(|(lat, lon)| region_from_coords(lat, lon))
            .map(String::from);

        // Build tags
        let mut tags = vec![
            "maritime".to_string(),
            "maritime-security".to_string(),
            "ukmto".to_string(),
            warning.warning_type.as_tag().to_string(),
        ];
        tags.extend(extract_tags_from_body(&warning.body));
        tags.dedup();

        // Payload with full structured data
        let payload = json!({
            "reference": warning.reference,
            "warning_type": type_label,
            "update_number": warning.update_number,
            "report_date": warning.report_date.map(|d| d.to_string()),
            "report_time": warning.report_time,
            "body": warning.body,
            "location_text": warning.location_text,
            "pdf_url": warning.pdf_url,
            "source": "UKMTO via MSCIO",
        });

        InsertableEvent {
            event_time,
            source_type: SourceType::UkmtoWarnings,
            source_id: Some(format!("ukmto:{}", warning.reference)),
            longitude: warning.longitude,
            latitude: warning.latitude,
            region_code,
            entity_id: None,
            entity_name: None,
            event_type: EventType::MaritimeSecurity,
            severity,
            confidence: Some(0.95), // UKMTO data is authoritative
            tags,
            title: Some(title),
            description: Some(description),
            payload,
            heading: None,
            speed: None,
            altitude: None,
        }
    }

    /// Create a warning from MSCIO filename metadata (no PDF text available).
    fn warning_from_metadata(&self, meta: &FilenameMetadata) -> UkmtoWarning {
        // The filename itself is the only type hint we have without PDF text.
        // UKMTO warning filenames don't encode the incident type (ATTACK etc.),
        // so we default to Warning type.
        let warning_type = WarningType::Warning;

        UkmtoWarning {
            reference: meta.reference.clone(),
            warning_type,
            update_number: meta.update_number.clone(),
            report_date: meta.date,
            report_time: None,
            body: format!(
                "UKMTO Warning {} published {}. PDF: {}",
                meta.reference,
                meta.date.map(|d| d.to_string()).unwrap_or_else(|| "unknown date".to_string()),
                meta.pdf_url,
            ),
            latitude: None,
            longitude: None,
            location_text: None,
            pdf_url: Some(meta.pdf_url.clone()),
        }
    }
}

impl Default for UkmtoWarningsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for UkmtoWarningsSource {
    fn id(&self) -> &str {
        "ukmto-warnings"
    }

    fn name(&self) -> &str {
        "UKMTO Maritime Warnings"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(300) // 5 minutes
    }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        debug!("Polling MSCIO for UKMTO maritime security warnings");

        let resp = ctx.http
            .get(FOLDER_URL)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("MSCIO folder listing returned HTTP {}", status);
        }

        let html = resp.text().await?;
        let entries = parse_mscio_folder(&html);

        if entries.is_empty() {
            debug!("No warning PDFs found in MSCIO folder listing — page structure may have changed");
            return Ok(Vec::new());
        }

        let mut events = Vec::new();

        for meta in &entries {
            // Deduplication by source_id (full filename-based key)
            {
                let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&meta.source_id) {
                    continue;
                }
                seen.insert(meta.source_id.clone());
            }

            // Try to fetch the PDF and extract text via parse_warning_text.
            // The MSCIO PDFs are small (200-300 KB) but we cannot parse PDF
            // binary without a PDF crate. Instead, create event from filename
            // metadata and store the PDF URL in the payload for reference.
            let warning = self.warning_from_metadata(meta);
            let mut event = Self::warning_to_event(&warning);

            // Override the source_id to use the full filename-based key
            // so that base warnings and their updates are tracked separately.
            event.source_id = Some(meta.source_id.clone());

            events.push(event);
        }

        // Prune seen set if it grows too large
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 5_000 {
                debug!(old_size = seen.len(), "Pruning seen UKMTO warning refs");
                seen.clear();
            }
        }

        if !events.is_empty() {
            info!(count = events.len(), "UKMTO maritime security warnings via MSCIO");
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reference_attack() {
        let (reference, wtype, update) =
            parse_reference_line("019-26 – ATTACK – UPDATE 002").unwrap();
        assert_eq!(reference, "019-26");
        assert_eq!(wtype, WarningType::Attack);
        assert_eq!(update, Some("UPDATE 002".to_string()));
    }

    #[test]
    fn parse_reference_suspicious_approach() {
        let (reference, wtype, update) =
            parse_reference_line("005-26 – SUSPICIOUS APPROACH").unwrap();
        assert_eq!(reference, "005-26");
        assert_eq!(wtype, WarningType::SuspiciousApproach);
        assert!(update.is_none());
    }

    #[test]
    fn parse_reference_hijack() {
        let (reference, wtype, _) =
            parse_reference_line("012-25 - HIJACKING").unwrap();
        assert_eq!(reference, "012-25");
        assert_eq!(wtype, WarningType::Hijack);
    }

    #[test]
    fn parse_reference_warning() {
        let (reference, wtype, _) =
            parse_reference_line("001-26 – WARNING").unwrap();
        assert_eq!(reference, "001-26");
        assert_eq!(wtype, WarningType::Warning);
    }

    #[test]
    fn parse_reference_boarding() {
        let (reference, wtype, _) =
            parse_reference_line("033-25 – BOARDING").unwrap();
        assert_eq!(reference, "033-25");
        assert_eq!(wtype, WarningType::Boarding);
    }

    #[test]
    fn parse_reference_line_no_match() {
        assert!(parse_reference_line("Just some random text").is_none());
        assert!(parse_reference_line("").is_none());
    }

    #[test]
    fn extract_coords_decimal_degrees() {
        let (lat, lon) = extract_coordinates("Position: 12.345N 045.678E reported").unwrap();
        assert!((lat - 12.345).abs() < 0.001);
        assert!((lon - 45.678).abs() < 0.001);
    }

    #[test]
    fn extract_coords_decimal_degrees_south_west() {
        let (lat, lon) = extract_coordinates("Located at 34.567S 058.901W").unwrap();
        assert!((lat - -34.567).abs() < 0.001);
        assert!((lon - -58.901).abs() < 0.001);
    }

    #[test]
    fn extract_coords_degrees_minutes() {
        let (lat, lon) = extract_coordinates("12° 30.0' N 045° 45.0' E").unwrap();
        assert!((lat - 12.5).abs() < 0.01);
        assert!((lon - 45.75).abs() < 0.01);
    }

    #[test]
    fn extract_coords_lat_lon_prefix() {
        let (lat, lon) = extract_coordinates("Lat: 15.5 Lon: 42.3").unwrap();
        assert!((lat - 15.5).abs() < 0.01);
        assert!((lon - 42.3).abs() < 0.01);
    }

    #[test]
    fn extract_coords_none() {
        assert!(extract_coordinates("No coordinates here").is_none());
        assert!(extract_coordinates("").is_none());
    }

    #[test]
    fn extract_location_strait_of_hormuz() {
        // Distance pattern takes priority over area name (more specific)
        let loc = extract_location_text("11NM north of Oman in the Strait of Hormuz").unwrap();
        assert!(loc.contains("11NM north of Oman"));
    }

    #[test]
    fn extract_location_strait_of_hormuz_bare() {
        let loc = extract_location_text("Vessel transiting the Strait of Hormuz").unwrap();
        assert_eq!(loc, "Strait of Hormuz");
    }

    #[test]
    fn extract_location_red_sea() {
        let loc = extract_location_text("Vessel attacked in the Red Sea area").unwrap();
        assert_eq!(loc, "Red Sea");
    }

    #[test]
    fn extract_location_gulf_of_aden() {
        let loc = extract_location_text("Report from the Gulf of Aden region").unwrap();
        assert_eq!(loc, "Gulf of Aden");
    }

    #[test]
    fn extract_location_distance_pattern() {
        let loc = extract_location_text("The vessel was 50nm south of Nishtun, Yemen").unwrap();
        assert!(loc.contains("50nm south"));
    }

    #[test]
    fn extract_location_none() {
        assert!(extract_location_text("An incident occurred").is_none());
    }

    #[test]
    fn extract_tags_missile() {
        let tags = extract_tags_from_body("A missile struck the vessel near the bow");
        assert!(tags.contains(&"missile".to_string()));
    }

    #[test]
    fn extract_tags_houthi_red_sea() {
        let tags = extract_tags_from_body("Houthi forces attacked a vessel in the Red Sea");
        assert!(tags.contains(&"actor:houthi".to_string()));
        assert!(tags.contains(&"red-sea".to_string()));
    }

    #[test]
    fn extract_tags_drone() {
        let tags = extract_tags_from_body("UAV strike on merchant vessel");
        assert!(tags.contains(&"drone".to_string()));
    }

    #[test]
    fn parse_report_date_formats() {
        assert_eq!(
            parse_report_date("11 Mar 2026"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 11).unwrap())
        );
        assert_eq!(
            parse_report_date("25/01/2026"),
            Some(NaiveDate::from_ymd_opt(2026, 1, 25).unwrap())
        );
        assert_eq!(
            parse_report_date("2026-03-11"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 11).unwrap())
        );
        assert!(parse_report_date("not a date").is_none());
    }

    #[test]
    fn parse_warning_text_full() {
        let text = r#"Reference: 019-26 – ATTACK – UPDATE 002
Report Date: 11 Mar 2026
Report Time: 0845 UTC
Source: UKMTO

A merchant vessel was struck by a missile while transiting
the Red Sea approximately 50nm west of Al Hudaydah.
Position: 14.567N 042.890E. The vessel sustained damage
to the port side but remains underway."#;

        let warning = parse_warning_text(text).unwrap();
        assert_eq!(warning.reference, "019-26");
        assert_eq!(warning.warning_type, WarningType::Attack);
        assert_eq!(warning.update_number, Some("UPDATE 002".to_string()));
        assert_eq!(
            warning.report_date,
            Some(NaiveDate::from_ymd_opt(2026, 3, 11).unwrap())
        );
        assert_eq!(warning.report_time, Some("0845 UTC".to_string()));
        assert!(warning.body.contains("missile"));
        assert!(warning.latitude.is_some());
        assert!(warning.longitude.is_some());
        assert!((warning.latitude.unwrap() - 14.567).abs() < 0.01);
        assert!((warning.longitude.unwrap() - 42.890).abs() < 0.01);
        // Distance pattern takes priority (more specific than area name)
        assert!(warning.location_text.as_ref().unwrap().contains("50nm west"));
    }

    #[test]
    fn parse_warning_text_minimal() {
        let text = "005-26 – SUSPICIOUS APPROACH\n\nSmall boats approached a vessel.";
        let warning = parse_warning_text(text).unwrap();
        assert_eq!(warning.reference, "005-26");
        assert_eq!(warning.warning_type, WarningType::SuspiciousApproach);
        assert!(warning.body.contains("Small boats"));
    }

    #[test]
    fn parse_warning_text_no_reference() {
        let text = "This is just random text with no UKMTO reference.";
        assert!(parse_warning_text(text).is_none());
    }

    #[test]
    fn warning_severity_attack() {
        assert_eq!(WarningType::Attack.severity(), Severity::High);
    }

    #[test]
    fn warning_severity_hijack() {
        assert_eq!(WarningType::Hijack.severity(), Severity::Critical);
    }

    #[test]
    fn warning_severity_suspicious() {
        assert_eq!(WarningType::SuspiciousApproach.severity(), Severity::Medium);
    }

    #[test]
    fn warning_severity_info() {
        assert_eq!(WarningType::Information.severity(), Severity::Low);
    }

    #[test]
    fn warning_to_event_basic() {
        let warning = UkmtoWarning {
            reference: "019-26".to_string(),
            warning_type: WarningType::Attack,
            update_number: None,
            report_date: NaiveDate::from_ymd_opt(2026, 3, 11),
            report_time: Some("0845 UTC".to_string()),
            body: "Missile struck vessel in the Red Sea".to_string(),
            latitude: Some(14.5),
            longitude: Some(42.8),
            location_text: Some("Red Sea".to_string()),
            pdf_url: Some("https://mscio.eu/media/documents/20260311-UKMTO_WARNING_019-26.pdf".to_string()),
        };

        let event = UkmtoWarningsSource::warning_to_event(&warning);
        assert_eq!(event.source_type, SourceType::UkmtoWarnings);
        assert_eq!(event.event_type, EventType::MaritimeSecurity);
        assert_eq!(event.source_id, Some("ukmto:019-26".to_string()));
        // Missile keyword should escalate to Critical
        assert_eq!(event.severity, Severity::Critical);
        assert!(event.title.unwrap().contains("UKMTO ATTACK 019-26"));
        assert!(event.tags.contains(&"maritime".to_string()));
        assert!(event.tags.contains(&"ukmto".to_string()));
        assert!(event.tags.contains(&"missile".to_string()));
        assert!(event.tags.contains(&"red-sea".to_string()));
    }

    #[test]
    fn warning_to_event_with_update() {
        let warning = UkmtoWarning {
            reference: "019-26".to_string(),
            warning_type: WarningType::Attack,
            update_number: Some("002".to_string()),
            report_date: None,
            report_time: None,
            body: "Update to previous report".to_string(),
            latitude: None,
            longitude: None,
            location_text: None,
            pdf_url: None,
        };

        let event = UkmtoWarningsSource::warning_to_event(&warning);
        assert!(event.title.unwrap().contains("(UPDATE 002)"));
    }

    #[test]
    fn parse_mscio_folder_basic() {
        let html = r#"
        <div>
            <a href="/media/documents/20260311-UKMTO_WARNING_020-26.pdf">20260311-UKMTO_WARNING_020-26.pdf</a>
            <a href="/media/documents/20260310-UKMTO_WARNING_017-26.pdf">20260310-UKMTO_WARNING_017-26</a>
        </div>
        "#;

        let entries = parse_mscio_folder(html);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].reference, "020-26");
        assert_eq!(entries[0].date, NaiveDate::from_ymd_opt(2026, 3, 11));
        assert_eq!(entries[0].pdf_url, "https://mscio.eu/media/documents/20260311-UKMTO_WARNING_020-26.pdf");
        assert!(entries[0].update_number.is_none());
        assert_eq!(entries[1].reference, "017-26");
    }

    #[test]
    fn parse_mscio_folder_with_update() {
        let html = r#"
        <a href="/media/documents/20260304-UKMTO_WARNING_014-26-UPDATE_001.pdf">20260304-UKMTO_WARNING_014-26-UPDATE_001.pdf</a>
        <a href="/media/documents/20260304-UKMTO_WARNING_012-26_Update_001.pdf">text</a>
        <a href="/media/documents/20260302-UKMTO_WARNING_005-26_-UPDATE_001.pdf">text</a>
        "#;

        let entries = parse_mscio_folder(html);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].reference, "014-26");
        assert_eq!(entries[0].update_number, Some("001".to_string()));
        assert_eq!(entries[1].reference, "012-26");
        assert_eq!(entries[1].update_number, Some("001".to_string()));
        assert_eq!(entries[2].reference, "005-26");
        assert_eq!(entries[2].update_number, Some("001".to_string()));
    }

    #[test]
    fn parse_mscio_folder_uk_os_suffix() {
        let html = r#"
        <a href="/media/documents/20260307-UKMTO_WARNING_016-26-UK-OS-UPDATE_001.pdf">text</a>
        <a href="/media/documents/20260307-UKMTO_WARNING_016-26-UK-OS.pdf">text</a>
        "#;

        let entries = parse_mscio_folder(html);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].reference, "016-26");
        assert_eq!(entries[0].update_number, Some("001".to_string()));
        assert_eq!(entries[1].reference, "016-26");
        assert!(entries[1].update_number.is_none());
        // Both should have distinct source_ids
        assert_ne!(entries[0].source_id, entries[1].source_id);
    }

    #[test]
    fn parse_mscio_folder_underscore_ref() {
        let html = r#"
        <a href="/media/documents/20251205-UKMTO_WARNING_043_25.pdf">text</a>
        "#;

        let entries = parse_mscio_folder(html);
        assert_eq!(entries.len(), 1);
        // Underscore-separated ref should be normalized to dash
        assert_eq!(entries[0].reference, "043-25");
    }

    #[test]
    fn parse_mscio_folder_bare_number() {
        let html = r#"
        <a href="/media/documents/20251103-UKMTO_WARNING_039.pdf">text</a>
        "#;

        let entries = parse_mscio_folder(html);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].reference, "039");
    }

    #[test]
    fn parse_mscio_folder_no_matches() {
        let html = "<html><body>Nothing here</body></html>";
        let entries = parse_mscio_folder(html);
        assert!(entries.is_empty());
    }

    #[test]
    fn deduplication() {
        let source = UkmtoWarningsSource::new();
        {
            let mut seen = source.seen.lock().unwrap();
            seen.insert("ukmto:20260311-UKMTO_WARNING_019-26".to_string());
        }
        let seen = source.seen.lock().unwrap();
        assert!(seen.contains("ukmto:20260311-UKMTO_WARNING_019-26"));
        assert!(!seen.contains("ukmto:20260311-UKMTO_WARNING_020-26"));
    }

    #[test]
    fn source_metadata() {
        let source = UkmtoWarningsSource::new();
        assert_eq!(source.id(), "ukmto-warnings");
        assert_eq!(source.name(), "UKMTO Maritime Warnings");
        assert_eq!(source.default_interval(), Duration::from_secs(300));
        assert!(!source.is_streaming());
    }
}
