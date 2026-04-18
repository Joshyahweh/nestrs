# Fundamentals (Nest-style DI and modules)

This chapter aligns **nestrs** with Nest’s **Fundamentals** docs: dynamic modules, injection scopes, lifecycle hooks, [`ModuleRef`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ModuleRef.html), [`DiscoveryService`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.DiscoveryService.html), custom providers, and **circular dependency** behavior. The **API is Rust** (traits, `Arc`, sync `construct`); behavior is **analogous**, not identical, to Nest’s runtime metadata.

## Injection scopes

[`ProviderScope`](https://docs.rs/nestrs-core/latest/nestrs_core/enum.ProviderScope.html) controls how long a resolved instance lives:

| Scope | Nest analogue | Behavior in nestrs |
|-------|----------------|-------------------|
| **`Singleton`** | `DEFAULT` | One shared instance per application container (default for `#[injectable]`). |
| **`Transient`** | `TRANSIENT` | A **new** instance on every `registry.get::<T>()` (and per injection site in generated code). |
| **`Request`** | `REQUEST` | One instance **per HTTP request** when request-scoped middleware is enabled (see below). |

Set scope on a type with **`#[injectable(scope = "singleton" | "transient" | "request")]`** (default `singleton`). The generated [`Injectable::scope()`](https://docs.rs/nestrs-core/latest/nestrs_core/trait.Injectable.html#method.scope) drives registration.

### Request scope and `RequestScoped`

1. Call **`NestFactory::...use_request_scope()`** on your app so each request gets a task-local cache and registry access.
2. Mark providers with **`#[injectable(scope = "request")]`**.
3. In handlers, use the **`RequestScoped<T>`** extractor to resolve `T` for that request.

Without **`use_request_scope()`**, resolving a `Request`-scoped provider outside a request context **panics** (there is no cache).

## Lifecycle hooks

[`Injectable`](https://docs.rs/nestrs-core/latest/nestrs_core/trait.Injectable.html) provides **async** hooks (default no-ops):

- **`on_module_init`**
- **`on_module_destroy`**
- **`on_application_bootstrap`**
- **`on_application_shutdown`**

**When they run (singletons):** After the module graph is built, **`NestFactory`** / **`listen`** / **`listen_graceful`** drive the registry in order:

1. **`eager_init_singletons()`** — constructs singletons (and runs **provider** construction guards).
2. **`run_on_module_init().await`**
3. **`run_on_application_bootstrap().await`**
4. On shutdown: **`run_on_application_shutdown().await`** then **`run_on_module_destroy().await`**

**Note:** Hooks are wired for **singleton** entries in the current implementation. **Transient** types are constructed on demand and do **not** receive this global hook sequence unless you call into them explicitly.

**Async initialization of a service:** There is no Nest **`onModuleInit` returning Promise** split from `construct`: **`construct` is synchronous**. Use:

- **`on_module_init`** to run async setup after the instance exists, or  
- A **`register_use_factory`** that returns an `Arc<T>` whose `T` was built synchronously but defers I/O to **`on_module_init`**, or  
- **`ConfigurableModuleBuilder::for_root_async`** (below) to load **module options** before the graph is built.

## `ModuleRef`

[`ModuleRef`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ModuleRef.html) is a thin handle to the root [`ProviderRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ProviderRegistry.html) after composition.

```rust
let app = NestFactory::create::<AppModule>().into_router();
// Or keep `NestApplication` before `into_router`:
// let mref = app.module_ref();
// let svc = mref.get::<MyService>();
```

Use it for **dynamic** resolution (plugins, conditional code) where static typing of every constructor is awkward. It is the same registry backing **`State<Arc<...>>`** injection in controllers.

## `DiscoveryService`

[`DiscoveryService::new(module_ref)`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.DiscoveryService.html) exposes:

- **`get_providers()`** — [`TypeId`](https://doc.rust-lang.org/std/any/struct.TypeId.html) keys of registered providers  
- **`get_provider_type_names()`** — debug strings  
- **`get_routes()`** — HTTP routes from the global [`RouteRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.RouteRegistry.html) (OpenAPI / diagnostics)

Nest’s reflection over class metadata has no direct equivalent; discovery is **type-id / route list** oriented.

After you have a [`ModuleRef`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ModuleRef.html) (from `NestFactory::create` before `into_router`, or by wrapping the root registry in tests):

```rust
use nestrs::core::{DiscoveryService, ModuleRef};
// let mref: ModuleRef = app.module_ref();

let discovery = DiscoveryService::new(mref);
let _provider_ids = discovery.get_providers();
let _route_specs = discovery.get_routes(); // OpenAPI / diagnostics
```

## Dynamic modules

### `DynamicModule`

[`DynamicModule`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.DynamicModule.html) bundles a **`ProviderRegistry`** fragment, a **`Router`** subtree, and **`exports: Vec<TypeId>`** for re-exports. Typical sources:

- **`DynamicModule::from_module::<M>()`** — run a static module’s **`Module::build()`**  
- **`DynamicModule::from_parts(...)`** — crates like queues / i18n build registries by hand  
- **`DynamicModule::lazy::<M>()`** — **once per process** `M::build()` with shared singleton cells when re-imported  

Import dynamic modules from **`#[module(imports = [...])]`** using expressions that evaluate to a **`DynamicModule`** (see feature crates).

### `DynamicModuleBuilder` and overrides

[`DynamicModuleBuilder::<M>::new()`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.DynamicModuleBuilder.html) runs **`M::register_providers`**, applies **`override_provider`** closures, then **`register_controllers`**. Used for tests and **configurable** modules.

### `ConfigurableModuleBuilder` — `for_root` / `for_root_async`

Nest-style **synchronous** options:

```rust
ConfigurableModuleBuilder::for_root::<MyModule>(options)
```

**Async options** (e.g. load secrets from remote config) before building the graph:

```rust
use nestrs::core::{ConfigurableModuleBuilder, DynamicModule};

async fn build_module() -> DynamicModule {
    ConfigurableModuleBuilder::<MyOptions>::for_root_async::<AppModule, _, _>(|| async {
        load_my_options().await
    })
    .await
}
```

This **awaits** your future, then injects **`ModuleOptions<MyOptions, AppModule>`** so `AppModule` can read options from the registry. It does **not** make individual **`Injectable::construct`** async.

## Custom providers (Nest `useValue` / `useFactory` / `useClass`)

On [`ProviderRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ProviderRegistry.html) (or via module graph absorption):

| Method | Nest analogue | Notes |
|--------|----------------|--------|
| **`register_use_value::<T>(Arc<T>)`** | `useValue` | Pre-built singleton; no lifecycle hooks unless you wrap a type that implements them separately. |
| **`register_use_factory::<T>(scope, \|registry\| Arc<T>)`** | `useFactory` | **Sync** closure; use `registry.get()` for dependencies. Supports **any** [`ProviderScope`]. |
| **`register_use_class::<T>()`** | `useClass` | Same as **`register::<T>()`** for normal `#[injectable]` types. |

**“Async factory” for a provider:** Nest’s async factory is usually modeled by:

1. **`for_root_async`** for **module-level** config, then normal `construct` reads `ModuleOptions`, or  
2. **`register_use_factory`** returning `Arc<T>` where `T::construct` is cheap and **`T::on_module_init`** performs I/O.

Avoid **`block_on`** inside `construct` — it can deadlock the async runtime.

## Circular dependencies

### Module import graph

**Symptom:** panic **`Circular module dependency detected: A -> B -> ... -> A`**.

**Cause:** Static **`#[module(imports = [A, B, ...])]`** expanded to a cycle during **`Module::build`**.

**Fix (Nest `forwardRef`):** Mark the **back-edge** import with **`forward_ref::<ThatModule>()`** or **`forwardRef::<ThatModule>()`** in the `imports = [...]` list so that module is not entered recursively while already on the build stack.

**Alternative:** Restructure modules (shared types in a third module, feature modules, or **`DynamicModule::lazy`** so one side initializes once).

### Provider construction graph

**Symptom:** panic **`Circular provider dependency detected: TypeA -> TypeB -> ...`**.

**Cause:** **`construct`** or a **factory** calls **`registry.get::<U>()`** while **`U`** (transitively) needs the type currently being constructed.

**Fixes:**

- **Split** types or introduce an interface/trait registered once.  
- **Defer** work to **`on_module_init`** so `construct` only wires `Arc`s without pulling the cycle.  
- **`register_use_factory`** with a factory that breaks the eager cycle (e.g. one side resolves lazily on first use — still avoid re-entrant `get` of the same `TypeId` during init).  
- **`ModuleRef`** / **`get`** only **after** the app is built, not from inside `construct` of a singleton in the cycle.

There is **no** `forwardRef` for individual classes in Rust DI — cycles must be broken in **code structure** or **initialization order**.

## Worked recipe: configurable module options (`for_root` / `for_root_async`)

Nest’s **`ConfigModule.forRoot`** pattern maps to **`ConfigurableModuleBuilder`**, which registers a **[`ModuleOptions<O, M>`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ModuleOptions.html)** provider your injectables can depend on.

**Synchronous options** (constants, env parsed before `main`):

```rust
use nestrs::prelude::*;

#[derive(Clone)]
struct ApiOptions {
    base_url: String,
}

#[injectable]
struct HttpClientConfig {
    opts: std::sync::Arc<ModuleOptions<ApiOptions, ConfigModule>>,
}

impl HttpClientConfig {
    fn base_url(&self) -> &str {
        &self.opts.get().base_url
    }
}

#[module(providers = [HttpClientConfig], exports = [HttpClientConfig])]
struct ConfigModule;

fn build() -> DynamicModule {
    ConfigurableModuleBuilder::<ApiOptions>::for_root::<ConfigModule>(ApiOptions {
        base_url: "https://api.example.com".into(),
    })
}
```

**Async options** (remote secrets, KMS, slow disk): await **`for_root_async`** before composing the root module (the future runs **once**, before singleton construction):

```rust
async fn build_from_vault() -> DynamicModule {
    ConfigurableModuleBuilder::<ApiOptions>::for_root_async::<ConfigModule, _, _>(|| async {
        // Example: await a secrets client here, then fill `ApiOptions`.
        ApiOptions {
            base_url: std::env::var("API_BASE_URL").unwrap_or_else(|_| "https://api.example.com".into()),
        }
    })
    .await
}
```

`Injectable::construct` remains **synchronous**; use **`on_module_init`** inside `HttpClientConfig` (or a dedicated bootstrap service) for I/O that must happen **after** the type exists.

## Worked recipe: `forward_ref` for cyclic **module** imports

When **`AModule` imports `BModule`** and **`BModule` imports `AModule`**, the macro expansion detects a **cycle** and panics unless one import is the intentional **back-edge**. Mark it with **`forward_ref::<TheOtherModule>()`** (alias **`forwardRef`**):

```rust
use nestrs::prelude::*;

// … controllers / providers for A and B omitted …

#[module(
    imports = [BForwardModule],
    controllers = [AController],
    providers = [AState],
)]
struct AForwardModule;

#[module(
    imports = [forward_ref::<AForwardModule>()],
    controllers = [BController],
    providers = [BState],
)]
struct BForwardModule;
```

See **`nestrs/tests/forward_ref_modules.rs`** in the repository for a compiling router test. This fixes **module graph** cycles only; **provider** cycles still need factories, `on_module_init`, or structural refactors (above).

## Worked recipe: `register_use_factory` (Nest `useFactory`)

Use a **sync** factory when construction order must be explicit or you need to close a provider cycle without eager `get()` during another type’s `construct`. The closure receives the root [`ProviderRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ProviderRegistry.html) so dependencies resolve the same way as in `construct`:

```rust
use nestrs::core::{ProviderRegistry, ProviderScope};
use std::sync::Arc;

// `HeavyService` / `LightService`: your own `Send + Sync + 'static` types.
fn register_heavy(registry: &mut ProviderRegistry) {
    registry.register_use_factory::<HeavyService>(ProviderScope::Singleton, |reg| {
        let light = reg.get::<LightService>();
        Arc::new(HeavyService::new(light))
    });
}
```

Keep the closure **non-async**; defer I/O to **`on_module_init`** on `HeavyService` or to **`for_root_async`** for module-level configuration.

## Integration tests and global registries

`RouteRegistry` and `MetadataRegistry` are **process-global** ([ADR-0002](adrs/0002-global-registries.md)). In **`#[tokio::test]`** suites that build multiple apps, enable **`nestrs`’s `test-hooks`** feature and clear registries between tests (see `nestrs/tests/common/mod.rs` **`RegistryResetGuard`**). **Never** enable `test-hooks` in production binaries—see **`STABILITY.md`** at the repository root.

## Related

- [First steps](first-steps.md) — minimal app  
- [Custom decorators](custom-decorators.md) — metadata vs Nest decorators  
- [Roadmap parity](roadmap-parity.md) — feature matrix  
- [API cookbook](appendix-api-cookbook.md) — `module_ref`, `set_global_prefix`, `use_request_scope`, and other `NestApplication` helpers  
- Rustdoc: [`nestrs_core`](https://docs.rs/nestrs-core), [`nestrs`](https://docs.rs/nestrs)  
