use crate::options::WorkflowsOptions;
use crate::WorkflowsService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct WorkflowsModule;

impl WorkflowsModule {
    pub fn register(options: WorkflowsOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(WorkflowsService::from_options(&options));
        registry.register_use_value::<WorkflowsService>(service);
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<WorkflowsService>()],
        )
    }
}
