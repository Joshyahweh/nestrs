# NestJS 12 parity (NestRS 1.7)

NestJS 12 first-party reliability, mail, webhooks, workflows, and AI packages map to NestRS workspace crates and umbrella features. This chapter is the mdBook companion to the Mintlify [NestJS 12 hub](https://docs.nestrs.dev) guides.

**Exceptions (documented, not silent):** Express/Fastify adapters; Nest Observe SaaS / Mau; `nestrs-drizzle` until upstream publishes. Open observability uses OTel instrument hooks instead of Observe SaaS.

## Install

```toml
[dependencies]
nestrs = { version = "1.7.0", features = [
  "resilience", "idempotency", "outbox", "locks",
  "mail", "webhooks", "workflows", "ai",
] }
```

## Package matrix

| NestJS 12 | Crate / feature | Chapter / notes |
|-----------|-----------------|-----------------|
| `@nestjs/resilience` | `nestrs-resilience` / `resilience` | Retry + jitter, timeout, circuit, bulkhead |
| `@nestjs/idempotency` | `nestrs-idempotency` / `idempotency` | `Idempotency-Key` store + middleware |
| `@nestjs/outbox` | `nestrs-outbox` / `outbox` | Enqueue + `tick` relay + DLQ |
| `@nestjs/locks` | `nestrs-locks` / `locks` | Lease locks; schedule single-runner |
| `@nestjs/mail` | `nestrs-mail` / `mail` | Escaped templates; in-memory transport |
| `@nestjs/webhooks` | `nestrs-webhooks` / `webhooks` | Standard Webhooks; outbox outbound |
| `@nestjs/workflows` | `nestrs-workflows` / `workflows` | Durable steps + approval |
| `@nestjs/ai` | `nestrs-ai` / `ai` | `FakeModel`, tools, fail-closed objects |
| Observe SaaS | `otel` + span attrs | [Observability](observability.md) |

Threat model: [nestjs12-security.md](nestjs12-security.md). Roadmap labels: [roadmap-parity.md](roadmap-parity.md).

## Foundation (core)

| Feature | API |
|---------|-----|
| `errorCode` | `HttpException::with_error_code` |
| Standard Schema analogue | `NestrsSchema` + `SchemaValidationPipe` |
| OpenAPI `$ref` auto-link | `OpenApiRouteSpec` schema name fields |
| Route conflicts | `RouteConflictPolicy` |
| Richer `ExecutionContext` | `get_class` / `get_handler` |
| Controller pipes | `controller_pipes` on `#[routes]` |

## Module wiring pattern

```rust
use nestrs::prelude::*;
use std::sync::Arc;

#[injectable]
struct Demo {
    resilience: Arc<nestrs::resilience::ResilienceService>,
}

#[module(
    imports = [
        nestrs::resilience::ResilienceModule::register(
            nestrs::resilience::ResilienceOptions::default()
        ),
    ],
    providers = [Demo],
)]
struct AppModule;
```

`FooModule::register` returns a `DynamicModule` that **exports** service `TypeId`s so importers can inject them.

## Lab smoke

```bash
cargo run -p lab --bin lab9_nestjs12
```

Covers `errorCode`, resilience retries, mail escaping, AI `FakeModel`, locks, outbox, workflows, webhooks, and idempotency races.

## Deep dives (Mintlify)

The richest how-to pages live in Mintlify under **Ecosystem** (`resilience` … `ai`) and **API Reference → Crates**. mdBook keeps this overview plus the security notes and roadmap matrix for offline/book builds.
