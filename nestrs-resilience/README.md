# nestrs-resilience

NestJS [`@nestjs/resilience`](https://docs.nestjs.com/reliability/resilience) analogue for NestRS.

Policies: **retry** (with jitter), **timeout**, **circuit breaker**, **bulkhead**, and **fallback**.

```toml
nestrs-resilience = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["resilience"] }
```

```rust
use nestrs_resilience::{ResilienceModule, ResilienceOptions, ResilienceService, PolicyBundle};

let dm = ResilienceModule::register(ResilienceOptions::default());
// inject ResilienceService and call execute("label", policy, || async { ... })
```

## Docs

- Mintlify: `mintlify-docs/ecosystem/resilience.mdx`
- API ref: `mintlify-docs/api/crates/nestrs-resilience.mdx`
- rustdoc: <https://docs.rs/nestrs-resilience/1.7.0>
- Lab: `cargo run -p lab --bin lab9_nestjs12`
