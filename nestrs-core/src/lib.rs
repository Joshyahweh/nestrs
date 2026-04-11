use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use async_trait::async_trait;
use axum::Router;

mod guard;
mod metadata;
mod pipe;
mod route_registry;
mod strategy;

pub use guard::{CanActivate, GuardError};
pub use metadata::MetadataRegistry;
pub use pipe::PipeTransform;
pub use route_registry::{RouteInfo, RouteRegistry};
pub use strategy::{AuthError, AuthStrategy};

/// Provider lifetime semantics (NestJS `Scope.DEFAULT` / `Scope.TRANSIENT` analogues).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderScope {
    /// One instance per application container (default).
    Singleton,
    /// A new instance is created on every injection site / resolution.
    Transient,
    /// One instance per request/task scope (requires request-scope middleware).
    Request,
}

struct ProviderEntry {
    type_name: &'static str,
    scope: ProviderScope,
    factory: fn(&ProviderRegistry) -> Arc<dyn Any + Send + Sync>,
    instance: OnceLock<Arc<dyn Any + Send + Sync>>,
    on_module_init: HookFn,
    on_module_destroy: HookFn,
    on_application_bootstrap: HookFn,
    on_application_shutdown: HookFn,
}

pub struct ProviderRegistry {
    entries: HashMap<TypeId, ProviderEntry>,
}

/// Per-request handle identifying the matched handler (used for metadata lookups).
#[derive(Clone, Copy, Debug)]
pub struct HandlerKey(pub &'static str);

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn register<T>(&mut self)
    where
        T: Injectable + Send + Sync + 'static,
    {
        fn factory<T>(registry: &ProviderRegistry) -> Arc<dyn Any + Send + Sync>
        where
            T: Injectable + Send + Sync + 'static,
        {
            let value: Arc<dyn Any + Send + Sync> = T::construct(registry);
            value
        }

        self.entries.insert(
            TypeId::of::<T>(),
            ProviderEntry {
                type_name: std::any::type_name::<T>(),
                scope: T::scope(),
                factory: factory::<T>,
                instance: OnceLock::new(),
                on_module_init: hook_on_module_init::<T>,
                on_module_destroy: hook_on_module_destroy::<T>,
                on_application_bootstrap: hook_on_application_bootstrap::<T>,
                on_application_shutdown: hook_on_application_shutdown::<T>,
            },
        );
    }

    /// Override a provider with a concrete singleton instance (testing utility).
    ///
    /// This is primarily intended for `TestingModule`-style overrides where you want to replace an
    /// injectable with a mock instance.
    pub fn override_provider<T>(&mut self, instance: Arc<T>)
    where
        T: Injectable + Send + Sync + 'static,
    {
        fn factory<T>(registry: &ProviderRegistry) -> Arc<dyn Any + Send + Sync>
        where
            T: Injectable + Send + Sync + 'static,
        {
            let value: Arc<dyn Any + Send + Sync> = T::construct(registry);
            value
        }

        let mut entry = ProviderEntry {
            type_name: std::any::type_name::<T>(),
            scope: ProviderScope::Singleton,
            factory: factory::<T>,
            instance: OnceLock::new(),
            on_module_init: hook_on_module_init::<T>,
            on_module_destroy: hook_on_module_destroy::<T>,
            on_application_bootstrap: hook_on_application_bootstrap::<T>,
            on_application_shutdown: hook_on_application_shutdown::<T>,
        };

        let any: Arc<dyn Any + Send + Sync> = instance;
        let _ = entry.instance.set(any);

        self.entries.insert(TypeId::of::<T>(), entry);
    }

    pub fn get<T>(&self) -> Arc<T>
    where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let entry = self
            .entries
            .get(&type_id)
            .unwrap_or_else(|| panic!("Provider `{}` not registered", std::any::type_name::<T>()));

        let any = match entry.scope {
            ProviderScope::Singleton => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                entry.instance.get_or_init(|| (entry.factory)(self)).clone()
            }
            ProviderScope::Transient => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                (entry.factory)(self)
            }
            ProviderScope::Request => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                REQUEST_SCOPE_CACHE
                    .try_with(|cell| {
                        if let Some(existing) = cell.borrow().get(&type_id).cloned() {
                            return existing;
                        }
                        let value = (entry.factory)(self);
                        cell.borrow_mut().insert(type_id, value.clone());
                        value
                    })
                    .unwrap_or_else(|_| {
                        panic!(
                            "Request-scoped provider `{}` requested outside request scope; enable request scope middleware",
                            entry.type_name
                        )
                    })
            }
        };

        any.downcast::<T>().unwrap_or_else(|_| {
            panic!(
                "Provider downcast failed for `{}`",
                std::any::type_name::<T>()
            )
        })
    }

    pub fn absorb(&mut self, other: ProviderRegistry) {
        self.entries.extend(other.entries);
    }

    pub fn absorb_exported(&mut self, other: ProviderRegistry, exported: &[TypeId]) {
        if exported.is_empty() {
            return;
        }
        let allow = exported.iter().copied().collect::<HashSet<_>>();
        for (type_id, entry) in other.entries {
            if allow.contains(&type_id) {
                self.entries.insert(type_id, entry);
            }
        }
    }

    /// Construct all singleton providers (so their lifecycle hooks can run deterministically).
    pub fn eager_init_singletons(&self) {
        for (type_id, entry) in self.entries.iter() {
            if entry.scope == ProviderScope::Singleton {
                let _guard = ConstructionGuard::push(*type_id, entry.type_name);
                let _ = entry.instance.get_or_init(|| (entry.factory)(self));
            }
        }
    }

    pub async fn run_on_module_init(&self) {
        for entry in self.entries.values() {
            if entry.scope == ProviderScope::Singleton {
                (entry.on_module_init)(self).await;
            }
        }
    }

    pub async fn run_on_module_destroy(&self) {
        for entry in self.entries.values() {
            if entry.scope == ProviderScope::Singleton {
                (entry.on_module_destroy)(self).await;
            }
        }
    }

    pub async fn run_on_application_bootstrap(&self) {
        for entry in self.entries.values() {
            if entry.scope == ProviderScope::Singleton {
                (entry.on_application_bootstrap)(self).await;
            }
        }
    }

    pub async fn run_on_application_shutdown(&self) {
        for entry in self.entries.values() {
            if entry.scope == ProviderScope::Singleton {
                (entry.on_application_shutdown)(self).await;
            }
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

type HookFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
type HookFn = for<'a> fn(&'a ProviderRegistry) -> HookFuture<'a>;

fn hook_on_module_init<'a, T>(registry: &'a ProviderRegistry) -> HookFuture<'a>
where
    T: Injectable + Send + Sync + 'static,
{
    Box::pin(async move {
        let v = registry.get::<T>();
        v.on_module_init().await;
    })
}

fn hook_on_module_destroy<'a, T>(registry: &'a ProviderRegistry) -> HookFuture<'a>
where
    T: Injectable + Send + Sync + 'static,
{
    Box::pin(async move {
        let v = registry.get::<T>();
        v.on_module_destroy().await;
    })
}

fn hook_on_application_bootstrap<'a, T>(registry: &'a ProviderRegistry) -> HookFuture<'a>
where
    T: Injectable + Send + Sync + 'static,
{
    Box::pin(async move {
        let v = registry.get::<T>();
        v.on_application_bootstrap().await;
    })
}

fn hook_on_application_shutdown<'a, T>(registry: &'a ProviderRegistry) -> HookFuture<'a>
where
    T: Injectable + Send + Sync + 'static,
{
    Box::pin(async move {
        let v = registry.get::<T>();
        v.on_application_shutdown().await;
    })
}

#[async_trait]
pub trait Injectable: Send + Sync + 'static {
    fn construct(registry: &ProviderRegistry) -> Arc<Self>;

    /// Provider scope used when the module registers this type.
    fn scope() -> ProviderScope {
        ProviderScope::Singleton
    }

    async fn on_module_init(&self) {}
    async fn on_module_destroy(&self) {}
    async fn on_application_bootstrap(&self) {}
    async fn on_application_shutdown(&self) {}
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

/// Testing-oriented module traversal API.
///
/// Unlike [`Module::build`], implementations are expected to register *all* providers and controllers
/// from the import graph into a shared registry/router, so tests can apply overrides before
/// controllers are registered.
pub trait ModuleGraph {
    fn register_providers(registry: &mut ProviderRegistry);
    fn register_controllers(router: Router, registry: &ProviderRegistry) -> Router;
}

/// Runtime-composed module unit for conditional imports (feature flags, env switches, plugins).
///
/// This is a lightweight bridge until fully generic dynamic modules and scoped provider graphs
/// are implemented.
pub struct DynamicModule {
    /// Provider registry for this dynamic module.
    pub registry: ProviderRegistry,
    pub router: Router,
    /// Types exported to importing modules.
    pub exports: Vec<TypeId>,
}

impl DynamicModule {
    /// Builds a dynamic module from a static [`Module`] type.
    pub fn from_module<M: Module>() -> Self {
        let (registry, router) = M::build();
        let exports = <M as Module>::exports();
        Self {
            registry,
            router,
            exports,
        }
    }

    /// Wrap an already-built [`Router`] subtree as a dynamic module.
    pub fn from_router(router: Router) -> Self {
        Self {
            registry: ProviderRegistry::new(),
            router,
            exports: Vec::new(),
        }
    }

    /// Construct a dynamic module from explicit parts.
    pub fn from_parts(registry: ProviderRegistry, router: Router, exports: Vec<TypeId>) -> Self {
        Self {
            registry,
            router,
            exports,
        }
    }
}

/// Typed runtime options token for configurable modules.
///
/// This is intended to be provided via `ConfigurableModuleBuilder` / `DynamicModuleBuilder`
/// (it panics if requested without an override).
pub struct ModuleOptions<O, M> {
    inner: O,
    _marker: std::marker::PhantomData<fn() -> M>,
}

impl<O, M> ModuleOptions<O, M> {
    pub fn new(inner: O) -> Self {
        Self {
            inner,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get(&self) -> &O {
        &self.inner
    }

    pub fn into_inner(self) -> O {
        self.inner
    }
}

impl<O, M> std::ops::Deref for ModuleOptions<O, M> {
    type Target = O;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[async_trait]
impl<O, M> Injectable for ModuleOptions<O, M>
where
    O: Send + Sync + 'static,
    M: 'static,
{
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        panic!(
            "ModuleOptions requested but no value was provided. Use ConfigurableModuleBuilder / DynamicModuleBuilder to supply module options."
        );
    }
}

/// Builds a [`DynamicModule`] from a static module graph, optionally applying provider overrides
/// before controllers are registered (useful for configurable modules and testing-like setups).
pub struct DynamicModuleBuilder<M>
where
    M: Module + ModuleGraph,
{
    overrides: Vec<Box<dyn FnOnce(&mut ProviderRegistry) + Send>>,
    _marker: std::marker::PhantomData<M>,
}

impl<M> DynamicModuleBuilder<M>
where
    M: Module + ModuleGraph,
{
    pub fn new() -> Self {
        Self {
            overrides: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn override_provider<T>(mut self, instance: Arc<T>) -> Self
    where
        T: Injectable + Send + Sync + 'static,
    {
        self.overrides
            .push(Box::new(move |r| r.override_provider::<T>(instance)));
        self
    }

    pub fn build(self) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        M::register_providers(&mut registry);
        for apply in self.overrides {
            apply(&mut registry);
        }
        let router = M::register_controllers(Router::new(), &registry);
        DynamicModule::from_parts(registry, router, M::exports())
    }
}

/// Convenience builder for NestJS-like configurable modules (`for_root`, `for_root_async`).
pub struct ConfigurableModuleBuilder<O> {
    _marker: std::marker::PhantomData<O>,
}

impl<O> ConfigurableModuleBuilder<O>
where
    O: Send + Sync + 'static,
{
    pub fn for_root<M>(options: O) -> DynamicModule
    where
        M: Module + ModuleGraph + 'static,
    {
        DynamicModuleBuilder::<M>::new()
            .override_provider::<ModuleOptions<O, M>>(Arc::new(ModuleOptions::new(options)))
            .build()
    }

    pub async fn for_root_async<M, F, Fut>(factory: F) -> DynamicModule
    where
        M: Module + ModuleGraph + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = O>,
    {
        let options = factory().await;
        Self::for_root::<M>(options)
    }
}

thread_local! {
    static MODULE_BUILD_STACK: std::cell::RefCell<Vec<(&'static str, TypeId)>> =
        std::cell::RefCell::new(Vec::new());
}

/// Internal module build/graph traversal guard (used by `#[module]`-generated code).
#[doc(hidden)]
pub struct __NestrsModuleBuildGuard {
    type_id: TypeId,
}

impl __NestrsModuleBuildGuard {
    pub fn push(type_id: TypeId, type_name: &'static str) -> Self {
        let is_cycle = MODULE_BUILD_STACK.with(|stack| {
            let mut guard = stack.borrow_mut();
            let cycle = guard.iter().any(|(_, id)| *id == type_id);
            if !cycle {
                guard.push((type_name, type_id));
            }
            cycle
        });

        if is_cycle {
            __nestrs_panic_circular_module_dependency(type_name);
        }

        Self { type_id }
    }
}

impl Drop for __NestrsModuleBuildGuard {
    fn drop(&mut self) {
        MODULE_BUILD_STACK.with(|stack| {
            let mut guard = stack.borrow_mut();
            if let Some((_, id)) = guard.last() {
                if *id == self.type_id {
                    guard.pop();
                }
            }
        });
    }
}

#[doc(hidden)]
pub fn __nestrs_module_stack_contains(type_id: TypeId) -> bool {
    MODULE_BUILD_STACK.with(|stack| stack.borrow().iter().any(|(_, id)| *id == type_id))
}

#[doc(hidden)]
pub fn __nestrs_panic_circular_module_dependency(import_type_name: &'static str) -> ! {
    let chain = MODULE_BUILD_STACK.with(|stack| {
        stack
            .borrow()
            .iter()
            .map(|(name, _)| *name)
            .chain(std::iter::once(import_type_name))
            .collect::<Vec<_>>()
            .join(" -> ")
    });

    panic!(
        "Circular module dependency detected: {chain}. If intentional, mark the back-edge import with `forward_ref::<T>()`.",
    );
}

tokio::task_local! {
    static REQUEST_SCOPE_CACHE: std::cell::RefCell<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>;
}

/// Runs `future` with an empty request-scoped provider cache (used by request middleware).
pub async fn with_request_scope<Fut, T>(future: Fut) -> T
where
    Fut: std::future::Future<Output = T>,
{
    REQUEST_SCOPE_CACHE
        .scope(std::cell::RefCell::new(HashMap::new()), future)
        .await
}

thread_local! {
    static CONSTRUCTION_STACK: std::cell::RefCell<Vec<(&'static str, TypeId)>> =
        std::cell::RefCell::new(Vec::new());
}

struct ConstructionGuard {
    type_id: TypeId,
}

impl ConstructionGuard {
    fn push(type_id: TypeId, type_name: &'static str) -> Self {
        CONSTRUCTION_STACK.with(|stack| {
            let mut guard = stack.borrow_mut();
            if guard.iter().any(|(_, id)| *id == type_id) {
                let chain = guard
                    .iter()
                    .map(|(name, _)| *name)
                    .chain(std::iter::once(type_name))
                    .collect::<Vec<_>>()
                    .join(" -> ");
                panic!("Circular provider dependency detected: {chain}");
            }
            guard.push((type_name, type_id));
        });
        Self { type_id }
    }
}

impl Drop for ConstructionGuard {
    fn drop(&mut self) {
        CONSTRUCTION_STACK.with(|stack| {
            let mut guard = stack.borrow_mut();
            if let Some((_, id)) = guard.last() {
                if *id == self.type_id {
                    guard.pop();
                }
            }
        });
    }
}
