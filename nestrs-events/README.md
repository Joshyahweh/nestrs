# nestrs-events

**In-process, async event bus** for [nestrs](https://crates.io/crates/nestrs): subscribe by pattern, emit JSON or typed payloads. Pairs with `#[on_event]` / `#[event_routes]` when you enable **microservices** features on the main crate.

Kept **separate** from `nestrs-microservices` so HTTP-only apps can use domain events without NATS/Kafka/Redis adapters.

**Docs:** [docs.rs/nestrs-events](https://docs.rs/nestrs-events) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-events = "0.3.6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## Example

```rust
use nestrs_events::EventBus;
use serde::Serialize;

#[derive(Serialize)]
struct UserCreated {
    id: u64,
    email: String,
}

#[tokio::main]
async fn main() {
    let bus = EventBus::new();

    bus.subscribe("user.created", |payload: serde_json::Value| async move {
        eprintln!("handler saw: {payload}");
    });

    bus
        .emit(
            "user.created",
            &UserCreated {
                id: 1,
                email: "a@b.com".into(),
            },
        )
        .await;
}
```

`EventBus` implements `nestrs_core::Injectable` so you can register it as a provider in a `#[module]`.

## License

MIT OR Apache-2.0.
