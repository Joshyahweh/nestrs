//! WebSocket gateway primitives for `nestrs` (NestJS `WebSocketGateway` analogue).
//!
//! ## Wire format
//!
//! Default frames are JSON: `{ "event": "name", "data": <json> }`.
//!
//! ## Adapters (Nest vs Axum)
//!
//! This crate uses **Axum’s `WebSocketUpgrade`** (browser `WebSocket` / RFC 6455). Nest’s **Socket.IO**
//! transport is a different protocol; for Socket.IO on Axum see the **[`socketioxide`](https://crates.io/crates/socketioxide)**
//! ecosystem, or keep event-shaped JSON and document your client contract.
//!
//! The `nestrs-macros` crate can generate dispatch via `#[ws_routes]` + `#[subscribe_message("...")]`.
//! Optional cross-cutting attributes: **`#[use_ws_interceptors(...)]`**, **`#[use_ws_guards(...)]`**,
//! **`#[use_ws_pipes(...)]`** (see crate docs on `nestrs`).
//!
//! ## Errors vs HTTP “exception filters”
//!
//! WebSocket messages are **not** Axum HTTP responses. [`NestApplication::use_global_exception_filter`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_global_exception_filter)
//! and [`ExceptionFilter`](https://docs.rs/nestrs/latest/nestrs/trait.ExceptionFilter.html) **do not** run on
//! JSON frames. Instead, failures are sent to the client on the event name [`WS_ERROR_EVENT`]
//! (`"error"`) with JSON bodies:
//!
//! - **Guards** ([`WsGuardError`]): `statusCode`, `message`, `error` (see [`WsGuardError::to_json`]).
//! - **Pipes** ([`WsPipeError`]): same top-level keys as guards for pipe failures (`statusCode` 400).
//! - **Unknown event** (generated `#[ws_routes]` default arm): `event`, `message` (`"unknown event"`).
//! - **Invalid typed payload** (deserialize into handler DTO): `event`, `message`, `details` (string).
//! - **Wire parse errors** in [`serve_socket`] (malformed `{event,data}`): `message` only.
//!
//! Treat these as the WebSocket analogue of Nest’s gateway exception filters: **centralize** by
//! wrapping [`WsGateway::on_message`] or using shared guard/pipe types; there is no separate
//! `WsExceptionFilter` trait in-core today.

/// Event name used for server→client error frames (guards, pipes, unknown event, bad payloads).
pub const WS_ERROR_EVENT: &str = "error";

pub mod adapters;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
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

/// Headers (and related data) from the HTTP upgrade request, available on each [`WsClient`].
#[derive(Clone, Debug)]
pub struct WsHandshake {
    headers: Arc<HeaderMap>,
}

impl Default for WsHandshake {
    fn default() -> Self {
        Self {
            headers: Arc::new(HeaderMap::new()),
        }
    }
}

impl WsHandshake {
    pub fn new(headers: HeaderMap) -> Self {
        Self {
            headers: Arc::new(headers),
        }
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// Failure from a WebSocket [`WsCanActivate`] guard (emitted as an `error` event by generated code).
#[derive(Debug, Clone)]
pub struct WsGuardError {
    pub status_code: u16,
    pub message: String,
    pub error: &'static str,
}

impl WsGuardError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status_code: 401,
            message: message.into(),
            error: "Unauthorized",
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status_code: 403,
            message: message.into(),
            error: "Forbidden",
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status_code: 400,
            message: message.into(),
            error: "Bad Request",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "statusCode": self.status_code,
            "message": self.message,
            "error": self.error,
        })
    }
}

/// Authorization / policy hook before a message handler runs (Nest `WsGuard` analogue).
#[async_trait::async_trait]
pub trait WsCanActivate: Default + Send + Sync + 'static {
    async fn can_activate_ws(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<(), WsGuardError>;
}

/// Transform inbound JSON after guards (Nest `Pipe` analogue for payloads).
#[async_trait::async_trait]
pub trait WsPipeTransform: Default + Send + Sync + 'static {
    async fn transform(
        &self,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, WsPipeError>;
}

#[derive(Debug, Clone)]
pub struct WsPipeError {
    pub message: String,
}

impl WsPipeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "statusCode": 400,
            "message": self.message,
            "error": "Bad Request",
        })
    }
}

/// Observe inbound messages (logging / metrics); does not fail the pipeline.
#[async_trait::async_trait]
pub trait WsIncomingInterceptor: Default + Send + Sync + 'static {
    async fn before_handle(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    );
}

/// Handle for emitting messages back to a single connected client.
#[derive(Clone)]
pub struct WsClient {
    tx: mpsc::UnboundedSender<Message>,
    handshake: WsHandshake,
}

impl WsClient {
    pub fn handshake(&self) -> &WsHandshake {
        &self.handshake
    }

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
    axum::routing::get(move |ws: WebSocketUpgrade, headers: HeaderMap| {
        let gw = gateway.clone();
        let handshake = WsHandshake::new(headers);
        async move { ws.on_upgrade(move |socket| serve_socket(socket, gw, handshake)) }
    })
}

pub async fn serve_socket<G>(socket: WebSocket, gateway: Arc<G>, handshake: WsHandshake)
where
    G: WsGateway,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let client = WsClient {
        tx,
        handshake: handshake.clone(),
    };

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
                    let c = WsClient {
                        tx: client.tx.clone(),
                        handshake: client.handshake.clone(),
                    };
                    gateway.on_message(c, ev.event.as_str(), ev.data).await;
                } else {
                    let c = WsClient {
                        tx: client.tx.clone(),
                        handshake: client.handshake.clone(),
                    };
                    let _ = c.emit(
                        WS_ERROR_EVENT,
                        serde_json::json!({
                            "message": "invalid websocket payload (expected {event,data})"
                        }),
                    );
                }
            }
            Message::Binary(bin) => {
                if let Ok(text) = std::str::from_utf8(&bin) {
                    if let Ok(ev) = serde_json::from_str::<WsEvent<serde_json::Value>>(text) {
                        let c = WsClient {
                            tx: client.tx.clone(),
                            handshake: client.handshake.clone(),
                        };
                        gateway.on_message(c, ev.event.as_str(), ev.data).await;
                    } else {
                        let c = WsClient {
                            tx: client.tx.clone(),
                            handshake: client.handshake.clone(),
                        };
                        let _ = c.emit(
                            WS_ERROR_EVENT,
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

    drop(client);
    let _ = write_task.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ws_client_emit_json_sends_event_frame() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let client = WsClient {
            tx,
            handshake: WsHandshake::default(),
        };

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
