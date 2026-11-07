# Reliability, mail, webhooks, workflows & AI

NestRS **1.7** workspace crates and umbrella features for resilience, idempotency, outbox, locks, mail, webhooks, workflows, and LLM agents. This chapter is the mdBook companion to the Mintlify [reliability hub](../mintlify-docs/guides/reliability.mdx).

**Exceptions (documented, not silent):** Express/Fastify adapters; Nest Observe SaaS / Mau; `nestrs-drizzle` until upstream publishes. Open observability uses OTel instrument hooks.

## Install

```toml
[dependencies]
nestrs = { version = "1.7.1", features = [
  "resilience", "idempotency", "outbox", "locks",
  "mail", "webhooks", "workflows", "ai",
] }
```

## Package matrix

| Crate / feature | Notes |
|-----------------|-------|
| `nestrs-resilience` / `resilience` | Retry + jitter, timeout, circuit, bulkhead |
| `nestrs-idempotency` / `idempotency` | `Idempotency-Key` store + middleware |
| `nestrs-outbox` / `outbox` | Enqueue + `tick` relay + DLQ |
| `nestrs-locks` / `locks` | Lease locks; schedule single-runner |
| `nestrs-mail` / `mail` | Escaped templates; in-memory transport |
| `nestrs-webhooks` / `webhooks` | Standard Webhooks; outbox outbound |
| `nestrs-workflows` / `workflows` | Durable steps + approval |
| `nestrs-ai` / `ai` | `FakeModel`, tools, fail-closed objects |
| `otel` | [Observability](observability.md) |

Threat model: [reliability-security.md](reliability-security.md). Roadmap labels: [roadmap-parity.md](roadmap-parity.md). Nest package name mapping: [nestjs-migration.md](nestjs-migration.md).

## Foundation (core)

| Capability | API |
|------------|-----|
| Machine-readable error codes | `HttpException::with_error_code` |
| Schema + validation | `NestrsSchema` + `SchemaValidationPipe` |
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

The richest how-to pages live in Mintlify under **Ecosystem** (`resilience` … `ai`) and **API Reference → Crates**.
