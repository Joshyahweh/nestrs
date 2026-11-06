use crate::options::WebhooksOptions;
use crate::WebhooksService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct WebhooksModule;

impl WebhooksModule {
    pub fn register(options: WebhooksOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(WebhooksService::new(options.secret.clone(), options.outbox.clone()));
        registry.register_use_value::<WebhooksService>(service);
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<WebhooksService>()],
        )
    }
}
