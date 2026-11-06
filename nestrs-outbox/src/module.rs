use crate::options::OutboxOptions;
use crate::relay::OutboxRelay;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct OutboxModule;

impl OutboxModule {
    pub fn register(options: OutboxOptions) -> DynamicModule {
        if !options.allow_in_memory {
            // Production gate: Nest refuses in-memory without allowInMemoryStorage.
            tracing::warn!("outbox: allow_in_memory=false; ensure a durable store is configured");
        }
        let mut registry = ProviderRegistry::new();
        let relay = Arc::new(OutboxRelay::from_options(&options));
        registry.register_use_value::<OutboxRelay>(relay);
        registry.register_use_value::<OutboxOptions>(Arc::new(options));
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![
                TypeId::of::<OutboxRelay>(),
                TypeId::of::<OutboxOptions>(),
            ],
        )
    }
}
