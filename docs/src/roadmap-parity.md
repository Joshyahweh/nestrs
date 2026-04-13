# Roadmap parity checklist

This page tracks Nest-style capabilities and where they live in `nestrs`.

## Overview (documentation vs Nest)

| Area | Status | Notes |
|------|--------|--------|
| Introduction / first steps | **Partial** | [First steps](first-steps.md) + [Introduction](index.md) (repo README) + examples; **not** Nest’s full onboarding (tutorial depth, videos, in-browser playground, TS-first pedagogy) |
| Custom decorators | **Partial** | [Custom decorators](custom-decorators.md): `#[set_metadata]`, `#[roles]`, [`MetadataRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.MetadataRegistry.html), `HandlerKey`, Axum extractors / `#[param::…]` — **not** TypeScript `reflect-metadata` or `createParamDecorator` runtime |
| Stability / semver / test hooks | **Doc** | Workspace [`STABILITY.md`](../../STABILITY.md): semver for **`nestrs`**, **`nestrs-core`**, macros; public vs `#[doc(hidden)]`; process-global registries; optional **`test-hooks`** feature |
| Fundamentals (dynamic modules, scopes, lifecycle, `ModuleRef`, discovery, factories, cycles) | **Doc** | [Fundamentals](fundamentals.md) + rustdoc on **`nestrs-core`**; Nest parity is behavioral, not reflection-based |

---

| Area | Status | Notes |
|------|--------|--------|
| URI / header / `Accept` versioning | **Done** | `enable_uri_versioning`, `enable_header_versioning`, `enable_media_type_versioning`, `NestApiVersion` |
| Host / subdomain routes | **Done** | `#[controller(host = "...")]` + host middleware on controller routers |
| Redis rate limiting | **Done** | `RateLimitOptions::redis(...)` with **`cache-redis`** |
| Kafka / MQTT / RabbitMQ transports | **Done** (features) | `rskafka` / `rumqttc` / `lapin` (work queue + reply queues); TLS + SASL (Kafka), username/password + TLS (MQTT); optional **`microservices-metrics`** (`nestrs` feature); **`microservices-rabbitmq`** on umbrella; shared JSON **`wire`** contract + golden tests in `nestrs-microservices`; **gRPC** (`microservices-grpc`) uses same JSON inside protobuf — [GraphQL, WS & micro DX](graphql-ws-micro-dx.md) |
| Microservices custom transporters | **Doc + API** | Implement [`Transport`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/trait.Transport.html); public [`wire`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/index.html) + `custom` module for JSON protocol |
| Microservices cross-cutting | **Partial** | `#[use_micro_guards]` / `#[use_micro_pipes]` / `#[use_micro_interceptors]` on `#[micro_routes]` handlers (`MicroCanActivate`, `MicroPipeTransform`, `MicroIncomingInterceptor`); **no** exception-filter pipeline — [`TransportError`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.TransportError.html); parity table in [`MICROSERVICES.md`](../../MICROSERVICES.md) + [GraphQL, WS & micro DX](graphql-ws-micro-dx.md) |
| Broker readiness | **Partial** | `RedisBrokerHealth` / `NatsBrokerHealth`; Kafka `kafka_cluster_reachable` / `kafka_cluster_reachable_with`; topic retention via cluster ops (not client `create_topic` attrs) |
| CQRS | **Partial** | `nestrs-cqrs`: commands/queries/events-style building blocks; **sagas** are **`Saga` / `SagaDefinition` traits** (compile-time shape), not Nest’s orchestration runtime + docs surface |
| Event bus crate | **Done** | `nestrs-events` (`EventBus`) |
| Prisma / SQL connectivity | **Done** | **`sqlx`**: `ping()`, `query_scalar()`, `query_all_as`, `execute`; models in `schema.prisma` + Rust `FromRow` or generated client (`cargo prisma generate`) — see `nestrs-prisma/README.md` |
| GraphQL (async-graphql + Axum) | **Partial** | **Core (done):** `graphql_router` / `graphql_router_with_options` (`GraphQlHttpOptions`), Playground + batch POST, `with_default_limits` + `with_production_graphql_limits` + `Analyzer`, `export_schema_sdl` / `SDLExportOptions`. **Ecosystem (not Nest parity):** federation, plugins/extensions, CLI, mapped types, field guards → use async-graphql + Apollo Router / GraphOS / codegen crates; see `nestrs-graphql` crate docs. **DX:** [GraphQL, WS & micro DX](graphql-ws-micro-dx.md). |
| WebSockets cross-cutting | **Partial** | `WsHandshake`; `#[use_ws_guards]` / `#[use_ws_pipes]` / `#[use_ws_interceptors]` on `#[ws_routes]`; **errors** are `error` frames ([`WS_ERROR_EVENT`](https://docs.rs/nestrs-ws/latest/nestrs_ws/constant.WS_ERROR_EVENT.html)), not HTTP `ExceptionFilter` — documented in `nestrs-ws` + [GraphQL, WS & micro DX](graphql-ws-micro-dx.md). |
| WebSocket adapters | **Doc** | Axum `WebSocketUpgrade` (browser `WebSocket` / RFC 6455). Nest’s Socket.IO vs `ws` split maps to **event-shaped JSON here** vs **Socket.IO** → use [`socketioxide`](https://crates.io/crates/socketioxide); see `nestrs-ws::adapters`. |
| DTO validators (Nest-like) | **Partial** | `#[dto]` maps `Min`/`Max`/`IsUrl`/`ValidateNested`/… to `validator` |
| RFC 9457 problems | **Done** | `ProblemDetails` |
| OTel log correlation | **Doc** | Spans + `tracing`; OTLP logs via collector until stable exporters |
| Security: auth / roles | **Done** | `CanActivate`, `AuthStrategy`, `#[roles]`, `XRoleMetadataGuard`, `AuthStrategyGuard`, `BearerToken` / `parse_authorization_bearer` (no Passport bundle) |
| Security: Helmet-like headers | **Done** | `SecurityHeaders` + `helmet_like()` + per-header builders; Tower `SetResponseHeaderLayer` |
| Security: CSRF | **Done** (opt-in) | Feature **`csrf`**: double-submit vs cookie + header; requires **`cookies`** + `use_cookies()` |
| Security: crypto / hashing | **No** (by design) | Apps use `argon2` / `ring` / etc.; documented in `SECURITY.md` |
| CLI (`nestrs` / `nestrs-scaffold`) | **Partial** | `nestrs new` + `nestrs generate` (resources, DTOs, guards, …); see [CLI](cli.md) and `nestrs-cli/README.md`; **not** Nest CLI lifecycle/plugin parity |
| CLI workspaces | **No** | Use Cargo workspaces + `cargo new` per crate; no Nest-style monorepo generator |
| CLI libraries | **No** | Use `cargo new --lib`, workspace members, crates.io/path deps; no Nest publishable library package flow |
| CLI scripts | **No** | Use `cargo run --bin`, `cargo-make`, `just`, Make, shell — no `package.json` scripts analogue |
| OpenAPI / Swagger | **Partial** | `nestrs-openapi`: paths from `RouteRegistry`, Swagger UI, optional `servers` / `components` / `security` / document `tags`, path-inferred `summary` / `tags`, per-route **`#[openapi(...)]`**; **schema generation** via manual `components`, **utoipa** / **okapi** merge — [OpenAPI & HTTP](openapi-http.md); **optional** `infer_route_security_from_roles` (metadata **`#[roles]`** → operation `security`); **not** Nest `@ApiProperty` reflection or every Swagger plugin subsection |
| CRUD / resource generator | **Partial** | **`nestrs generate`** scaffolds (resources, DTOs, guards, …) via CLI — same *idea* as `nest g resource`, **not** identical flags, plugins, or lifecycle |
| Static file serving | **Partial** | **No** dedicated Nest-style static module or recipe; serve files with **Tower** / **Axum** (`ServeDir`, `tower_http::services::fs`, custom layers) — documented as ecosystem pattern, not a first-class `nestrs` feature |

## Recipes & other sidebar (typical gaps)

Nest’s docs sidebar mixes **framework features**, **recipes**, and **ecosystem** pointers. Below is how that maps to **nestrs**: **Partial** = something exists but is not Nest-parity; **No / external** = use another crate, tool, or host docs — not a goal for the core framework.

### Generators & CLI-shaped workflows

| Topic | Status | Notes |
|-------|--------|--------|
| CRUD / resource scaffolding | **Partial** | CLI (`nestrs-scaffold`): `nestrs generate` — see [CLI](cli.md); not `nest g resource` parity |
| Workspaces / monorepo generator | **No** | Cargo workspaces + `cargo new`; see CLI row in table above |
| Library packages / publish flow | **No** | Standard Rust crates + crates.io; see CLI row above |
| `package.json`-style scripts | **No** | `cargo run --bin`, `just`, `cargo-make`, Make, CI — no Nest scripts layer |

### Data & ORM recipes (Nest docs often name JS ORMs)

| Topic | Status | Notes |
|-------|--------|--------|
| Prisma / SQL (Rust path) | **Done** (crate) | `nestrs-prisma` + SQLx — see table above |
| TypeORM / Sequelize / Mongoose (as frameworks) | **No / external** | **No** JS ORMs in Rust; use **SQLx**, **Diesel**, **SeaORM**, **Prisma client**, etc., per app |
| MikroORM | **No / external** | Same as above — pick a Rust persistence stack |

### CQRS, events, messaging recipes

| Topic | Status | Notes |
|-------|--------|--------|
| CQRS module pattern | **Partial** | **`nestrs-cqrs`**; sagas are **trait-level** (`Saga`, `SagaDefinition`), not Nest’s runtime saga chapter parity |
| Event bus | **Done** (crate) | **`nestrs-events`** (`EventBus`) — see table above |
| Micro transports (Kafka, MQTT, RabbitMQ, …) | **Done / Partial** | See microservices rows in table above |

### Auth, security & companion recipes

| Topic | Status | Notes |
|-------|--------|--------|
| Guards / strategies / JWT-style flows | **Done** (patterns) | `CanActivate`, `AuthStrategy`, bearer helpers — see table above |
| **Passport** (Nest recipe) | **No / external** | **No** bundled Passport port; compose **`jsonwebtoken`**, **`oauth2`**, **`openidconnect`**, provider crates, or API gateways |
| **Sentry** as a first-class recipe | **No / external** | Use **`sentry`**, **`tracing`**, **`tracing-subscriber`** + Sentry integration; not a dedicated `nestrs` module |
| **Async local storage** (Nest `AsyncLocalStorage` recipe) | **No / external** | Rust: **`tokio::task_local!`**, Axum request extensions, **`RequestScoped`** / per-request state in `nestrs`; no Nest-named `AsyncLocalStorage` API |

### Realtime, adapters, testing ergonomics

| Topic | Status | Notes |
|-------|--------|--------|
| WebSockets | **Partial** | See WS rows in table above |
| **Necord** (Discord bots) | **No / external** | Use **Discord API** crates (e.g. **serenity**, **twilight**); not in `nestrs` |
| **Suites / Automock**-style testing | **No / external** | Use **`mockall`**, **`wiremock`**, **`tower::Service` mocks**, custom test harnesses; `nestrs::testing` helpers where provided |

### Tooling & DX (compiler, REPL, docs site)

| Topic | Status | Notes |
|-------|--------|--------|
| **SWC** / TS transpiler | **No / external** | Rust stack uses **rustc**; no SWC analogue inside `nestrs` |
| **REPL** | **No / external** | Use **`evcxr`**, **`rust-script`**, or small `cargo run` bins — not a framework REPL |
| **Hot reload** as a framework feature | **No / external** | Use **`cargo-watch`**, **`watchexec`**, **`bacon`**, or IDE; Axum server restart is app/ops concern |
| **Compodoc** (Angular/Nest doc generator) | **No / external** | Use **`rustdoc`**, **`docs.rs`**, mdBook (this book), or third-party diagram tools |
| **Commander** (Nest CLI UX) | **No / external** | **`clap`**-based **`nestrs`** CLI; different UX and plugin model |

### Router / module recipes (Nest naming)

| Topic | Status | Notes |
|-------|--------|--------|
| **Router module** (lazy dynamic children) | **No / external** | Compose **`Router`** / **`nestrs` modules** explicitly; no separate “RouterModule” type mirroring Nest |
| Lazy-loaded feature modules | **Partial** | Dynamic modules / composition exist; naming and ergonomics differ from Nest |

### Static assets & HTTP edge cases

| Topic | Status | Notes |
|-------|--------|--------|
| **Serve static** (Nest `ServeStaticModule` recipe) | **Partial** | **Tower / Axum** file serving; no dedicated Nest-style wrapper in `nestrs` (see table above) |

### FAQ-style topics (hosting, careers, curriculum)

These are **documentation / product** surface on [docs.nestjs.com](https://docs.nestjs.com), not runtime APIs. In **nestrs** they are **not** framework deliverables; treat as **No** for parity or **use ecosystem + your own docs**.

| Topic | Status | Notes |
|-------|--------|--------|
| Serverless / FaaS deployment guides | **No / external** | Platform-specific (Lambda, Cloud Run, Vercel Rust, etc.) + Axum `tower` adapters |
| Hybrid app (HTTP + microservice in one process) | **Partial / pattern** | Possible by composing routers and transports; no single “hybrid” chapter |
| Devtools / migration guides / version upgrade playbooks | **No / external** | This book + changelog + [Contributing](contributing.md); no Nest-style devtools package |
| Courses, jobs board, community FAQ pages | **No / external** | Project/community content, not the library |

---

See [`nestrs-plan-2.md`](../../nestrs-plan-2.md) in the repo root for the full phased roadmap narrative.
