//! In-process async event bus for domain and integration events (`#[on_event]` / Nest `EventEmitter2`).
//!
//! This crate is separate from `nestrs-microservices` so HTTP-only apps can depend on events
//! without pulling transport adapters.

use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

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

    pub async fn emit<T>(&self, pattern: &str, payload: &T)
    where
        T: Serialize + Send + Sync,
    {
        let json = serde_json::to_value(payload).unwrap_or_else(|e| {
            panic!("EventBus emit serialize failed for pattern `{pattern}`: {e}")
        });
        self.emit_json(pattern, json).await;
    }
}

#[async_trait]
impl nestrs_core::Injectable for EventBus {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::new())
    }
}
