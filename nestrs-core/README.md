# nestrs-core

Runtime **traits and types** for [nestrs](https://crates.io/crates/nestrs): dependency injection (`ProviderRegistry`, `Injectable`), **module graph** hooks, **route metadata** (`RouteRegistry`), guards, pipes, and auth strategy extension points.

You rarely depend on this crate alone in application code; the main framework re-exports what you need via `nestrs::prelude::*`. Use **`nestrs-core`** when building **extensions** or **custom modules** without pulling the full HTTP stack.

**Docs:** [docs.rs/nestrs-core](https://docs.rs/nestrs-core) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-core = "0.3.2"
axum = "0.7"
async-trait = "0.1"
```

## Example: custom injectable

```rust
use async_trait::async_trait;
use nestrs_core::{Injectable, ProviderRegistry};
use std::sync::Arc;

pub struct Clock;

#[async_trait]
impl Injectable for Clock {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Clock)
    }
}
```

## Example: register in a `ProviderRegistry`

```rust
use nestrs_core::ProviderRegistry;

let mut registry = ProviderRegistry::new();
registry.register::<Clock>();
let clock = registry.get::<Clock>();
```

`Module` / `ModuleGraph` implementations in higher-level crates call `register_providers` and `register_controllers` using these primitives.

## NestJS fundamentals parity (custom providers, module ref, discovery, lazy modules)

Full narrative (scopes, lifecycle, dynamic modules, circular deps): **[Fundamentals](https://github.com/Joshyahweh/nestrs/blob/main/docs/src/fundamentals.md)** in the repo mdBook source.

- **Custom providers**: `register_use_value` (Nest `useValue`), `register_use_factory` (Nest `useFactory`, any [`ProviderScope`]; factory is **sync**—use `on_module_init` or async module options for I/O), `register_use_class` (alias of `register` / Nest `useClass` for normal injectables).
- **Module reference**: `ModuleRef` — resolve providers from an `Arc<ProviderRegistry>` after the graph is built (`NestApplication::module_ref` in the main crate).
- **Discovery**: `DiscoveryService` lists registered provider `TypeId`s / type names and `RouteRegistry` HTTP routes.
- **Execution context**: `ExecutionContext` + `HostType` mirror Nest’s `ArgumentsHost` basics for HTTP (install middleware from the `nestrs` crate).
- **Platform hook**: `HttpServerEngine` / `AxumHttpEngine` documents the Axum-first “platform” story.
- **Lazy modules**: `DynamicModule::lazy::<M>()` or `#[module(imports = [lazy_module::<M>()])]` / `lazy::<M>()` in the `nestrs` proc macro.

## Cargo features

- **`test-hooks`** — exposes `RouteRegistry::clear_for_tests()` and `MetadataRegistry::clear_for_tests()` so integration tests can reset process-global registries. **Do not enable in production.** See workspace **`STABILITY.md`**.

## License

MIT OR Apache-2.0.
