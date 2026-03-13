use super::model::{Certainty, EntityStateChange, StateChangeMention, StateChangeType};
use uuid::Uuid;

/// Detects entity state changes from enrichment data.
pub struct StateDetector;

impl StateDetector {
    /// Detect state changes from enrichment-extracted mentions.
    pub fn detect_from_mentions(
        mentions: &[StateChangeMention],
        entity_id_lookup: &dyn Fn(&str) -> Option<Uuid>,
    ) -> Vec<EntityStateChange> {
        let mut changes = Vec::new();

        for mention in mentions {
            let entity_id = match entity_id_lookup(&mention.entity) {
                Some(id) => id,
                None => continue,
            };

            let change_type = match StateChangeType::from_str(&mention.to) {
                Some(ct) => ct,
                None => continue,
            };

            let certainty = mention
                .certainty
                .as_deref()
                .map(Certainty::from_str)
                .unwrap_or(Certainty::Alleged);

            let mut change = EntityStateChange::new(entity_id, change_type, certainty);
            if let Some(ref from) = mention.from {
                change.previous_state = Some(serde_json::json!({"status": from}));
            }

            changes.push(change);
        }

        changes
    }

    /// Detect state changes from raw text by scanning for trigger keywords.
    /// Returns (entity_name_fragment, change_type) pairs.
    pub fn detect_from_text(text: &str) -> Vec<(String, StateChangeType)> {
        let lower = text.to_lowercase();
        let mut results = Vec::new();

        for &(keyword, ref change_type) in StateChangeType::trigger_keywords() {
            if let Some(pos) = lower.find(keyword) {
                // Try to extract nearby entity name (crude: take preceding words)
                let before = &text[..pos].trim_end();
                let words: Vec<&str> = before.split_whitespace().collect();
                if !words.is_empty() {
                    // Take last 1-3 words as entity name fragment
                    let start = words.len().saturating_sub(3);
                    let entity = words[start..].join(" ");
                    if entity.len() >= 3 {
                        results.push((entity, change_type.clone()));
                    }
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_from_text() {
        let results = StateDetector::detect_from_text("Nasrallah killed in airstrike");
        assert!(!results.is_empty());
        assert!(matches!(results[0].1, StateChangeType::Killed));
    }

    #[test]
    fn test_detect_from_mentions() {
        let mentions = vec![StateChangeMention {
            entity: "Hassan Nasrallah".to_string(),
            attribute: "status".to_string(),
            from: Some("alive".to_string()),
            to: "killed".to_string(),
            certainty: Some("confirmed".to_string()),
        }];

        let id = Uuid::new_v4();
        let lookup = |name: &str| -> Option<Uuid> {
            if name == "Hassan Nasrallah" {
                Some(id)
            } else {
                None
            }
        };

        let changes = StateDetector::detect_from_mentions(&mentions, &lookup);
        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0].change_type, StateChangeType::Killed));
        assert!(matches!(changes[0].certainty, Certainty::Confirmed));
    }
}
