//! WebSocket gateway primitives for `nestrs` (NestJS `WebSocketGateway` analogue).
//!
//! Protocol: we default to **event-shaped JSON** frames:
//! `{ "event": "name", "data": <json> }`.
//!
//! The `nestrs-macros` crate can generate a dispatch implementation via `#[ws_routes]` + `#[subscribe_message("...")]`.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent<T> {
    pub event: String,
    pub data: T,
}

#[derive(Debug)]
pub enum WsSendError {
    Serialize(String),
    Closed,
}

/// Handle for emitting messages back to a single connected client.
#[derive(Clone)]
pub struct WsClient {
    tx: mpsc::UnboundedSender<Message>,
}

impl WsClient {
    pub fn emit<T: Serialize>(&self, event: &str, data: T) -> Result<(), WsSendError> {
        let payload =
            serde_json::to_value(data).map_err(|e| WsSendError::Serialize(e.to_string()))?;
        self.emit_json(event, payload)
    }

    pub fn emit_json(&self, event: &str, data: serde_json::Value) -> Result<(), WsSendError> {
        let frame = WsEvent {
            event: event.to_string(),
            data,
        };
        let text =
            serde_json::to_string(&frame).map_err(|e| WsSendError::Serialize(e.to_string()))?;
        self.tx
            .send(Message::Text(text))
            .map_err(|_| WsSendError::Closed)
    }
}

#[async_trait::async_trait]
pub trait WsGateway: Send + Sync + 'static {
    async fn on_message(&self, client: WsClient, event: &str, payload: serde_json::Value);
}

pub fn ws_route<G>(gateway: Arc<G>) -> axum::routing::MethodRouter
where
    G: WsGateway,
{
    axum::routing::get(move |ws: WebSocketUpgrade| {
        let gw = gateway.clone();
        async move { ws.on_upgrade(move |socket| serve_socket(socket, gw)) }
    })
}

pub async fn serve_socket<G>(socket: WebSocket, gateway: Arc<G>)
where
    G: WsGateway,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let client = WsClient { tx };

    let (mut ws_tx, mut ws_rx) = socket.split();

    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(ev) = serde_json::from_str::<WsEvent<serde_json::Value>>(&text) {
                    gateway
                        .on_message(client.clone(), ev.event.as_str(), ev.data)
                        .await;
                } else {
                    let _ = client.emit(
                        "error",
                        serde_json::json!({
                            "message": "invalid websocket payload (expected {event,data})"
                        }),
                    );
                }
            }
            Message::Binary(bin) => {
                if let Ok(text) = std::str::from_utf8(&bin) {
                    if let Ok(ev) = serde_json::from_str::<WsEvent<serde_json::Value>>(text) {
                        gateway
                            .on_message(client.clone(), ev.event.as_str(), ev.data)
                            .await;
                    } else {
                        let _ = client.emit(
                            "error",
                            serde_json::json!({
                                "message": "invalid websocket payload (expected {event,data})"
                            }),
                        );
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Dropping the sender closes the writer loop.
    drop(client);
    let _ = write_task.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ws_client_emit_json_sends_event_frame() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let client = WsClient { tx };

        client
            .emit_json("ping", serde_json::json!({ "ok": true }))
            .expect("send");

        let msg = rx.recv().await.expect("recv");
        let Message::Text(text) = msg else {
            panic!("expected text frame");
        };
        let ev: WsEvent<serde_json::Value> = serde_json::from_str(&text).expect("json");
        assert_eq!(ev.event, "ping");
        assert_eq!(ev.data["ok"], true);
    }
}
