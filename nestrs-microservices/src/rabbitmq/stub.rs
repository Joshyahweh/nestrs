//! RabbitMQ transport stub when the `rabbitmq` feature is disabled.

use async_trait::async_trait;
use serde_json::Value;

use crate::{Transport, TransportError};

/// Placeholder transport: enable the **`rabbitmq`** feature for a real [`lapin`](https://docs.rs/lapin) client.
#[derive(Debug, Clone, Default)]
pub struct RabbitMqTransport;

impl RabbitMqTransport {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transport for RabbitMqTransport {
    async fn send_json(&self, pattern: &str, _payload: Value) -> Result<Value, TransportError> {
        Err(TransportError::new(format!(
            "RabbitMQ is disabled: enable `nestrs-microservices/rabbitmq` (attempted pattern `{pattern}`)"
        )))
    }

    async fn emit_json(&self, pattern: &str, _payload: Value) -> Result<(), TransportError> {
        Err(TransportError::new(format!(
            "RabbitMQ is disabled: enable `nestrs-microservices/rabbitmq` (attempted pattern `{pattern}`)"
        )))
    }
}
