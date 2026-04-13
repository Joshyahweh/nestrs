use crate::ProviderRegistry;
use std::sync::Arc;

/// NestJS [`ModuleRef`](https://docs.nestjs.com/fundamentals/module-ref) analogue: typed access to the
/// root [`ProviderRegistry`] after the application graph is built.
///
/// Obtain it from [`NestApplication::module_ref`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.module_ref)
/// in the main `nestrs` crate (after `NestFactory::create`). Use [`Self::get`] for dynamic resolution
/// of registered providers by type.
///
/// **Docs:** the repository mdBook **Fundamentals** chapter (`docs/src/fundamentals.md`) describes
/// patterns alongside [`DiscoveryService`](crate::DiscoveryService) and lifecycle ordering.
#[derive(Clone)]
pub struct ModuleRef {
    registry: Arc<ProviderRegistry>,
}

impl ModuleRef {
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }

    pub fn into_inner(self) -> Arc<ProviderRegistry> {
        self.registry
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.registry.get()
    }

    pub fn registry(&self) -> &ProviderRegistry {
        self.registry.as_ref()
    }

    pub fn registry_arc(&self) -> &Arc<ProviderRegistry> {
        &self.registry
    }
}
