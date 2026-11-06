use crate::options::MailOptions;
use crate::MailService;
use axum::Router;
use nestrs_core::{DynamicModule, ProviderRegistry};
use std::any::TypeId;
use std::sync::Arc;

pub struct MailModule;

impl MailModule {
    pub fn register(options: MailOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(MailService::from_options(&options));
        registry.register_use_value::<MailService>(service);
        DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<MailService>()],
        )
    }
}
