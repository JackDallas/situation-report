use sr_sources::InsertableEvent;

/// Build embedding input text from an event's available fields.
///
/// Returns `None` for high-volume types or events with no meaningful text.
/// Fields are concatenated with `" | "` separator in priority order:
/// 1. Translated title (from enrichment) or raw title
/// 2. Description (truncated to 500 chars)
/// 3. Entity name
/// 4. Non-actor/topic tags
/// 5. Enrichment summary
pub fn compose_text(event: &InsertableEvent) -> Option<String> {
    // Skip high-volume types
    if event.event_type.is_high_volume() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();

    // 1. Title — prefer translated title from enrichment
    let translated_title = event
        .payload
        .get("enrichment")
        .and_then(|e| e.get("translated_title"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(title) = translated_title.or_else(|| event.title.clone()) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    // 2. Description (truncated)
    if let Some(ref desc) = event.description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            let truncated = if trimmed.len() > 500 {
                // Find a safe UTF-8 char boundary at or before byte 500
                let end = (0..=500)
                    .rev()
                    .find(|&i| trimmed.is_char_boundary(i))
                    .unwrap_or(0);
                format!("{}...", &trimmed[..end])
            } else {
                trimmed.to_string()
            };
            parts.push(truncated);
        }
    }

    // 3. Entity name
    if let Some(ref name) = event.entity_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    // 4. Non-actor/topic tags (those are already captured by SituationGraph)
    let other_tags: Vec<&str> = event
        .tags
        .iter()
        .filter(|t| !t.starts_with("actor:") && !t.starts_with("topic:"))
        .map(|s| s.as_str())
        .collect();
    if !other_tags.is_empty() {
        parts.push(other_tags.join(", "));
    }

    // 5. Enrichment summary
    if let Some(summary) = event
        .payload
        .get("enrichment")
        .and_then(|e| e.get("summary"))
        .and_then(|v| v.as_str())
    {
        let trimmed = summary.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use sr_types::{EventType, Severity, SourceType};

    fn make_event(overrides: serde_json::Value) -> InsertableEvent {
        InsertableEvent {
            event_time: Utc::now(),
            source_type: overrides["source_type"]
                .as_str()
                .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok())
                .unwrap_or(SourceType::Gdelt),
            source_id: None,
            longitude: None,
            latitude: None,
            region_code: None,
            entity_id: None,
            entity_name: overrides["entity_name"].as_str().map(|s| s.to_string()),
            event_type: overrides["event_type"]
                .as_str()
                .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok())
                .unwrap_or(EventType::NewsArticle),
            severity: Severity::Medium,
            confidence: None,
            tags: overrides["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            title: overrides["title"].as_str().map(|s| s.to_string()),
            description: overrides["description"].as_str().map(|s| s.to_string()),
            payload: overrides
                .get("payload")
                .cloned()
                .unwrap_or_else(|| json!({})),
            heading: None,
            speed: None,
            altitude: None,
        }
    }

    #[test]
    fn test_compose_basic_news() {
        let event = make_event(json!({
            "title": "Explosion in Beirut port",
            "entity_name": "Hezbollah",
            "tags": ["topic:explosion", "lebanon", "conflict"],
        }));
        let text = compose_text(&event).unwrap();
        assert!(text.contains("Explosion in Beirut port"));
        assert!(text.contains("Hezbollah"));
        // topic: tags are filtered out, but "lebanon" and "conflict" remain
        assert!(text.contains("lebanon"));
        assert!(text.contains("conflict"));
        assert!(!text.contains("topic:explosion"));
    }

    #[test]
    fn test_skip_high_volume() {
        let event = make_event(json!({
            "event_type": "flight_position",
            "title": "Some flight",
        }));
        assert!(compose_text(&event).is_none());
    }

    #[test]
    fn test_skip_empty() {
        let event = make_event(json!({
            "event_type": "news_article",
        }));
        assert!(compose_text(&event).is_none());
    }

    #[test]
    fn test_prefers_translated_title() {
        let event = make_event(json!({
            "title": "Titulo original",
            "payload": {
                "enrichment": {
                    "translated_title": "Translated title in English"
                }
            }
        }));
        let text = compose_text(&event).unwrap();
        assert!(text.contains("Translated title in English"));
        assert!(!text.contains("Titulo original"));
    }

    #[test]
    fn test_description_truncation() {
        let long_desc = "a".repeat(600);
        let event = make_event(json!({
            "title": "Test",
            "description": long_desc,
        }));
        let text = compose_text(&event).unwrap();
        // Should be truncated to 500 + "..."
        assert!(text.len() < 520 + "Test | ".len());
    }

    #[test]
    fn test_enrichment_summary() {
        let event = make_event(json!({
            "title": "Article",
            "payload": {
                "enrichment": {
                    "summary": "Brief summary of the article content"
                }
            }
        }));
        let text = compose_text(&event).unwrap();
        assert!(text.contains("Brief summary"));
    }
}
