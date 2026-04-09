//! Optional WebSocket gateway primitives for nestrs (Phase 4 roadmap crate).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent<T> {
    pub event: String,
    pub data: T,
}

#[async_trait]
pub trait GatewayHandler: Send + Sync + 'static {
    async fn on_message(&self, event: &str, payload: serde_json::Value) -> Result<(), String>;
}
