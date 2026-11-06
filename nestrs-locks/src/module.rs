use crate::options::LocksOptions;
use crate::LocksService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct LocksModule;

impl LocksModule {
    pub fn register(options: LocksOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(LocksService::from_options(&options));
        registry.register_use_value::<LocksService>(service);
        registry.register_use_value::<LocksOptions>(Arc::new(options));
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![
                TypeId::of::<LocksService>(),
                TypeId::of::<LocksOptions>(),
            ],
        )
    }
}
