use crate::options::IdempotencyOptions;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct IdempotencyModule;

impl IdempotencyModule {
    pub fn register(options: IdempotencyOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        registry.register_use_value::<IdempotencyOptions>(Arc::new(options));
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<IdempotencyOptions>()],
        )
    }
}
