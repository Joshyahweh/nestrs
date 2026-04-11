//! Kafka transport stub when the `kafka` feature is disabled.

use async_trait::async_trait;
use serde_json::Value;

use crate::{Transport, TransportError};

/// Placeholder transport: enable the **`kafka`** feature and use [`super::KafkaTransportOptions`](crate::kafka) for a real client.
#[derive(Debug, Clone, Default)]
pub struct KafkaTransport;

impl KafkaTransport {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transport for KafkaTransport {
    async fn send_json(&self, pattern: &str, _payload: Value) -> Result<Value, TransportError> {
        Err(TransportError::new(format!(
            "Kafka is disabled: enable `nestrs-microservices/kafka` (attempted pattern `{pattern}`)"
        )))
    }

    async fn emit_json(&self, pattern: &str, _payload: Value) -> Result<(), TransportError> {
        Err(TransportError::new(format!(
            "Kafka is disabled: enable `nestrs-microservices/kafka` (attempted pattern `{pattern}`)"
        )))
    }
}
