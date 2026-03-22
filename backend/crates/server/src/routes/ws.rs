use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::SinkExt;
use sr_pipeline::PublishEvent;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

// Use futures::StreamExt for WebSocket receiver (not tokio_stream)
use futures::StreamExt as FuturesStreamExt;

use crate::state::AppState;

/// WebSocket handler — subscribes to the pipeline's publish channel.
/// Sends JSON envelopes: `{"type": "<event_type>", "data": <payload>}`
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    let rx = state.publish_tx.subscribe();
    let mut stream = BroadcastStream::new(rx);

    // Ping interval for keepalive
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.tick().await; // consume first tick

    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming message from broadcast channel
            msg = tokio_stream::StreamExt::next(&mut stream) => {
                match msg {
                    Some(Ok(publish_event)) => {
                        let (event_type, json) = match format_event(&publish_event) {
                            Some(v) => v,
                            None => continue,
                        };
                        let envelope = match serde_json::to_string(&serde_json::json!({
                            "type": event_type,
                            "data": serde_json::from_str::<serde_json::Value>(&json).unwrap_or_default(),
                        })) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        if sender.send(Message::Text(envelope.into())).await.is_err() {
                            break; // client disconnected
                        }
                    }
                    Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n))) => {
                        tracing::warn!("WebSocket subscriber lagged {n} messages — skipping");
                    }
                    None => break, // channel closed
                }
            }

            // Incoming message from WebSocket client
            msg = FuturesStreamExt::next(&mut receiver) => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {} // ignore other client messages for now
                }
            }

            // Keepalive ping
            _ = ping_interval.tick() => {
                if sender.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        }
    }

    tracing::info!("WebSocket client disconnected");
}

/// Convert a PublishEvent into (event_type_string, json_payload).
/// Returns None if serialization fails.
fn format_event(publish_event: &PublishEvent) -> Option<(String, String)> {
    let event_type = match publish_event {
        PublishEvent::Event { event } => event.event_type.to_string(),
        PublishEvent::Incident(incident) => format!("incident:{}", incident.rule_id),
        PublishEvent::Summary(summary) => format!("summary:{}", summary.event_type),
        PublishEvent::Analysis(_) => "analysis".to_string(),
        PublishEvent::Situations { .. } => "situations".to_string(),
        PublishEvent::Alert(alert) => format!("alert:{}", alert.severity),
        PublishEvent::SourceHealthChange { source_id, .. } => {
            format!("source_health:{}", source_id)
        }
    };
    let json = serde_json::to_string(publish_event).ok()?;
    Some((event_type, json))
}
