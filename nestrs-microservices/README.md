# nestrs-microservices

**Microservice transports and client proxy** for [nestrs](https://crates.io/crates/nestrs): request/response and fire-and-forget patterns over optional backends (**TCP** stub, **NATS**, **Redis**, **gRPC**, **Kafka**, **MQTT** — feature-gated).

Also re-exports [`nestrs_events::EventBus`](https://crates.io/crates/nestrs-events) and wires `#[on_event]` handlers registered via `#[event_routes]`.

**Docs:** [docs.rs/nestrs-microservices](https://docs.rs/nestrs-microservices) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-microservices = { version = "0.1.3", features = ["nats"] }
# or: features = ["redis"], ["kafka"], ["mqtt"], ["grpc"], etc.
```

From the umbrella crate you can use:

```toml
nestrs = { version = "0.1.3", features = ["microservices", "microservices-nats"] }
```

## Example: `ClientProxy` over a transport

```rust
use nestrs_microservices::{ClientProxy, NatsTransport, NatsTransportOptions};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), nestrs_microservices::TransportError> {
    let transport = Arc::new(NatsTransport::new(NatsTransportOptions::new(
        "nats://127.0.0.1:4222",
    )));

    let proxy = ClientProxy::new(transport);

    #[derive(serde::Serialize)]
    struct Ping {
        msg: &'static str,
    }

    #[derive(serde::Deserialize)]
    struct Pong {
        echo: String,
    }

    let out: Pong = proxy
        .send("app.ping", &Ping { msg: "hi" })
        .await?;

    println!("{}", out.echo);
    Ok(())
}
```

Handlers are implemented on injectable types using `#[micro_routes]` + `#[message_pattern]` / `#[event_pattern]` (see main `nestrs` docs).

## Features

| Feature | Purpose |
|---------|---------|
| `nats` | NATS request/reply + events |
| `redis` | Redis lists / pub-sub style bridge |
| `grpc` | gRPC transport |
| `kafka` | Kafka request topic consumer |
| `mqtt` | MQTT RPC-style topics |
| `microservice-metrics` | Handler metrics hooks |

## License

MIT OR Apache-2.0.
