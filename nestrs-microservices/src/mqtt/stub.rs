//! MQTT transport stub when the `mqtt` feature is disabled.

use async_trait::async_trait;
use serde_json::Value;

use crate::{Transport, TransportError};

/// Placeholder transport: enable the **`mqtt`** feature for a real [`rumqttc`](https://docs.rs/rumqttc) client.
#[derive(Debug, Clone, Default)]
pub struct MqttTransport;

impl MqttTransport {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transport for MqttTransport {
    async fn send_json(&self, pattern: &str, _payload: Value) -> Result<Value, TransportError> {
        Err(TransportError::new(format!(
            "MQTT is disabled: enable `nestrs-microservices/mqtt` (attempted pattern `{pattern}`)"
        )))
    }

    async fn emit_json(&self, pattern: &str, _payload: Value) -> Result<(), TransportError> {
        Err(TransportError::new(format!(
            "MQTT is disabled: enable `nestrs-microservices/mqtt` (attempted pattern `{pattern}`)"
        )))
    }
}
