//! Shared JSON wire format for Redis / Kafka / MQTT microservice transports.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::{MicroserviceHandler, TransportError};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireKind {
    Send,
    Emit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireRequest {
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
pub(crate) struct WireError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

pub(crate) async fn dispatch_send(
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

pub(crate) async fn dispatch_emit(handlers: &[Arc<dyn MicroserviceHandler>], pattern: &str, payload: Value) {
    for h in handlers {
        let _ = h.handle_event(pattern, payload.clone()).await;
    }
}
