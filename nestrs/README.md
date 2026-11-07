# nestrs

NestJS-style **modules, controllers, dependency injection, and HTTP routes** on **[Axum](https://github.com/tokio-rs/axum)** and **Tower**.

**Repository:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs) · **Docs:** [docs.rs/nestrs](https://docs.rs/nestrs) · **Guides:** [mintlify-docs](https://github.com/Joshyahweh/nestrs/tree/main/mintlify-docs)

## Install

```toml
[dependencies]
nestrs = "1.7.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### Popular features

| Feature | Pulls in | Role |
|---------|----------|------|
| `resilience` | `nestrs-resilience` | Retry, timeout, circuit, bulkhead |
| `idempotency` | `nestrs-idempotency` | `Idempotency-Key` dedupe |
| `outbox` | `nestrs-outbox` | Transactional outbox + DLQ |
| `locks` | `nestrs-locks` | Lease locks / single-runner cron |
| `mail` | `nestrs-mail` | Templated email |
| `webhooks` | `nestrs-webhooks` (+ `outbox`) | Standard Webhooks sign/verify |
| `workflows` | `nestrs-workflows` (+ `outbox`, `locks`) | Durable multi-step flows |
| `ai` | `nestrs-ai` (+ `resilience`) | LLM / agent module |
| `ws` | `nestrs-ws` | WebSocket gateway |
| `graphql` | `nestrs-graphql` | async-graphql |
| `openapi` | `nestrs-openapi` | OpenAPI + Swagger UI |
| `throttler` | `nestrs-throttle` | Per-route rate limits |
| `mongo` | `nestrs-mongodb` | MongoDB module |
| `http-client` | `nestrs-http` | Outbound HTTP |
| `schedule` | — | Cron / intervals |
| `cache-redis` | — | Redis cache backend |
| `otel` | — | OpenTelemetry |

Also available: `microservices` (+ NATS / Redis / Kafka / MQTT / RabbitMQ / gRPC), `queues`, `cookies`, `session`, `csrf`, `throttler-redis`, and more — see [`Cargo.toml`](https://github.com/Joshyahweh/nestrs/blob/main/nestrs/Cargo.toml).

Reliability hub: [guides/reliability.mdx](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/reliability.mdx). Smoke lab: `cargo run -p lab --bin lab9_nestjs12`.

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

### Core

| Crate | Role |
|--------|------|
| [`nestrs-core`](https://crates.io/crates/nestrs-core) | DI container, module traits, route registry |
| [`nestrs-macros`](https://crates.io/crates/nestrs-macros) | `#[module]`, `#[controller]`, `#[get]`, … |
| [`nestrs-scaffold`](https://crates.io/crates/nestrs-scaffold) | CLI (`cargo install nestrs-scaffold`, binary `nestrs-cli`) |

### Reliability & agents (1.7+)

| Crate | Umbrella feature | Role |
|--------|------------------|------|
| [`nestrs-resilience`](https://crates.io/crates/nestrs-resilience) | `resilience` | Retry, timeout, circuit, bulkhead, fallback |
| [`nestrs-idempotency`](https://crates.io/crates/nestrs-idempotency) | `idempotency` | `Idempotency-Key` store + middleware |
| [`nestrs-outbox`](https://crates.io/crates/nestrs-outbox) | `outbox` | Outbox relay, retries, DLQ |
| [`nestrs-locks`](https://crates.io/crates/nestrs-locks) | `locks` | Lease locks for single-runner work |
| [`nestrs-mail`](https://crates.io/crates/nestrs-mail) | `mail` | Escaped templates + transports |
| [`nestrs-webhooks`](https://crates.io/crates/nestrs-webhooks) | `webhooks` | Standard Webhooks sign / verify |
| [`nestrs-workflows`](https://crates.io/crates/nestrs-workflows) | `workflows` | Durable steps + human approval |
| [`nestrs-ai`](https://crates.io/crates/nestrs-ai) | `ai` | Named models, generate text/object, tools |

### HTTP, data & messaging

| Crate | Role |
|--------|------|
| [`nestrs-events`](https://crates.io/crates/nestrs-events) | In-process event bus |
| [`nestrs-cqrs`](https://crates.io/crates/nestrs-cqrs) | Command/query buses |
| [`nestrs-ws`](https://crates.io/crates/nestrs-ws) | WebSocket gateway helpers |
| [`nestrs-graphql`](https://crates.io/crates/nestrs-graphql) | async-graphql router |
| [`nestrs-openapi`](https://crates.io/crates/nestrs-openapi) | OpenAPI + Swagger UI |
| [`nestrs-microservices`](https://crates.io/crates/nestrs-microservices) | Transports (NATS, Redis, Kafka, MQTT, RabbitMQ, …) |
| [`nestrs-prisma`](https://crates.io/crates/nestrs-prisma) | Prisma-oriented DB module |
| [`nestrs-mongodb`](https://crates.io/crates/nestrs-mongodb) | MongoDB adapter (`MongoModule`, `MongoRepository`) |
| [`nestrs-http`](https://crates.io/crates/nestrs-http) | Outbound HTTP client |
| [`nestrs-throttle`](https://crates.io/crates/nestrs-throttle) | Per-route rate limiting |
| [`nestrs-health`](https://crates.io/crates/nestrs-health) | Health indicators |
| [`nestrs-security`](https://crates.io/crates/nestrs-security) | Auth helpers, helmet, CSRF |
| [`nestrs-mcp`](https://crates.io/crates/nestrs-mcp) | Model Context Protocol server |

See the [workspace README](https://github.com/Joshyahweh/nestrs/blob/main/README.md) for badges, benchmarks, and contribution guides.

## License

Licensed under **MIT OR Apache-2.0**, matching the workspace.
