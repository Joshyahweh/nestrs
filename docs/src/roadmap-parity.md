# Roadmap parity (NestJS ↔ nestrs)

This page is a **practical feature matrix**: what feels familiar if you know [NestJS](https://docs.nestjs.com), what is **partially** mirrored in nestrs, and what you should plan for explicitly when shipping Rust services. It is not a promise of line‑for‑line API compatibility—nestrs targets **Nest-like structure** with **Rust idioms** (macros, `Arc`, explicit extractors).

## How to read the status labels

| Label | Meaning |
|-------|--------|
| **Full** | Common Nest workflows map cleanly; docs and tests cover the golden path. |
| **Partial** | Core idea exists; names, limits, or edge behavior differ—read the linked chapter. |
| **Planned / evolving** | Directionally aligned; check crate READMEs and `CHANGELOG.md`. |
| **Out of scope (today)** | Use the wider Rust ecosystem or an external product (Router, codegen, etc.). |

## HTTP application core

| NestJS area | nestrs | Status | Notes |
|-------------|--------|--------|--------|
| `NestFactory.create` | `NestFactory::create::<AppModule>()` | **Full** | See [First steps](first-steps.md). |
| `@Module` | `#[module(...)]` | **Full** | Static graph; dynamic modules via `DynamicModule` patterns—[Fundamentals](fundamentals.md). |
| `@Controller` / `@Get`… | `#[controller]` + `#[routes]` + `#[get]`… | **Full** | Prefix, versioning, and metadata on handlers. |
| `@Injectable` | `#[injectable]` + `providers = [...]` | **Full** | Scopes: singleton / transient / request—[Fundamentals](fundamentals.md). |
| Pipes / validation | `ValidationPipe`, `ValidatedBody` / `ValidatedQuery` / `ValidatedPath` | **Partial** | `#[dto]` + `validator`; not identical to `class-validator`—[NestJS migration](nestjs-migration.md). |
| Guards | `#[use_guards]`, `CanActivate` | **Partial** | Semantics align; integration differs from Nest’s `ExecutionContext`—[HTTP pipeline order](http-pipeline-order.md). |
| Interceptors | `#[use_interceptors]` | **Partial** | Tower-style wrapping; ordering is documented—[HTTP pipeline order](http-pipeline-order.md). |
| Exception filters | `#[use_filters]`, global filter on `NestApplication` | **Partial** | `HttpException` mapping; see same ordering page. |
| Middleware | `MiddlewareConsumer` + `configure_middleware` | **Full** | Path `for_routes` / `exclude`; no glob `ab*cd` DSL. |

## Cross-cutting concerns and operations

| Concern | nestrs | Status | Where to read |
|---------|--------|--------|----------------|
| Request logging / tracing | `configure_tracing`, `use_request_tracing`, `use_request_id` | **Full** | [Observability](observability.md) |
| Prometheus metrics | `enable_metrics` | **Full** | [Observability](observability.md), [Production](production.md) |
| OpenTelemetry export | `otel` feature, `configure_tracing_opentelemetry` | **Partial** | [Observability](observability.md) |
| Health checks | `enable_health_check` | **Full** | rustdoc / [Production](production.md) |
| Rate limits, timeouts, body limits | `NestApplication` builders | **Partial** | Often paired with edge proxies—[Secure defaults](secure-defaults.md) |
| CORS, security headers, CSRF | Opt-in APIs + warnings in prod | **Partial** | Explicit by design—[Secure defaults](secure-defaults.md), [Security](security.md) |

## OpenAPI and documentation

| Nest (`@nestjs/swagger`) | nestrs | Status | Notes |
|--------------------------|--------|--------|--------|
| Automatic DTO schemas from classes | `#[dto]` derives `schemars::JsonSchema`; `schema_entry` / `with_schemas` land it under `components.schemas` | **Partial** | Not auto-linked to operations; non-DTO types via `utoipa`, manual JSON, or `okapi` fragments—[OpenAPI & HTTP](openapi-http.md). |
| `paths` / operations from controllers | Generated from route registry | **Full** | Summaries, tags, `operationId`, default responses. |
| Security schemes + `#[roles]` hint | `OpenApiOptions` | **Partial** | Heuristic from metadata—[OpenAPI & HTTP](openapi-http.md). |

## GraphQL, WebSockets, microservices

| Nest area | nestrs | Status | Notes |
|-----------|--------|--------|--------|
| GraphQL (code-first / schema) | `nestrs-graphql` | **Partial** | async-graphql ecosystem; lightweight federation gateway in-tree (`graphql-federation-gateway` feature, batched `EntityResolver`); query planning / codegen external—[GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md). |
| WebSockets gateway | `nestrs-ws` | **Partial** | Guards/pipes/interceptors with DI resolution; origin allowlist via `WsSecurityConfig` (CSWSH); error path differs from HTTP filters—same chapter. |
| Transport microservices | `nestrs-microservices` | **Partial** | Kafka, NATS, Redis, MQTT, RabbitMQ, gRPC—[Microservices](microservices.md). |
| Socket.IO | **`nestrs-socketio`** | **Partial** | socketioxide layer on the Axum router; nestrs-ws stays RFC 6455. |
| Serverless / Lambda | **`nestrs-lambda`** | **Partial** | `listen_lambda(router)` via `lambda_http`. No Fastify/Express adapter. |
| Passport | **`nestrs-auth-strategy`** | **Partial** | `JwtStrategy` / `LocalBasicStrategy` on `AuthStrategy`. |
| Better Auth | **`nestrs-better-auth`** | **Partial** | Nest better-auth.rs router + session-cookie guard. |
| SAML / LDAP | **`nestrs-saml`**, **`nestrs-ldap`** | **Partial** | SP redirect + bind auth; assertion crypto stays in the app. |
| TypeORM / Sequelize | **`nestrs-sea-orm`** | **Full / Partial** | `Repo`, ambient tx, `RowAuthz` / `AbilityAuthz`; drizzle still carved out. |
| Route posture (`#[Public]`-style) | `#[public]` + `require_route_posture` | **Full** | Boot fails if a route lacks `#[public]` or guards. |
| Shared JSON wire format | `nestrs_microservices::wire` | **Full** | Golden tests in crate; revision constant for docs. |

## Ecosystem modules (Nest “@nestjs/…”-style)

| Module | nestrs | Status | Notes |
|--------|--------|--------|--------|
| Caching | `CacheModule` / `CacheService` | **Partial** | In-memory + optional Redis—[Ecosystem modules](ecosystem.md). |
| Scheduling | `ScheduleModule`, `#[cron]`, `#[interval]` | **Partial** | Feature `schedule`—[Ecosystem modules](ecosystem.md). |
| Queues | `QueuesModule` + **`nestrs-bullmq`** | **Partial** | In-process `queues`; `queues-redis` LPUSH/BRPOP; BullMQ key-layout producer crate for Node workers. |
| i18n | `I18nModule` | **Partial** | Catalogs + locale resolver—[Ecosystem modules](ecosystem.md). |

## CLI and developer experience

| Nest CLI | nestrs | Status | Notes |
|----------|--------|--------|--------|
| `nest new` | `nestrs-cli new` | **Partial** | Single-crate scaffold—[CLI](cli.md). |
| `nest generate` | `nestrs-cli generate` (`nestrs-cli g`) | **Partial** | Resource, service, controller, DTO, guard, …—[CLI](cli.md). |
| Monorepo / plugins | — | **Out of scope (today)** | Use Cargo workspaces and standard Rust tooling—[CLI](cli.md). |
| Nest Devtools | Admin sidecar HTML | **Partial** | `GET /__nestrs` / `/__nestrs/devtools` (feature `admin`). |

## Testing and stability

| Topic | nestrs | Notes |
|-------|--------|--------|
| Global registries in tests | `test-hooks` feature (tests only) | See `STABILITY.md` at repository root; never enable in production binaries. |
| Semver and public API | Documented in `STABILITY.md` | Includes `#[doc(hidden)]` policy. |

## Using this matrix in a migration

1. **Start HTTP‑shaped**: modules, controllers, DTO validation, guards—[First steps](first-steps.md), [NestJS migration](nestjs-migration.md).
2. **Lock cross-cutting order**: [HTTP pipeline order](http-pipeline-order.md).
3. **Harden for production**: [Secure defaults](secure-defaults.md), [Observability](observability.md), [Production runbook](production.md).
4. **Add OpenAPI and optional transports** when the core API is stable—[OpenAPI & HTTP](openapi-http.md), [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md).

## FAQ

**Is nestrs a NestJS port?**  
No. It borrows **structure** (modules, controllers, guards) and maps them to **Rust** (macros, Axum, Tower). Expect to write **explicit types** and to read [HTTP pipeline order](http-pipeline-order.md) for ordering guarantees.

**Can I use Express/Fastify-style middleware instead of Axum?**  
The core HTTP engine is **Axum-only** ([ADR-0001](adrs/0001-axum-only.md)). Add Tower layers or bridge at the process edge.

**Why do my integration tests flake?**  
See **`STABILITY.md`** and [Fundamentals](fundamentals.md) — global registries may need **`test-hooks`** + clears in parallel test runs.

**Where is Swagger schema generation for my DTOs?**  
`#[dto]` types derive `schemars::JsonSchema` — feed them to `schema_entry` / `OpenApiOptions::with_schemas` to land under `components.schemas` (not auto-linked to operations); merge **`components`** for everything else ([OpenAPI & HTTP](openapi-http.md)).

## Related

- [Fundamentals](fundamentals.md) — DI, scopes, lifecycle, dynamic modules, cycles  
- [Custom decorators](custom-decorators.md) — metadata and extractors vs Nest decorators  
- [Changelog](changelog.md) — what changed release by release  
- [API cookbook](appendix-api-cookbook.md) — runnable-style snippets for `NestFactory` / `NestApplication` APIs  
