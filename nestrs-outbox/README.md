# nestrs-outbox

Transactional outbox for [nestrs](https://crates.io/crates/nestrs) — NestJS `@nestjs/outbox` analogue.

Enqueue side effects in the same logical unit of work as your writes, then deliver them asynchronously with retries and a dead-letter queue (DLQ). Pair with [`nestrs-webhooks`](https://crates.io/crates/nestrs-webhooks) for signed outbound HTTP.

## Install

```toml
[dependencies]
nestrs-outbox = "1.7.1"
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["outbox"] }
```

## Flow

1. **`OutboxRelay::add(topic, payload)`** inside the same logical transaction as business writes.
2. Background loop calls **`OutboxRelay::tick(&PublishFn)`**.
3. Success → published; failure → retry; after `max_attempts` → **DLQ**.

```rust
use nestrs_outbox::{OutboxModule, OutboxOptions, OutboxRelay, PublishFn};
use serde_json::json;
use std::sync::Arc;

#[module(imports = [OutboxModule::register(OutboxOptions::default())])]
struct AppModule;

async fn after_order(relay: Arc<OutboxRelay>) {
    relay.add("order.created", json!({ "id": 42 })).await;

    let publish: PublishFn = Arc::new(|topic, payload| {
        Box::pin(async move {
            // send to broker / HTTP / queue
            let _ = (topic, payload);
            Ok(())
        })
    });
    relay.tick(&publish).await;
}
```

### Options

| Field | Default | Role |
|-------|---------|------|
| `store` | in-memory | `OutboxStore` |
| `max_attempts` | `10` | Failures before DLQ |
| `poll_interval` | `1s` | Suggested tick cadence |
| `batch_size` | `32` | Messages per `tick` |
| `allow_in_memory` | `true` | Set `false` once a durable store is wired |

## Surface

- `OutboxModule` / `OutboxOptions` / `OutboxRelay`
- `PublishFn`
- `OutboxMessage`, `OutboxStatus`, `DeadLetter`
- `InMemoryOutboxStore` / `OutboxStore` trait

## Docs

- Guide: [Outbox](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/outbox.mdx)
- Hub: [Reliability](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/reliability.mdx)
- rustdoc: <https://docs.rs/nestrs-outbox/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
