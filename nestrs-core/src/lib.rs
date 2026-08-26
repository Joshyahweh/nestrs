use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;

use async_trait::async_trait;
use axum::Router;

mod database;
mod discovery;
mod execution_context;
mod guard;
mod metadata;
mod module_ref;
mod pipe;
mod platform;
mod route_registry;
mod strategy;

pub use database::DatabasePing;
pub use discovery::DiscoveryService;
pub use execution_context::{ExecutionContext, HostType, HttpExecutionArguments};
pub use guard::{CanActivate, GuardError};
pub use metadata::MetadataRegistry;
pub use module_ref::ModuleRef;
pub use pipe::PipeTransform;
pub use platform::{AxumHttpEngine, HttpServerEngine};
pub use route_registry::{OpenApiResponseDesc, OpenApiRouteSpec, RouteInfo, RouteRegistry};
pub use strategy::{AuthError, AuthStrategy};

type CustomFactoryFn =
    std::sync::Arc<dyn Fn(&ProviderRegistry) -> Arc<dyn Any + Send + Sync> + Send + Sync>;

/// Provider lifetime semantics (NestJS `Scope.DEFAULT` / `Scope.TRANSIENT` / `Scope.REQUEST` analogues).
///
/// Set per type via `#[injectable(scope = "singleton" | "transient" | "request")]` or pass to
/// [`ProviderRegistry::register_use_factory`]. **Request** scope requires the app to call
/// [`nestrs::NestApplication::use_request_scope`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_request_scope).
///
/// **Docs:** mdBook **Fundamentals** in the repository (`docs/src/fundamentals.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderScope {
    /// One instance per application container (default).
    Singleton,
    /// A new instance is created on every injection site / resolution.
    Transient,
    /// One instance per request/task scope (requires request-scope middleware).
    Request,
}

#[derive(Clone)]
enum ProviderFactory {
    InjectableFn(fn(&ProviderRegistry) -> Arc<dyn Any + Send + Sync>),
    Custom(CustomFactoryFn),
}

#[derive(Clone)]
struct ProviderEntry {
    type_name: &'static str,
    scope: ProviderScope,
    factory: ProviderFactory,
    instance: Arc<OnceLock<Arc<dyn Any + Send + Sync>>>,
    on_module_init: HookFn,
    on_module_destroy: HookFn,
    on_application_bootstrap: HookFn,
    on_application_shutdown: HookFn,
}

fn noop_hook<'a>(_registry: &'a ProviderRegistry) -> HookFuture<'a> {
    Box::pin(async {})
}

fn create_entry_for_injectable<T: Injectable + Send + Sync + 'static>() -> ProviderEntry {
    fn factory<T: Injectable + Send + Sync + 'static>(
        registry: &ProviderRegistry,
    ) -> Arc<dyn Any + Send + Sync> {
        T::construct(registry)
    }

    ProviderEntry {
        type_name: std::any::type_name::<T>(),
        scope: T::scope(),
        factory: ProviderFactory::InjectableFn(factory::<T>),
        instance: Arc::new(OnceLock::new()),
        on_module_init: hook_on_module_init::<T>,
        on_module_destroy: hook_on_module_destroy::<T>,
        on_application_bootstrap: hook_on_application_bootstrap::<T>,
        on_application_shutdown: hook_on_application_shutdown::<T>,
    }
}

pub struct ProviderRegistry {
    entries: HashMap<TypeId, ProviderEntry>,
    /// Registration order of providers. Iteration over [`Self::entries`] alone is nondeterministic
    /// (HashMap), so lifecycle hooks and discovery use this order for stable startup/shutdown.
    order: Vec<TypeId>,
}

/// Per-request handle identifying the matched handler (used for metadata lookups).
#[derive(Clone, Copy, Debug)]
pub struct HandlerKey(pub &'static str);

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn insert_entry(&mut self, type_id: TypeId, entry: ProviderEntry) {
        if !self.entries.contains_key(&type_id) {
            self.order.push(type_id);
        }
        self.entries.insert(type_id, entry);
    }

    pub fn register<T>(&mut self)
    where
        T: Injectable + Send + Sync + 'static,
    {
        self.insert_entry(TypeId::of::<T>(), create_entry_for_injectable::<T>());
    }

    /// NestJS **`useValue`**: register a pre-built singleton without an [`Injectable`] impl.
    pub fn register_use_value<T: Send + Sync + 'static>(&mut self, value: Arc<T>) {
        let preset: Arc<dyn Any + Send + Sync> = value;
        let cell = Arc::new(OnceLock::new());
        let _ = cell.set(preset.clone());
        self.insert_entry(
            TypeId::of::<T>(),
            ProviderEntry {
                type_name: std::any::type_name::<T>(),
                scope: ProviderScope::Singleton,
                factory: ProviderFactory::Custom(Arc::new(move |_| preset.clone())),
                instance: cell,
                on_module_init: noop_hook,
                on_module_destroy: noop_hook,
                on_application_bootstrap: noop_hook,
                on_application_shutdown: noop_hook,
            },
        );
    }

    /// NestJS **`useFactory`**: register a provider from a **synchronous** closure `Fn(&ProviderRegistry) -> Arc<T>`.
    ///
    /// The closure may call [`Self::get`] for dependencies. For **async** initialization of `T`, keep
    /// `construct`/`factory` cheap and use [`Injectable::on_module_init`] on `T`, or load **module
    /// options** with [`ConfigurableModuleBuilder::for_root_async`]. Do **not** block the async
    /// runtime inside the factory.
    ///
    /// Prefer [`Self::register`] when the provider is a normal `#[injectable]` type.
    pub fn register_use_factory<T, F>(&mut self, scope: ProviderScope, factory: F)
    where
        T: Send + Sync + 'static,
        F: Fn(&ProviderRegistry) -> Arc<T> + Send + Sync + 'static,
    {
        let factory: std::sync::Arc<F> = std::sync::Arc::new(factory);
        let factory = factory.clone();
        self.insert_entry(
            TypeId::of::<T>(),
            ProviderEntry {
                type_name: std::any::type_name::<T>(),
                scope,
                factory: ProviderFactory::Custom(Arc::new(move |r| {
                    let v = factory(r);
                    v as Arc<dyn Any + Send + Sync>
                })),
                instance: Arc::new(OnceLock::new()),
                on_module_init: noop_hook,
                on_module_destroy: noop_hook,
                on_application_bootstrap: noop_hook,
                on_application_shutdown: noop_hook,
            },
        );
    }

    /// NestJS **`useClass`**: equivalent to [`Self::register`] for a normal injectable type.
    #[inline]
    pub fn register_use_class<T>(&mut self)
    where
        T: Injectable + Send + Sync + 'static,
    {
        self.register::<T>();
    }

    /// Override a provider with a concrete singleton instance (testing utility).
    ///
    /// This is primarily intended for `TestingModule`-style overrides where you want to replace an
    /// injectable with a mock instance.
    pub fn override_provider<T>(&mut self, instance: Arc<T>)
    where
        T: Injectable + Send + Sync + 'static,
    {
        let entry = ProviderEntry {
            type_name: std::any::type_name::<T>(),
            scope: ProviderScope::Singleton,
            factory: ProviderFactory::InjectableFn(|_| unreachable!("override preset")),
            instance: Arc::new(OnceLock::new()),
            on_module_init: hook_on_module_init::<T>,
            on_module_destroy: hook_on_module_destroy::<T>,
            on_application_bootstrap: hook_on_application_bootstrap::<T>,
            on_application_shutdown: hook_on_application_shutdown::<T>,
        };

        let any: Arc<dyn Any + Send + Sync> = instance;
        let _ = entry.instance.set(any);

        self.insert_entry(TypeId::of::<T>(), entry);
    }

    fn produce_any(&self, type_id: TypeId, entry: &ProviderEntry) -> Arc<dyn Any + Send + Sync> {
        match entry.scope {
            ProviderScope::Singleton => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                entry
                    .instance
                    .get_or_init(|| match &entry.factory {
                        ProviderFactory::InjectableFn(f) => f(self),
                        ProviderFactory::Custom(f) => f(self),
                    })
                    .clone()
            }
            ProviderScope::Transient => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                match &entry.factory {
                    ProviderFactory::InjectableFn(f) => f(self),
                    ProviderFactory::Custom(f) => f(self),
                }
            }
            ProviderScope::Request => {
                let _guard = ConstructionGuard::push(type_id, entry.type_name);
                REQUEST_SCOPE_CACHE
                    .try_with(|cell| {
                        if let Some(existing) = cell.borrow().get(&type_id).cloned() {
                            return existing;
                        }
                        let value = match &entry.factory {
                            ProviderFactory::InjectableFn(f) => f(self),
                            ProviderFactory::Custom(f) => f(self),
                        };
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
        }
    }

    /// Resolves a provider, panicking when it is not registered. Prefer
    /// [`Self::try_get`] at call sites that can handle absence.
    ///
    /// When called **during** another provider's construction (inside `construct` or a
    /// `useFactory` closure), the edge `constructor -> requested` is recorded so lifecycle
    /// hooks can run in dependency order (see [`Self::run_on_module_init`]).
    pub fn get<T>(&self) -> Arc<T>
    where
        T: Send + Sync + 'static,
    {
        self.try_get::<T>()
            .unwrap_or_else(|| panic!("Provider `{}` not registered", std::any::type_name::<T>()))
    }

    /// Fallible resolution: returns `None` instead of panicking when the provider is missing.
    pub fn try_get<T>(&self) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let entry = self.entries.get(&type_id)?;

        if let Some(parent) =
            CONSTRUCTION_STACK.with(|stack| stack.borrow().last().map(|(_, id)| *id))
        {
            record_provider_dependency(parent, type_id);
        }

        let any = self.produce_any(type_id, entry);

        any.downcast::<T>().ok()
    }

    /// All registered provider [`TypeId`] keys (NestJS discovery-style introspection), in registration order.
    pub fn registered_type_ids(&self) -> Vec<TypeId> {
        self.order.clone()
    }

    /// Human-readable type names for registered providers (debug / tooling), in registration order.
    pub fn registered_type_names(&self) -> Vec<&'static str> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id).map(|e| e.type_name))
            .collect()
    }

    pub fn absorb(&mut self, other: ProviderRegistry) {
        let ProviderRegistry { entries, order } = other;
        let mut leftover = entries;
        for type_id in order {
            if let Some(entry) = leftover.remove(&type_id) {
                self.insert_entry(type_id, entry);
            }
        }
        for (type_id, entry) in leftover {
            self.insert_entry(type_id, entry);
        }
    }

    pub fn absorb_exported(&mut self, mut other: ProviderRegistry, exported: &[TypeId]) {
        if exported.is_empty() {
            return;
        }
        let allow = exported.iter().copied().collect::<HashSet<_>>();
        // Preserve the source registry's registration order (every entry is tracked in `order`).
        for type_id in std::mem::take(&mut other.order) {
            if allow.contains(&type_id) {
                if let Some(entry) = other.entries.remove(&type_id) {
                    self.insert_entry(type_id, entry);
                }
            }
        }
    }

    /// Like [`Self::absorb_exported`], but clones bindings from `other` so the source registry is kept intact
    /// (used for lazy modules and shared provider cells).
    pub fn absorb_exported_from(&mut self, other: &ProviderRegistry, exported: &[TypeId]) {
        if exported.is_empty() {
            return;
        }
        let allow = exported.iter().copied().collect::<HashSet<_>>();
        for type_id in &other.order {
            if allow.contains(type_id) {
                if let Some(entry) = other.entries.get(type_id) {
                    self.insert_entry(*type_id, entry.clone());
                }
            }
        }
    }

    /// Construct all singleton providers (so their lifecycle hooks can run deterministically),
    /// in registration order.
    pub fn eager_init_singletons(&self) {
        for type_id in &self.order {
            let Some(entry) = self.entries.get(type_id) else {
                continue;
            };
            if entry.scope == ProviderScope::Singleton {
                let _guard = ConstructionGuard::push(*type_id, entry.type_name);
                let _ = entry.instance.get_or_init(|| match &entry.factory {
                    ProviderFactory::InjectableFn(f) => f(self),
                    ProviderFactory::Custom(f) => f(self),
                });
            }
        }
    }

    /// Registration-ordered, dependency-sorted [`TypeId`]s of all **singleton** providers.
    ///
    /// Ordering: construction dependencies recorded by [`Self::get`] are respected first
    /// (dependencies initialize before dependents); ties fall back to registration order.
    /// Providers involved in a hook-time cycle are appended in registration order.
    fn ordered_singletons(&self) -> Vec<TypeId> {
        let singletons: HashSet<TypeId> = self
            .order
            .iter()
            .filter(|id| {
                self.entries
                    .get(id)
                    .is_some_and(|e| e.scope == ProviderScope::Singleton)
            })
            .copied()
            .collect();

        // Edge `dep -> dependent`: dep must be initialized first. Only edges between
        // registered singletons participate (edges to transient/request types or types from
        // other registries are ignored).
        let deps = provider_dep_graph().read().expect("provider dep graph");
        let mut incoming: HashMap<TypeId, usize> =
            singletons.iter().map(|id| (*id, 0usize)).collect();
        let mut adjacency: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
        for (from, targets) in deps.iter() {
            if !singletons.contains(from) {
                continue;
            }
            for to in targets {
                if singletons.contains(to) {
                    adjacency.entry(*from).or_default().push(*to);
                    *incoming.entry(*to).or_insert(0) += 1;
                }
            }
        }
        drop(deps);

        // Kahn's algorithm; among ready nodes pick the earliest registration order for stability.
        use std::cmp::Reverse;
        let position: HashMap<&TypeId, usize> = self
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let mut ready: std::collections::BinaryHeap<Reverse<usize>> = singletons
            .iter()
            .filter(|id| incoming[id] == 0)
            .map(|id| Reverse(position[id]))
            .collect();

        let mut sorted = Vec::with_capacity(singletons.len());
        let mut visited = HashSet::new();
        while let Some(Reverse(pos)) = ready.pop() {
            let id = self.order[pos];
            visited.insert(id);
            sorted.push(id);
            if let Some(dependents) = adjacency.get(&id) {
                for to in dependents {
                    let e = incoming.get_mut(to).expect("edge target tracked");
                    *e -= 1;
                    if *e == 0 && !visited.contains(to) {
                        ready.push(Reverse(position[to]));
                    }
                }
            }
        }

        // Cycle fallback: append anything not reached, in registration order.
        for id in &self.order {
            if singletons.contains(id) && !visited.contains(id) {
                sorted.push(*id);
            }
        }
        sorted
    }

    pub async fn run_on_module_init(&self) {
        for type_id in self.ordered_singletons() {
            if let Some(entry) = self.entries.get(&type_id) {
                (entry.on_module_init)(self).await;
            }
        }
    }

    /// Destroy hooks run in **reverse** initialization order (dependencies torn down after dependents).
    pub async fn run_on_module_destroy(&self) {
        for type_id in self.ordered_singletons().into_iter().rev() {
            if let Some(entry) = self.entries.get(&type_id) {
                (entry.on_module_destroy)(self).await;
            }
        }
    }

    pub async fn run_on_application_bootstrap(&self) {
        for type_id in self.ordered_singletons() {
            if let Some(entry) = self.entries.get(&type_id) {
                (entry.on_application_bootstrap)(self).await;
            }
        }
    }

    /// Shutdown hooks run in **reverse** initialization order (dependencies torn down after dependents).
    pub async fn run_on_application_shutdown(&self) {
        for type_id in self.ordered_singletons().into_iter().rev() {
            if let Some(entry) = self.entries.get(&type_id) {
                (entry.on_application_shutdown)(self).await;
            }
        }
    }
}

impl Clone for ProviderRegistry {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            order: self.order.clone(),
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

/// Application service or provider type constructed through the DI container.
///
/// **`construct` is synchronous.** Perform async I/O in [`Self::on_module_init`] or after you have
/// an `Arc<Self>` from the registry. Lifecycle hooks run for **singleton** providers when the
/// framework drives [`ProviderRegistry::run_on_module_init`] and related methods (see `NestFactory` / `listen`).
///
/// **Scopes:** override [`Self::scope`] via `#[injectable(scope = "...")]`.
///
/// **Docs:** mdBook **Fundamentals** (`docs/src/fundamentals.md`).
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
/// Typical constructors: [`Self::from_module`], [`Self::from_parts`], [`Self::lazy`], or builders
/// such as [`DynamicModuleBuilder`] / [`ConfigurableModuleBuilder`]. Import the resulting value from
/// `#[module(imports = [...])]` when the macro accepts a `DynamicModule` expression.
///
/// **Docs:** mdBook **Fundamentals** (`docs/src/fundamentals.md`).
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

    /// NestJS-style **lazy module**: `M::build()` runs at most once per process; imports clone bindings
    /// so singleton [`ProviderRegistry`] cells stay shared (see [`ProviderRegistry::absorb_exported_from`]).
    pub fn lazy<M: Module + 'static>() -> Self {
        static CELL: std::sync::OnceLock<DynamicModule> = std::sync::OnceLock::new();
        CELL.get_or_init(DynamicModule::from_module::<M>).clone()
    }
}

impl Clone for DynamicModule {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            router: self.router.clone(),
            exports: self.exports.clone(),
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

type RegistryOverrideFn = Box<dyn FnOnce(&mut ProviderRegistry) + Send>;

/// Builds a [`DynamicModule`] from a static module graph, optionally applying provider overrides
/// before controllers are registered (useful for configurable modules and testing-like setups).
pub struct DynamicModuleBuilder<M>
where
    M: Module + ModuleGraph,
{
    overrides: Vec<RegistryOverrideFn>,
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

impl<M> Default for DynamicModuleBuilder<M>
where
    M: Module + ModuleGraph,
{
    fn default() -> Self {
        Self::new()
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
        const { std::cell::RefCell::new(Vec::new()) };
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
        "Circular module dependency detected: {chain}. If intentional, mark the NestJS-style back-edge import with `forward_ref::<T>()` (or `forwardRef` alias in the `#[module]` macro). See the nestrs mdBook chapter **Fundamentals** (`docs/src/fundamentals.md`).",
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
        const { std::cell::RefCell::new(Vec::new()) };
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
                panic!(
                    "Circular provider dependency detected: {chain}. Break the cycle with lazy construction (`register_use_factory`), split types, defer work to `on_module_init`, or a `forward_ref`-style module import for module graphs. See the nestrs mdBook chapter **Fundamentals** (`docs/src/fundamentals.md`)."
                );
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

/// Global construction-dependency graph (`constructor -> dependency`) recorded by
/// [`ProviderRegistry::get`] while a provider factory is running. Used to order lifecycle hooks.
fn provider_dep_graph() -> &'static RwLock<HashMap<TypeId, Vec<TypeId>>> {
    static DEPS: OnceLock<RwLock<HashMap<TypeId, Vec<TypeId>>>> = OnceLock::new();
    DEPS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn record_provider_dependency(from: TypeId, to: TypeId) {
    let mut deps = provider_dep_graph().write().expect("provider dep graph");
    let targets = deps.entry(from).or_default();
    if !targets.contains(&to) {
        targets.push(to);
    }
}

/// Clears the recorded provider dependency graph.
///
/// **Available only with the `test-hooks` feature.** For tests; see `STABILITY.md` in the repo root.
#[cfg(feature = "test-hooks")]
pub fn clear_provider_dependencies_for_tests() {
    provider_dep_graph()
        .write()
        .expect("provider dep graph")
        .clear();
}

type ModuleBuildFn = Box<dyn FnOnce() -> (ProviderRegistry, Router) + Send>;

static MODULE_BUILD_CACHE: OnceLock<RwLock<HashMap<TypeId, Arc<OnceLock<DynamicModule>>>>> =
    OnceLock::new();

fn module_build_cache() -> &'static RwLock<HashMap<TypeId, Arc<OnceLock<DynamicModule>>>> {
    MODULE_BUILD_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Memoizes a [`Module::build`] result **process-wide**, keyed by the module type.
///
/// This makes NestJS-style module-instance sharing the default: when two modules import the same
/// shared module, both importers receive bindings cloned from **one** built instance (shared
/// singleton cells, one route subtree), instead of each importer rebuilding its own copy.
///
/// Route conflicts from duplicate registration and split-singleton bugs are thereby avoided;
/// `forward_ref` back-edges still skip via the existing module build-stack check before this is reached.
///
/// # Arguments
///
/// `build` is the uncached module body (generated by `#[module]`). It runs at most once per
/// process per module type.
#[doc(hidden)]
pub fn __nestrs_memoize_module_build<M: Module + 'static>(
    build: ModuleBuildFn,
) -> (ProviderRegistry, Router) {
    let key = TypeId::of::<M>();
    let entry = Arc::clone(
        module_build_cache()
            .write()
            .expect("module build cache")
            .entry(key)
            .or_insert_with(|| Arc::new(OnceLock::new())),
    );
    // At most one thread builds `M`; concurrent/reentrant callers block on the cell. Reentrant
    // builds (a true cycle) are rejected earlier by the module build-stack circular check inside
    // `build`, matching pre-memoization semantics.
    let dm = entry.get_or_init(|| {
        let (registry, router) = build();
        DynamicModule::from_parts(registry, router, <M as Module>::exports())
    });
    (dm.registry.clone(), dm.router.clone())
}

/// Clears the process-wide module build cache.
///
/// **Available only with the `test-hooks` feature.** For tests that rebuild the same module type
/// expecting fresh instances; see `STABILITY.md` in the repo root.
#[cfg(feature = "test-hooks")]
pub fn clear_module_cache_for_tests() {
    module_build_cache()
        .write()
        .expect("module build cache")
        .clear();
}
