# nestrs-idempotency

`Idempotency-Key` request deduplication for [nestrs](https://crates.io/crates/nestrs) — NestJS [`@nestjs/idempotency`](https://docs.nestjs.com/reliability/idempotency) analogue.

Safe retries for POSTs and other mutating routes: the first caller runs the handler; later callers with the same key **replay** the completed response. Concurrent in-flight keys return **409 Conflict**.

## Install

```toml
[dependencies]
nestrs-idempotency = "1.7.1"
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["idempotency"] }
# Redis-backed store (optional):
# nestrs-idempotency = { version = "1.7.1", features = ["cache-redis"] }
```

## Register

```rust
use nestrs_idempotency::{IdempotencyModule, IdempotencyOptions};
use std::time::Duration;

#[module(
    imports = [IdempotencyModule::register(
        IdempotencyOptions::in_memory(Duration::from_secs(24 * 3600))
    )],
)]
struct AppModule;
```

Mount `idempotency_middleware` on mutating routes (or globally) so the store sees the raw response body.

## Behaviour

| Situation | Result |
|-----------|--------|
| First request with key | Handler runs; response stored |
| Replay of completed key | Cached status + body returned |
| Second caller while in-flight | **409** (`IdempotencyBusy`) |
| Missing key when `required` | Rejected (configurable) |

### Options

| Field | Default | Role |
|-------|---------|------|
| `store` | in-memory (24h TTL) | `IdempotencyStore` impl |
| `header_name` | `idempotency-key` | Incoming header |
| `required` | `false` | Fail if header absent |
| `scope` | `None` | Prefix keys (e.g. tenant / user id) |

Use `scope` so tenants cannot collide on the same client-supplied key.

## Surface

- `IdempotencyModule` / `IdempotencyOptions`
- `idempotency_middleware`
- `InMemoryIdempotencyStore` (default)
- `RedisIdempotencyStore` (`cache-redis`)
- `IdempotencyRecord`, `IdempotencyBusy`

## Security notes

- Keys are attacker-chosen; always combine with auth + `scope`.
- Replay stores the full response body — avoid caching secrets in error payloads.
- See [reliability-security](https://github.com/Joshyahweh/nestrs/blob/main/docs/src/reliability-security.md).

## Docs

- Guide: [Idempotency](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/idempotency.mdx)
- rustdoc: <https://docs.rs/nestrs-idempotency/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
