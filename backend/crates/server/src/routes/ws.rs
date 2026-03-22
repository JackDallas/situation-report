use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::SinkExt;
use serde::Deserialize;
use sr_pipeline::PublishEvent;
use std::collections::HashSet;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

// Use futures::StreamExt for WebSocket receiver (not tokio_stream)
use futures::StreamExt as FuturesStreamExt;

use crate::state::AppState;

/// Viewport bounds sent by the client.
#[derive(Debug, Clone)]
struct ViewportBounds {
    north: f64,
    south: f64,
    east: f64,
    west: f64,
}

impl ViewportBounds {
    fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.south && lat <= self.north && lon >= self.west && lon <= self.east
    }
}

/// Client-to-server message types.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Subscribe {
        channels: Vec<String>,
    },
    Viewport {
        bounds: ViewportBoundsMsg,
    },
}

#[derive(Debug, Deserialize)]
struct ViewportBoundsMsg {
    north: f64,
    south: f64,
    east: f64,
    west: f64,
}

/// Map a PublishEvent variant to its channel name for subscription filtering.
fn event_channel(publish_event: &PublishEvent) -> &'static str {
    match publish_event {
        PublishEvent::Event { .. } => "events",
        PublishEvent::Incident(_) => "incidents",
        PublishEvent::Summary(_) => "summaries",
        PublishEvent::Analysis(_) => "analysis",
        PublishEvent::Situations { .. } => "situations",
        PublishEvent::Alert(_) => "alerts",
        PublishEvent::SourceHealthChange { .. } => "source_health",
    }
}

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

    // Per-connection state: subscriptions and viewport
    let mut subscriptions: Option<HashSet<String>> = None; // None = all channels (default)
    let mut viewport: Option<ViewportBounds> = None;

    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming message from broadcast channel
            msg = tokio_stream::StreamExt::next(&mut stream) => {
                match msg {
                    Some(Ok(publish_event)) => {
                        // Check subscription filter
                        let channel = event_channel(&publish_event);
                        if let Some(ref subs) = subscriptions {
                            if !subs.contains(channel) {
                                continue;
                            }
                        }

                        // Check viewport filter for geo_event push
                        if let Some(ref vp) = viewport {
                            if let PublishEvent::Event { ref event } = publish_event {
                                if let (Some(lat), Some(lon)) = (event.latitude, event.longitude) {
                                    if vp.contains(lat, lon) {
                                        // Send as geo_event in addition to normal envelope
                                        let geo_envelope = serde_json::json!({
                                            "type": "geo_event",
                                            "data": {
                                                "event_type": event.event_type.to_string(),
                                                "latitude": lat,
                                                "longitude": lon,
                                                "entity_id": event.entity_id,
                                                "entity_name": event.entity_name,
                                                "title": event.title,
                                                "description": event.description,
                                                "severity": event.severity.to_string(),
                                                "source_type": event.source_type.to_string(),
                                                "event_time": event.event_time,
                                                "tags": event.tags,
                                            }
                                        });
                                        if let Ok(s) = serde_json::to_string(&geo_envelope) {
                                            if sender.send(Message::Text(s.into())).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }

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
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            match client_msg {
                                ClientMessage::Subscribe { channels } => {
                                    tracing::debug!("WS client subscribed to: {:?}", channels);
                                    subscriptions = Some(channels.into_iter().collect());
                                }
                                ClientMessage::Viewport { bounds } => {
                                    viewport = Some(ViewportBounds {
                                        north: bounds.north,
                                        south: bounds.south,
                                        east: bounds.east,
                                        west: bounds.west,
                                    });
                                }
                            }
                        }
                    }
                    _ => {} // ignore pong, binary, etc.
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
