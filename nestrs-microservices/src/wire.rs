//! Shared JSON wire format for Redis, Kafka, MQTT, RabbitMQ, custom transporters, and the JSON
//! payloads carried by the gRPC adapter (`nestrs.microservices` proto).
//!
//! ## Format stability
//!
//! The JSON shapes are covered by **golden tests** in this crate (`tests/wire_conformance.rs` +
//! `tests/fixtures/*.json`) and the **`wire_json`** **`cargo-fuzz`** target (`fuzz/fuzz_targets/wire_json.rs`).
//! Bump [`WIRE_FORMAT_DOC_REVISION`] when you intentionally change fields or serialization so release
//! notes can call out wire compatibility.
//!
//! Revision **`1`**: `WireKind` as snake_case strings (`send`, `emit`); `WireRequest` with optional
//! `reply` and `correlation_id`; `WireResponse` with `ok`, optional `payload`, optional `error`
//! (`message` + optional `details`).

/// Human-readable revision for release notes and external integrators (not sent on the wire).
pub const WIRE_FORMAT_DOC_REVISION: u32 = 1;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::{MicroserviceHandler, TransportError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKind {
    Send,
    Emit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireRequest {
    pub kind: WireKind,
    pub pattern: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// Kafka (and similar) request–reply: reply record key for demux on a shared reply topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

pub async fn dispatch_send(
    handlers: &[Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: Value,
) -> Result<Value, TransportError> {
    for h in handlers {
        if let Some(res) = h.handle_message(pattern, payload.clone()).await {
            return res;
        }
    }
    Err(TransportError::new(format!(
        "no microservice handler for pattern `{pattern}`"
    )))
}

pub async fn dispatch_emit(
    handlers: &[Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: Value,
) {
    for h in handlers {
        let _ = h.handle_event(pattern, payload.clone()).await;
    }
}
