# nestrs

NestJS-style **modules, controllers, dependency injection, and HTTP routes** on **[Axum](https://github.com/tokio-rs/axum)** and **Tower**.

**Repository:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs) · **Docs:** [docs.rs/nestrs](https://docs.rs/nestrs)

## Install

```toml
[dependencies]
nestrs = "0.1.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Optional features (enable in `Cargo.toml`): `ws`, `graphql`, `openapi`, `microservices`, `microservices-nats`, `microservices-redis`, `microservices-kafka`, `microservices-mqtt`, `microservices-grpc`, `cache-redis`, `schedule`, `queues`, `otel`.

## Minimal app

```rust
use nestrs::prelude::*;
use std::sync::Arc;

#[injectable]
struct Greeter;

impl Greeter {
    fn hello(&self) -> &'static str {
        "Hello, nestrs!"
    }
}

#[controller(prefix = "/")]
#[routes(state = Greeter)]
impl HelloController {
    #[get("/")]
    async fn hello(State(g): State<Arc<Greeter>>) -> &'static str {
        g.hello()
    }
}

#[module(controllers = [HelloController], providers = [Greeter])]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>().listen_graceful(3000).await;
}
```

## Ecosystem crates

| Crate | Role |
|--------|------|
| [`nestrs-core`](https://crates.io/crates/nestrs-core) | DI container, module traits, route registry |
| [`nestrs-macros`](https://crates.io/crates/nestrs-macros) | `#[module]`, `#[controller]`, `#[get]`, … |
| [`nestrs-events`](https://crates.io/crates/nestrs-events) | In-process event bus |
| [`nestrs-cqrs`](https://crates.io/crates/nestrs-cqrs) | Command/query buses |
| [`nestrs-ws`](https://crates.io/crates/nestrs-ws) | WebSocket gateway helpers |
| [`nestrs-graphql`](https://crates.io/crates/nestrs-graphql) | async-graphql router |
| [`nestrs-openapi`](https://crates.io/crates/nestrs-openapi) | OpenAPI + Swagger UI |
| [`nestrs-microservices`](https://crates.io/crates/nestrs-microservices) | Transports (NATS, Redis, Kafka, …) |
| [`nestrs-prisma`](https://crates.io/crates/nestrs-prisma) | Prisma-oriented DB module |
| [`nestrs-scaffold`](https://crates.io/crates/nestrs-scaffold) | CLI (`cargo install nestrs-scaffold`, binary `nestrs`) |

See the [workspace README](https://github.com/Joshyahweh/nestrs/blob/main/README.md) for badges, benchmarks, and contribution guides.

## License

Licensed under **MIT OR Apache-2.0**, matching the workspace.
