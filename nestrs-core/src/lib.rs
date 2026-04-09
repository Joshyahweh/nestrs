use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Router;

mod guard;
mod metadata;
mod pipe;
mod strategy;

pub use guard::{CanActivate, GuardError};
pub use metadata::MetadataRegistry;
pub use pipe::PipeTransform;
pub use strategy::{AuthError, AuthStrategy};

pub struct ProviderRegistry {
    providers: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register<T>(&mut self)
    where
        T: Injectable + Send + Sync + 'static,
    {
        let value = T::construct(self);
        self.providers.insert(TypeId::of::<T>(), value);
    }

    pub fn get<T>(&self) -> Arc<T>
    where
        T: Send + Sync + 'static,
    {
        let value = self
            .providers
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("Provider `{}` not registered", std::any::type_name::<T>()))
            .clone();

        value.downcast::<T>().unwrap_or_else(|_| {
            panic!(
                "Provider downcast failed for `{}`",
                std::any::type_name::<T>()
            )
        })
    }

    pub fn absorb(&mut self, other: ProviderRegistry) {
        self.providers.extend(other.providers);
    }

    pub fn absorb_exported(&mut self, other: ProviderRegistry, exported: &[TypeId]) {
        if exported.is_empty() {
            return;
        }
        let allow = exported.iter().copied().collect::<HashSet<_>>();
        for (type_id, provider) in other.providers {
            if allow.contains(&type_id) {
                self.providers.insert(type_id, provider);
            }
        }
    }
}

pub trait Injectable: Send + Sync + 'static {
    fn construct(registry: &ProviderRegistry) -> Arc<Self>;
}

pub trait Controller {
    fn register(router: Router, registry: &ProviderRegistry) -> Router;
}

pub trait Module {
    fn build() -> (ProviderRegistry, Router);

    fn exports() -> Vec<TypeId> {
        Vec::new()
    }
}

/// Runtime-composed module unit for conditional imports (feature flags, env switches, plugins).
///
/// This is a lightweight bridge until fully generic dynamic modules and scoped provider graphs
/// are implemented. It currently carries a prebuilt [`Router`] subtree.
pub struct DynamicModule {
    pub router: Router,
}

impl DynamicModule {
    /// Builds a dynamic module from a static [`Module`] type.
    pub fn from_module<M: Module>() -> Self {
        let (_registry, router) = M::build();
        Self { router }
    }

    /// Wrap an already-built [`Router`] subtree as a dynamic module.
    pub fn from_router(router: Router) -> Self {
        Self { router }
    }
}
