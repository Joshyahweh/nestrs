//! Optional microservices transport primitives for nestrs (Phase 4 roadmap crate).
//!
//! This crate intentionally starts with a tiny, stable interface so transports (NATS/Redis/gRPC)
//! can be added incrementally without blocking core HTTP framework progress.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope<T> {
    pub pattern: String,
    pub payload: T,
}

#[derive(Debug, Clone)]
pub struct TransportError {
    pub message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn send_json(&self, pattern: &str, payload: serde_json::Value) -> Result<serde_json::Value, TransportError>;
    async fn emit_json(&self, pattern: &str, payload: serde_json::Value) -> Result<(), TransportError>;
}

/// Nest-like client proxy wrapper over a configured [`Transport`].
#[derive(Clone)]
pub struct ClientProxy {
    transport: Arc<dyn Transport>,
}

impl ClientProxy {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    pub async fn send<TReq, TRes>(
        &self,
        pattern: &str,
        payload: &TReq,
    ) -> Result<TRes, TransportError>
    where
        TReq: Serialize + Send + Sync,
        TRes: for<'de> Deserialize<'de> + Send,
    {
        let req = serde_json::to_value(payload)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;
        let res = self.transport.send_json(pattern, req).await?;
        serde_json::from_value(res)
            .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))
    }

    pub async fn emit<TReq>(&self, pattern: &str, payload: &TReq) -> Result<(), TransportError>
    where
        TReq: Serialize + Send + Sync,
    {
        let req = serde_json::to_value(payload)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
        self.transport.emit_json(pattern, req).await
    }
}

type EventHandler =
    Arc<dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// In-process async event bus for integration/domain events.
#[derive(Default)]
pub struct EventBus {
    handlers: RwLock<HashMap<String, Vec<EventHandler>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe<F, Fut>(&self, pattern: impl Into<String>, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut guard = self.handlers.write().expect("event bus lock poisoned");
        let entry = guard.entry(pattern.into()).or_default();
        entry.push(Arc::new(move |payload| Box::pin(handler(payload))));
    }

    pub async fn emit_json(&self, pattern: &str, payload: serde_json::Value) {
        let handlers = {
            let guard = self.handlers.read().expect("event bus lock poisoned");
            guard.get(pattern).cloned().unwrap_or_default()
        };
        for handler in handlers {
            handler(payload.clone()).await;
        }
    }
}
