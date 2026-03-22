use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, warn};

use sr_types::{EventType, Severity, SourceType};

use std::future::Future;
use std::pin::Pin;

use crate::common::region_for_country;
use crate::{DataSource, InsertableEvent, SourceContext};

/// Countries monitored for internet outages via IODA.
const COUNTRIES: &[&str] = &["IR", "IL", "UA", "RU", "YE", "SD"];

/// IODA internet outage detection source.
///
/// Uses the `/v2/outages/alerts` endpoint which provides pre-computed anomaly
/// alerts with severity levels, rather than raw time-series signals.
pub struct IodaSource {
    /// Rotating index into the COUNTRIES list.
    country_index: Mutex<usize>,
}

/// Top-level response envelope from the IODA outages/alerts API.
///
/// Example:
/// ```json
/// {
///   "type": "outages.alerts",
///   "data": [
///     {
///       "datasource": "bgp",
///       "entity": { "code": "IR", "name": "Iran ...", "type": "country" },
///       "time": 1772409300,
///       "level": "critical",
///       "condition": "< 0.99",
///       "value": 40704,
///       "historyValue": 41355,
///       "method": "median"
///     }
///   ]
/// }
/// ```
#[derive(Debug, Deserialize)]
struct IodaAlertsResponse {
    #[serde(default)]
    data: Vec<IodaAlert>,
}

#[derive(Debug, Deserialize)]
struct IodaAlert {
    /// Signal source (e.g. "bgp", "ping-slash24", "merit-nt").
    #[serde(default)]
    datasource: String,

    /// Entity metadata (country code, name, type).
    #[serde(default)]
    entity: IodaEntity,

    /// Unix timestamp of the alert.
    #[serde(default)]
    time: i64,

    /// Alert level: "normal", "warning", "critical".
    #[serde(default)]
    level: String,

    /// Condition description (e.g. "< 0.99", "normal").
    #[serde(default)]
    condition: String,

    /// Current measured value.
    #[serde(default)]
    value: Option<f64>,

    /// Historical baseline value.
    #[serde(default, rename = "historyValue")]
    history_value: Option<f64>,

    /// Detection method (e.g. "median").
    #[serde(default)]
    method: String,
}

#[derive(Debug, Deserialize, Default)]
struct IodaEntity {
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    /// Entity type (e.g. "country", "asn"). Kept for deserialization compatibility.
    #[serde(default, rename = "type")]
    #[allow(dead_code)]
    entity_type: String,
}

impl Default for IodaSource {
    fn default() -> Self {
        Self::new()
    }
}

impl IodaSource {
    pub fn new() -> Self {
        Self {
            country_index: Mutex::new(0),
        }
    }

    /// Advance the country index and return the current country code.
    fn next_country(&self) -> &'static str {
        let mut idx = self.country_index.lock().unwrap_or_else(|e| e.into_inner());
        let cc = COUNTRIES[*idx % COUNTRIES.len()];
        *idx = (*idx + 1) % COUNTRIES.len();
        cc
    }

    /// Convert an IODA alert into an InsertableEvent if it represents an anomaly.
    fn alert_to_event(alert: &IodaAlert, cc: &str) -> Option<InsertableEvent> {
        // Skip normal-level alerts — they are not outages.
        if alert.level == "normal" {
            return None;
        }

        let value = alert.value?;
        let history_value = match alert.history_value {
            Some(hv) if hv > 0.0 => hv,
            _ => return None,
        };

        let ratio = value / history_value;
        let severity = match alert.level.as_str() {
            "critical" => Severity::High,
            _ => Severity::Medium,
        };

        let region = region_for_country(cc);
        let entity_code = if alert.entity.code.is_empty() {
            cc.to_string()
        } else {
            alert.entity.code.clone()
        };

        let title = format!(
            "Internet outage: {} ({}) ratio={:.2} [{}]",
            cc, alert.datasource, ratio, alert.level
        );
        let source_id = format!(
            "ioda:{}:{}:{}",
            entity_code, alert.datasource, alert.time
        );

        let data = serde_json::json!({
            "country": cc,
            "signal_type": alert.datasource,
            "value": value,
            "history_value": history_value,
            "ratio": ratio,
            "level": alert.level,
            "condition": alert.condition,
            "method": alert.method,
            "entity_code": entity_code,
            "entity_name": alert.entity.name,
            "time": alert.time,
        });

        Some(InsertableEvent {
            event_time: Utc::now(),
            source_type: SourceType::Ioda,
            source_id: Some(source_id),
            longitude: None,
            latitude: None,
            region_code: region.map(String::from),
            entity_id: Some(entity_code),
            entity_name: None,
            event_type: EventType::InternetOutage,
            severity,
            confidence: None,
            tags: vec![],
            title: Some(title),
            description: None,
            payload: data,
            heading: None,
            speed: None,
            altitude: None,
        })
    }
}

impl DataSource for IodaSource {
    fn id(&self) -> &str {
        "ioda"
    }

    fn name(&self) -> &str {
        "IODA Outages"
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(10 * 60) // 10 minutes
    }

    fn poll<'a>(&'a self, ctx: &'a SourceContext) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<InsertableEvent>>> + Send + 'a>> {
        Box::pin(async move {
        let cc = self.next_country();
        let now = Utc::now().timestamp();
        // Look back 20 minutes to capture recent alerts.
        let from = now - 20 * 60;

        let url = format!(
            "https://api.ioda.inetintel.cc.gatech.edu/v2/outages/alerts\
             ?from={}&until={}&entityType=country&entityCode={}",
            from, now, cc,
        );

        debug!(country = cc, "Polling IODA outage alerts");

        let resp = match ctx.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(country = cc, error = %e, "IODA request failed");
                return Ok(Vec::new());
            }
        };

        // Propagate 429 rate limits to the registry for proper backoff
        let resp = crate::rate_limit::check_rate_limit(resp, "ioda")?;

        let body: IodaAlertsResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(country = cc, error = %e, "Failed to parse IODA response");
                return Ok(Vec::new());
            }
        };

        let events: Vec<InsertableEvent> = body
            .data
            .iter()
            .filter_map(|alert| Self::alert_to_event(alert, cc))
            .collect();

        debug!(count = events.len(), country = cc, "IODA poll complete");
        Ok(events)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_country_rotation() {
        let source = IodaSource::new();
        assert_eq!(source.next_country(), "IR");
        assert_eq!(source.next_country(), "IL");
        assert_eq!(source.next_country(), "UA");
        assert_eq!(source.next_country(), "RU");
        assert_eq!(source.next_country(), "YE");
        assert_eq!(source.next_country(), "SD");
        // Wraps around
        assert_eq!(source.next_country(), "IR");
    }

    #[test]
    fn test_alerts_response_parsing() {
        let json_data = r#"{
            "type": "outages.alerts",
            "metadata": {
                "requestTime": "2026-03-02T01:00:00+00:00",
                "responseTime": "2026-03-02T01:00:01+00:00"
            },
            "error": null,
            "data": [
                {
                    "datasource": "bgp",
                    "entity": {
                        "code": "IR",
                        "name": "Iran (Islamic Republic Of)",
                        "subnames": [],
                        "type": "country",
                        "attrs": { "fqid": "geo.netacuity.AS.IR" }
                    },
                    "time": 1772409300,
                    "level": "critical",
                    "condition": "< 0.99",
                    "value": 40704.0,
                    "historyValue": 41355.0,
                    "method": "median"
                },
                {
                    "datasource": "bgp",
                    "entity": {
                        "code": "IR",
                        "name": "Iran (Islamic Republic Of)",
                        "subnames": [],
                        "type": "country",
                        "attrs": { "fqid": "geo.netacuity.AS.IR" }
                    },
                    "time": 1772410200,
                    "level": "normal",
                    "condition": "normal",
                    "value": 40953.0,
                    "historyValue": 41355.0,
                    "method": "median"
                }
            ]
        }"#;

        let resp: IodaAlertsResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(resp.data.len(), 2);

        // First alert: critical
        let alert = &resp.data[0];
        assert_eq!(alert.datasource, "bgp");
        assert_eq!(alert.entity.code, "IR");
        assert_eq!(alert.level, "critical");
        assert_eq!(alert.value, Some(40704.0));
        assert_eq!(alert.history_value, Some(41355.0));
        assert_eq!(alert.time, 1772409300);

        // Second alert: normal
        assert_eq!(resp.data[1].level, "normal");
    }

    #[test]
    fn test_alert_to_event_critical() {
        let alert = IodaAlert {
            datasource: "bgp".to_string(),
            entity: IodaEntity {
                code: "IR".to_string(),
                name: "Iran (Islamic Republic Of)".to_string(),
                entity_type: "country".to_string(),
            },
            time: 1772409300,
            level: "critical".to_string(),
            condition: "< 0.99".to_string(),
            value: Some(40704.0),
            history_value: Some(41355.0),
            method: "median".to_string(),
        };

        let event = IodaSource::alert_to_event(&alert, "IR").expect("should produce event");
        assert_eq!(event.source_type, SourceType::Ioda);
        assert_eq!(event.event_type, EventType::InternetOutage);
        assert_eq!(event.severity, Severity::High); // critical -> high
        assert!(event.title.unwrap().contains("ratio=0.98"));
        assert_eq!(event.entity_id, Some("IR".to_string()));
        assert_eq!(event.region_code, Some("middle-east".to_string()));
    }

    #[test]
    fn test_alert_to_event_normal_skipped() {
        let alert = IodaAlert {
            datasource: "bgp".to_string(),
            entity: IodaEntity {
                code: "IR".to_string(),
                name: "Iran".to_string(),
                entity_type: "country".to_string(),
            },
            time: 1772410200,
            level: "normal".to_string(),
            condition: "normal".to_string(),
            value: Some(40953.0),
            history_value: Some(41355.0),
            method: "median".to_string(),
        };

        assert!(IodaSource::alert_to_event(&alert, "IR").is_none());
    }

    #[test]
    fn test_alert_to_event_warning() {
        let alert = IodaAlert {
            datasource: "ping-slash24".to_string(),
            entity: IodaEntity {
                code: "UA".to_string(),
                name: "Ukraine".to_string(),
                entity_type: "country".to_string(),
            },
            time: 1772400000,
            level: "warning".to_string(),
            condition: "< 0.95".to_string(),
            value: Some(350.0),
            history_value: Some(400.0),
            method: "median".to_string(),
        };

        let event = IodaSource::alert_to_event(&alert, "UA").expect("should produce event");
        assert_eq!(event.severity, Severity::Medium); // warning -> medium
        assert_eq!(event.region_code, Some("eastern-europe".to_string()));
    }

    #[test]
    fn test_alert_to_event_missing_values() {
        // No value
        let alert = IodaAlert {
            datasource: "bgp".to_string(),
            entity: IodaEntity::default(),
            time: 0,
            level: "critical".to_string(),
            condition: String::new(),
            value: None,
            history_value: Some(100.0),
            method: String::new(),
        };
        assert!(IodaSource::alert_to_event(&alert, "IR").is_none());

        // No history value
        let alert2 = IodaAlert {
            datasource: "bgp".to_string(),
            entity: IodaEntity::default(),
            time: 0,
            level: "critical".to_string(),
            condition: String::new(),
            value: Some(100.0),
            history_value: None,
            method: String::new(),
        };
        assert!(IodaSource::alert_to_event(&alert2, "IR").is_none());

        // Zero history value
        let alert3 = IodaAlert {
            datasource: "bgp".to_string(),
            entity: IodaEntity::default(),
            time: 0,
            level: "critical".to_string(),
            condition: String::new(),
            value: Some(100.0),
            history_value: Some(0.0),
            method: String::new(),
        };
        assert!(IodaSource::alert_to_event(&alert3, "IR").is_none());
    }

    #[test]
    fn test_empty_response() {
        let json_data = r#"{
            "type": "outages.alerts",
            "error": null,
            "data": []
        }"#;

        let resp: IodaAlertsResponse = serde_json::from_str(json_data).unwrap();
        assert!(resp.data.is_empty());
    }

    #[test]
    fn test_response_with_extra_fields() {
        // The API returns fields like "metadata", "perf", "copyright" that we don't model.
        // Ensure serde ignores them.
        let json_data = r#"{
            "type": "outages.alerts",
            "metadata": { "requestTime": "2026-03-02T00:00:00+00:00" },
            "requestParameters": { "from": "123" },
            "error": null,
            "perf": null,
            "data": [],
            "copyright": "Copyright Georgia Tech"
        }"#;

        let resp: IodaAlertsResponse = serde_json::from_str(json_data).unwrap();
        assert!(resp.data.is_empty());
    }
}
