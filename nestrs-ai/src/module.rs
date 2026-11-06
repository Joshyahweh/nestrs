use crate::options::AiOptions;
use crate::service::AiService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct AiModule;

impl AiModule {
    pub fn register(options: AiOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(AiService::new(options));
        registry.register_use_value::<AiService>(service);
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<AiService>()],
        )
    }
}
