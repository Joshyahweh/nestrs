# nestrs

**New to the framework?** Start with [**First steps**](first-steps.md) (minimal app, CLI, where to read next). For **Postgres/MySQL/SQLite + Prisma**, **MongoDB**, **GraphQL**, and **gRPC** wiring in one place, see [**Backend stack recipes**](backend-recipes.md). Coming from **NestJS**? Read the [**NestJS → nestrs migration guide**](nestjs-migration.md). For Nest’s *custom decorators* mapped to Rust macros and metadata, see [**Custom decorators**](custom-decorators.md). For **scopes**, **lifecycle**, **dynamic modules**, **`ModuleRef`**, **discovery**, **factories**, and **circular dependencies**, see [**Fundamentals**](fundamentals.md). For **OpenAPI** schemas (**utoipa** / **okapi** / manual `components`) and **Swagger security** (including **`#[roles]`** inference), see [**OpenAPI & HTTP**](openapi-http.md). For **GraphQL** scope, **WebSocket** error semantics vs HTTP filters, **micro** guard/pipe parity, **wire** golden tests, and **gRPC** usage, see [**GraphQL, WebSockets & microservices DX**](graphql-ws-micro-dx.md).

## How this book is organized

| Section | Chapters | Best for |
|---------|----------|----------|
| **Onboarding** | [First steps](first-steps.md), [Backend stack recipes](backend-recipes.md), [CLI](cli.md) | Running your first server, stack-specific procedures, generators. |
| **Nest → Rust** | [NestJS migration](nestjs-migration.md), [Custom decorators](custom-decorators.md), [Fundamentals](fundamentals.md) | Teams porting modules, DI, and route style from NestJS. |
| **Platform** | [Observability](observability.md), [Production](production.md), [Ecosystem modules](ecosystem.md) | Metrics, tracing, cache/schedule/queues/i18n. |
| **APIs & protocols** | [OpenAPI & HTTP](openapi-http.md), [Microservices](microservices.md), [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md) | HTTP docs, messaging, GraphQL/WS. |
| **Security & pipeline** | [Security](security.md), [Secure defaults](secure-defaults.md), [HTTP pipeline order](http-pipeline-order.md) | Hardening and deterministic middleware order. |
| **Project** | [Contributing](contributing.md), [ADRs](adrs.md), [Release](release.md), [Changelog](changelog.md), [Roadmap parity](roadmap-parity.md) | Contributors and release consumers. |
| **API reference snippets** | [API cookbook](appendix-api-cookbook.md) | Copy-paste examples for `NestApplication` builder methods and CLI commands named in other chapters. |

## Suggested reading paths

1. **“I want a working HTTP API today”** — [First steps](first-steps.md) → run `examples/hello-app` from the repo → [Fundamentals](fundamentals.md) for DI scopes you will hit next.  
2. **“We are migrating from NestJS”** — [NestJS migration](nestjs-migration.md) → [HTTP pipeline order](http-pipeline-order.md) → [OpenAPI & HTTP](openapi-http.md) if you rely on Swagger.  
3. **“We are shipping to production”** — [Secure defaults](secure-defaults.md) → [Observability](observability.md) → [Production runbook](production.md) (full operations text is included from the repository).  

The section below embeds the repository **`README.md`** so installation, layout, CI badges, and command cheat sheets stay in sync with the default GitHub view.

**About code snippets:** Examples use **`nestrs`** and **`tokio`** like a normal Cargo project. They are **not** executable in the browser Rust Playground (that service does not ship this framework). Use **copy** and paste into `src/main.rs` of a project that depends on `nestrs`, or run the repo’s **`examples/`** crates—see [First steps](first-steps.md). For **one snippet per `NestApplication` method** (and CLI commands), use the [API cookbook](appendix-api-cookbook.md).

---

{{#include ../../README.md}}

