# nestrs-resilience

Composable reliability policies for [nestrs](https://crates.io/crates/nestrs) — the Rust analogue of NestJS [`@nestjs/resilience`](https://docs.nestjs.com/reliability/resilience).

Wrap fallible async work in **retry** (exponential backoff + jitter), **timeout**, **circuit breaker**, **bulkhead**, and **fallback**. Typed `ResilienceError` / `ResilienceEvent` map cleanly to HTTP/RPC status codes and metrics.

## Install

```toml
[dependencies]
nestrs-resilience = "1.7.1"
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["resilience"] }
```

## Register

```rust
use nestrs_resilience::{ResilienceModule, ResilienceOptions};

#[module(
    imports = [ResilienceModule::register(ResilienceOptions::default())],
    providers = [PaymentsService],
)]
struct AppModule;
```

`ResilienceModule::register` exports `ResilienceService` for DI (`Arc<ResilienceService>`).

## Execute under a policy bundle

```rust
use nestrs_resilience::{
    PolicyBundle, ResilienceError, ResilienceService, RetryPolicy, TimeoutPolicy,
};
use std::sync::Arc;
use std::time::Duration;

async fn charge(svc: Arc<ResilienceService>) -> Result<u64, ResilienceError> {
    let policy = PolicyBundle {
        retry: Some(RetryPolicy {
            attempts: 4,
            delay: Duration::from_millis(50),
            factor: 2.0,
            max_delay: Duration::from_secs(2),
            jitter: true,
        }),
        timeout: Some(TimeoutPolicy {
            timeout: Duration::from_secs(3),
        }),
        circuit_name: Some("payments".into()),
        ..Default::default()
    };

    svc.execute("payments.charge", policy, || async {
        // map transport errors to ResilienceError::Other(...)
        Ok(42u64)
    })
    .await
}
```

### `PolicyBundle` fields

| Field | Role |
|-------|------|
| `retry` | Max attempts + exponential backoff; `jitter` multiplies delay by `1…1.2` |
| `timeout` | Per-attempt `tokio::time::timeout` |
| `bulkhead` | Semaphore of max concurrent executions |
| `circuit_name` | Named breaker; opens after `circuit_failure_threshold` failures |
| `fallback` | Optional closure when the primary path fails |

When a field is `None`, `ResilienceOptions` defaults apply.

### Suggested HTTP mapping

| `ResilienceError` | HTTP |
|-------------------|------|
| `Timeout` | **504** Gateway Timeout |
| `CircuitOpen` | **503** + `Retry-After` |
| `BulkheadFull` | **429** / **503** |
| `Other` | **502** / **500** (your choice) |

Prefer stable `errorCode` values on `nestrs::HttpException` when surfacing these at the edge.

## Surface

- `ResilienceModule` / `ResilienceOptions` / `ResilienceService`
- `PolicyBundle`, `RetryPolicy`, `TimeoutPolicy`, `BulkheadPolicy`
- `CircuitBreaker` / `CircuitState`
- `ResilienceError`, `ResilienceEvent`

## Docs

- Guide: [Resilience](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/resilience.mdx)
- Hub: [Reliability](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/reliability.mdx)
- rustdoc: <https://docs.rs/nestrs-resilience/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
