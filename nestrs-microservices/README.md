# nestrs-microservices

**Microservice transports and client proxy** for [nestrs](https://crates.io/crates/nestrs): request/response and fire-and-forget patterns over optional backends (**TCP**, **NATS**, **Redis**, **gRPC**, **Kafka**, **MQTT**, **RabbitMQ** — feature-gated).

Also re-exports [`nestrs_events::EventBus`](https://crates.io/crates/nestrs-events) and wires `#[on_event]` handlers registered via `#[event_routes]`.

**Docs:** [docs.rs/nestrs-microservices](https://docs.rs/nestrs-microservices) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-microservices = { version = "0.2.0", features = ["nats"] }
# or: features = ["redis"], ["kafka"], ["mqtt"], ["rabbitmq"], ["grpc"], etc.
```

From the umbrella crate you can use:

```toml
nestrs = { version = "0.2.0", features = ["microservices", "microservices-nats"] }
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

Per-handler cross-cutting (Nest-like, not identical): **`#[use_micro_interceptors(...)]`**, **`#[use_micro_guards(...)]`**, **`#[use_micro_pipes(...)]`** — types implement **`MicroIncomingInterceptor`**, **`MicroCanActivate`**, **`MicroPipeTransform`** (see crate docs).

**Custom brokers:** implement [`Transport`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/trait.Transport.html) and optionally consume the JSON protocol described in the **`wire`** and **`custom`** modules.

**Wire format stability:** golden JSON fixtures live in **`tests/fixtures/`** and are checked by **`tests/wire_conformance.rs`**. The doc revision constant is [`WIRE_FORMAT_DOC_REVISION`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/constant.WIRE_FORMAT_DOC_REVISION.html) (bump when changing serde shapes).

**gRPC:** JSON payloads match **`wire`** inside protobuf fields; server dispatch uses the same [`wire::dispatch_send`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/fn.dispatch_send.html) / [`dispatch_emit`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/fn.dispatch_emit.html) as other transports. Clients: [`GrpcTransportOptions::new(..).with_request_timeout(..)`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html).

**RabbitMQ:** one work queue (default `nestrs.micro`) carries [`wire::WireRequest`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/struct.WireRequest.html) JSON; request/reply uses a private reply queue name in `reply`. Bootstrap with `NestFactory::create_microservice_rabbitmq` when using the umbrella crate feature **`microservices-rabbitmq`**.

## Features

| Feature | Purpose |
|---------|---------|
| `nats` | NATS request/reply + events |
| `redis` | Redis lists / pub-sub style bridge |
| `grpc` | gRPC transport |
| `kafka` | Kafka request topic consumer |
| `mqtt` | MQTT RPC-style topics |
| `rabbitmq` | AMQP 0-9-1 via `lapin` (work queue + reply queues) |
| `microservice-metrics` | Handler metrics hooks |

## License

MIT OR Apache-2.0.
