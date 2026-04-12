# nestrs-core

Runtime **traits and types** for [nestrs](https://crates.io/crates/nestrs): dependency injection (`ProviderRegistry`, `Injectable`), **module graph** hooks, **route metadata** (`RouteRegistry`), guards, pipes, and auth strategy extension points.

You rarely depend on this crate alone in application code; the main framework re-exports what you need via `nestrs::prelude::*`. Use **`nestrs-core`** when building **extensions** or **custom modules** without pulling the full HTTP stack.

**Docs:** [docs.rs/nestrs-core](https://docs.rs/nestrs-core) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-core = "0.1.3"
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

## License

MIT OR Apache-2.0.
