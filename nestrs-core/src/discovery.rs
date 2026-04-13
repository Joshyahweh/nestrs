use crate::{ModuleRef, RouteInfo, RouteRegistry};
use std::any::TypeId;

/// NestJS [`DiscoveryService`](https://docs.nestjs.com/fundamentals/discovery-service) analogue:
/// introspect registered providers and compiled HTTP routes.
///
/// Unlike Nest, there is no reflection over arbitrary class metadata: you get [`TypeId`](std::any::TypeId)
/// keys, type **names** (strings), and whatever is in the global [`RouteRegistry`](crate::RouteRegistry).
/// Construct with [`Self::new`](DiscoveryService::new)([`ModuleRef`](crate::ModuleRef)).
///
/// **Docs:** see the mdBook **Fundamentals** chapter in the `nestrs` repo (`docs/src/fundamentals.md`).
pub struct DiscoveryService {
    module: ModuleRef,
}

impl DiscoveryService {
    pub fn new(module: ModuleRef) -> Self {
        Self { module }
    }

    pub fn module_ref(&self) -> ModuleRef {
        self.module.clone()
    }

    pub fn get_providers(&self) -> Vec<TypeId> {
        self.module.registry().registered_type_ids()
    }

    pub fn get_provider_type_names(&self) -> Vec<&'static str> {
        self.module.registry().registered_type_names()
    }

    pub fn get_routes(&self) -> Vec<RouteInfo> {
        RouteRegistry::list()
    }
}
