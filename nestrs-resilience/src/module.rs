use crate::options::ResilienceOptions;
use crate::service::ResilienceService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct ResilienceModule;

impl ResilienceModule {
    pub fn register(options: ResilienceOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(ResilienceService::from_options(options));
        registry.register_use_value::<ResilienceService>(service);
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<ResilienceService>()],
        )
    }
}
