# nestrs-idempotency

`Idempotency-Key` request deduplication for [nestrs](https://crates.io/crates/nestrs) — NestJS [`@nestjs/idempotency`](https://docs.nestjs.com/reliability/idempotency) analogue.

Safe retries for POSTs and other mutating routes: the first caller runs the handler; a later call with the same key **from the same caller and the same request** replays the completed response. A different method, path, query, or body is **409** and does not return the stored body. Concurrent in-flight keys return **409 Conflict**. The store key includes a hash of `Authorization` and `Cookie`, so another caller cannot read the cached response by reusing the key.

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
| Same caller, same method/path/query/body | Cached status + body returned |
| Same key, different request | **409** (`IDEMPOTENCY_KEY_MISMATCH`), stored body withheld |
| Different `Authorization` or `Cookie` | Separate slot; the other caller's body is not returned |
| Second caller while in-flight | **409** (`IdempotencyBusy`) |
| Missing key when `required` | Rejected (configurable) |

### Options

| Field | Default | Role |
|-------|---------|------|
| `store` | in-memory (24h TTL) | `IdempotencyStore` impl |
| `header_name` | `idempotency-key` | Incoming header |
| `required` | `false` | Fail if header absent |
| `scope` | `None` | Extra prefix (tenant / route group). Credentials are always part of the key. |

`scope` is optional. Caller identity comes from `Authorization` and `Cookie` even when `scope` is unset. Set `scope` when one process serves several tenants that share credentials.

## Surface

- `IdempotencyModule` / `IdempotencyOptions`
- `idempotency_middleware`
- `InMemoryIdempotencyStore` (default)
- `RedisIdempotencyStore` (`cache-redis`)
- `IdempotencyRecord`, `IdempotencyBusy`

## Security notes

- Keys are attacker-chosen. The middleware binds them to `Authorization` and `Cookie` and refuses to replay a different request.
- Replay stores the full response body — avoid caching secrets in error payloads.
- After this change, retries of keys stored by an older build run the handler once (the storage key changed) instead of returning the previous body.
- See [reliability-security](https://github.com/Joshyahweh/nestrs/blob/main/docs/src/reliability-security.md).

## Docs

- Guide: [Idempotency](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/idempotency.mdx)
- rustdoc: <https://docs.rs/nestrs-idempotency/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
