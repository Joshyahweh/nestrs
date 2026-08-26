use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

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
        self.entries
            .insert(TypeId::of::<T>(), create_entry_for_injectable::<T>());
    }

    /// NestJS **`useValue`**: register a pre-built singleton without an [`Injectable`] impl.
    pub fn register_use_value<T: Send + Sync + 'static>(&mut self, value: Arc<T>) {
        let preset: Arc<dyn Any + Send + Sync> = value;
        let cell = Arc::new(OnceLock::new());
        let _ = cell.set(preset.clone());
        self.entries.insert(
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
        self.entries.insert(
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

        self.entries.insert(TypeId::of::<T>(), entry);
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

    pub fn get<T>(&self) -> Arc<T>
    where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let entry = self
            .entries
            .get(&type_id)
            .unwrap_or_else(|| panic!("Provider `{}` not registered", std::any::type_name::<T>()));

        let any = self.produce_any(type_id, entry);

        any.downcast::<T>().unwrap_or_else(|_| {
            panic!(
                "Provider downcast failed for `{}`",
                std::any::type_name::<T>()
            )
        })
    }

    /// All registered provider [`TypeId`] keys (NestJS discovery-style introspection).
    pub fn registered_type_ids(&self) -> Vec<TypeId> {
        self.entries.keys().copied().collect()
    }

    /// Human-readable type names for registered providers (debug / tooling).
    pub fn registered_type_names(&self) -> Vec<&'static str> {
        self.entries.values().map(|e| e.type_name).collect()
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

    /// Like [`Self::absorb_exported`], but clones bindings from `other` so the source registry is kept intact
    /// (used for lazy modules and shared provider cells).
    pub fn absorb_exported_from(&mut self, other: &ProviderRegistry, exported: &[TypeId]) {
        if exported.is_empty() {
            return;
        }
        let allow = exported.iter().copied().collect::<HashSet<_>>();
        for (type_id, entry) in &other.entries {
            if allow.contains(type_id) {
                self.entries.insert(*type_id, entry.clone());
            }
        }
    }

    /// Construct all singleton providers (so their lifecycle hooks can run deterministically).
    pub fn eager_init_singletons(&self) {
        for (type_id, entry) in self.entries.iter() {
            if entry.scope == ProviderScope::Singleton {
                let _guard = ConstructionGuard::push(*type_id, entry.type_name);
                let _ = entry.instance.get_or_init(|| match &entry.factory {
                    ProviderFactory::InjectableFn(f) => f(self),
                    ProviderFactory::Custom(f) => f(self),
                });
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

impl Clone for ProviderRegistry {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
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
