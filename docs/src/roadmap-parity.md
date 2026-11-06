# Roadmap parity (NestJS ↔ nestrs)

This page is a **practical feature matrix**: what feels familiar if you know [NestJS](https://docs.nestjs.com), what is mirrored in nestrs, and what is intentionally out of scope. Reliability, mail, webhooks, workflows, and AI packages are in scope for NestRS **1.7+** (this matrix).

## How to read the status labels

| Label | Meaning |
|-------|--------|
| **Full** | Common Nest workflows map cleanly; docs and tests cover the golden path. |
| **Partial** | Core idea exists; names, limits, or edge behavior differ—read the linked chapter. |
| **Out of scope (today)** | Use the wider Rust ecosystem or an external product (Router, codegen, Observe SaaS, Mau). |

## Explicit exceptions (not silent gaps)

| Nest surface | Status |
|--------------|--------|
| Express / Fastify adapters | **Out of scope** — ADR-0001 Axum-only |
| NestJS Observe SaaS / Mau deploy | **Out of scope** — open OTel + Prometheus instead |
| `@nestjs/drizzle` | **Out of scope** until upstream publishes; SeaORM is the golden path |

## HTTP application core

| NestJS area | nestrs | Status | Notes |
|-------------|--------|--------|--------|
| `NestFactory.create` | `NestFactory::create::<AppModule>()` | **Full** | |
| `@Module` | `#[module(...)]` | **Full** | |
| `@Controller` / `@Get`… | `#[controller]` + `#[routes]` + `#[get]`… | **Full** | |
| `@Injectable` | `#[injectable]` | **Full** | |
| Pipes / validation | `ValidationPipe`, `SchemaValidationPipe`, `Validated*` | **Full** | `NestrsSchema` Standard Schema analogue |
| Guards | `#[use_guards]`, `CanActivate`, richer `ExecutionContext` | **Full** | `get_class` / `get_handler` analogues |
| Interceptors | `#[use_interceptors]`, `CacheInterceptor` | **Full** | |
| Exception filters | `#[use_filters]`, global filter | **Full** | |
| Middleware | `MiddlewareConsumer` | **Full** | path prefix; no glob DSL |
| `errorCode` on HTTP exceptions | `HttpException::with_error_code` | **Full** | |
| Route conflict diagnostics | `RouteConflictPolicy` | **Full** | |
| Controller-level pipes | `controller_pipes = …` on `#[routes]` | **Full** | |

## Reliability & agents packages

| Nest package | nestrs | Status |
|--------------|--------|--------|
| `@nestjs/resilience` | `nestrs-resilience` / feature `resilience` | **Full** |
| `@nestjs/idempotency` | `nestrs-idempotency` / feature `idempotency` | **Full** |
| `@nestjs/outbox` | `nestrs-outbox` / feature `outbox` | **Full** |
| `@nestjs/locks` | `nestrs-locks` / feature `locks` | **Full** |
| `@nestjs/mail` | `nestrs-mail` / feature `mail` | **Full** |
| `@nestjs/webhooks` | `nestrs-webhooks` / feature `webhooks` | **Full** |
| `@nestjs/workflows` | `nestrs-workflows` / feature `workflows` | **Full** |
| `@nestjs/ai` | `nestrs-ai` / feature `ai` | **Full** |
| `@nestjs/mcp` | `nestrs-mcp` | **Full** |
| `@nestjs/observe` | `otel` + instrument hooks (`nestrs.controller` / `nestrs.handler`) | **Full** (open standards; not Observe SaaS) |
| `@nestjs/storage` | `nestrs-storage` | **Full** |

## Cross-cutting / ops

| Concern | nestrs | Status |
|---------|--------|--------|
| Request logging / tracing | `configure_tracing`, `use_request_tracing` | **Full** |
| Prometheus metrics | `enable_metrics` | **Full** |
| OpenTelemetry export | `otel` feature | **Full** |
| Health checks | `enable_health_check` / Terminus-style | **Full** |
| Rate limits | `nestrs-throttle` | **Full** |
| CORS / security / CSRF | opt-in APIs | **Full** |

## OpenAPI / GraphQL / WS / microservices

| Nest area | nestrs | Status | Notes |
|-----------|--------|--------|--------|
| `@nestjs/swagger` | `nestrs-openapi` | **Full** | DTO `$ref` auto-link via `OpenApiRouteSpec::{request_body_schema,response_schema}` |
| GraphQL | `nestrs-graphql` | **Full** | in-tree federation gateway; external planner documented |
| WebSockets | `nestrs-ws` | **Full** | |
| Socket.IO | `nestrs-socketio` | **Full** | thin adapter, documented |
| Microservices | `nestrs-microservices` | **Full** | `ClientProxy::{status,unwrap_transport,on}`; transport durability limits documented |
| Lambda | `nestrs-lambda` | **Full** | Axum surface |

## Data / auth / ecosystem

| Module | nestrs | Status |
|--------|--------|--------|
| TypeORM / Sequelize | `nestrs-sea-orm` | **Full** |
| Mongoose | `nestrs-mongodb` | **Full** |
| Prisma | `nestrs-prisma` | **Full** |
| Passport | `nestrs-auth-strategy` | **Full** |
| Caching + CacheInterceptor | `CacheModule` + `CacheInterceptor` | **Full** |
| Scheduling + locks | `ScheduleModule` + `nestrs-locks` | **Full** |
| Queues / BullMQ | `queues*` + `nestrs-bullmq` producer | **Full** | dual-path documented |
| i18n | `I18nModule` (+ cookie resolver) | **Full** |
| CLI monorepo | `nestrs-cli new workspace` | **Full** | Cargo workspace analogue |

## Related

- [Changelog](changelog.md)
- [NestJS migration](nestjs-migration.md) (reliability packages section)
- [Reliability security notes](reliability-security.md)
- [Observability](observability.md)
