use std::convert::Infallible;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use sr_pipeline::PublishEvent;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::state::AppState;

/// SSE handler — subscribes to the pipeline's publish channel.
/// Emits three SSE event types: `event`, `incident`, `summary`.
pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.publish_tx.subscribe();
    let counter = state.sse_event_counter.clone();

    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        match result {
            Ok(publish_event) => {
                let id = counter.fetch_add(1, Ordering::Relaxed);
                let (sse_event_type, json) = match &publish_event {
                    PublishEvent::Event { event } => {
                        let event_type = event.event_type.to_string();
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => (event_type, json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::Incident(incident) => {
                        let sse_type = format!("incident:{}", incident.rule_id);
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => (sse_type, json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::Summary(summary) => {
                        let sse_type = format!("summary:{}", summary.event_type);
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => (sse_type, json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::Analysis(_) => {
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => ("analysis".to_string(), json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::Situations { .. } => {
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => ("situations".to_string(), json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::Alert(alert) => {
                        let sse_type = format!("alert:{}", alert.severity);
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => (sse_type, json),
                            Err(_) => return None,
                        }
                    }
                    PublishEvent::SourceHealthChange { source_id, .. } => {
                        let sse_type = format!("source_health:{}", source_id);
                        match serde_json::to_string(&publish_event) {
                            Ok(json) => (sse_type, json),
                            Err(_) => return None,
                        }
                    }
                };

                Some(Ok(
                    Event::default()
                        .data(json)
                        .event(sse_event_type)
                        .id(id.to_string()),
                ))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
