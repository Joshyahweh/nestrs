# nestrs

NestJS-like API framework for Rust built on Axum and Tower.

`nestrs` gives you a familiar module/controller/provider mental model with Rust performance and explicit typing.

[![Security](https://github.com/Joshyahweh/nestrs/actions/workflows/security.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/security.yml)
[![CI](https://github.com/Joshyahweh/nestrs/actions/workflows/ci.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/ci.yml)
[![Performance](https://github.com/Joshyahweh/nestrs/actions/workflows/performance.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/performance.yml)
[![Fuzz](https://github.com/Joshyahweh/nestrs/actions/workflows/fuzz.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/fuzz.yml)
[![Release Version Check](https://github.com/Joshyahweh/nestrs/actions/workflows/release-version-check.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/release-version-check.yml)
[![Publish Crates](https://github.com/Joshyahweh/nestrs/actions/workflows/publish-crates.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/publish-crates.yml)
[![Benchmark Storage Sync Template](https://github.com/Joshyahweh/nestrs/actions/workflows/benchmark-storage-sync.yml/badge.svg)](https://github.com/Joshyahweh/nestrs/actions/workflows/benchmark-storage-sync.yml)

## Highlights

- Module-oriented architecture (`module`, `controller`, `injectable` macros)
- HTTP route macros (`get`, `post`, `put`, `patch`, `delete`, `options`, `head`, `all`)
- DI + application context — request scopes, lifecycle hooks, `useValue`/`useFactory` providers with opt-in lifecycles
- DTO validation pipeline, class-validator-style ergonomics, and extraction-time pipe chains (`#[use_pipes]` — `ValidationPipe`, `ParseIntPipe`, `TrimPipe`, custom pipes)
- Cross-cutting pipeline: guards, pipes, interceptors, exception filters, strategies
- Row-level authorization (`authz-row-level`): deny-closed `CrudService`, module-qualified principals, masking subjects
- `#[crud]` generated controllers with paginated list endpoints (limit/offset)
- Microservice transports via `nestrs-microservices`: NATS, Redis, Kafka, RabbitMQ (AMQP 0.9), MQTT, TCP (length-prefixed, bounded frames), gRPC
- GraphQL (async-graphql) incl. **federation gateway** (`EntityResolver`, `_service`/`_entities`), data loaders, WebSocket subscriptions
- OAuth2 via `nestrs-oauth2`: 4 grant types incl. PKCE, JWKS-backed resource server, social providers (Google/GitHub/Microsoft/Apple), `OAuth2Guard`
- Multi-cloud object storage via `nestrs-storage` (S3 / GCS / Azure / Local, built on `object_store`)
- DB migrations + seeding: `nestrs-cli db migrate add|run|revert|info` and `db seed --bin|--seed-file`
- OpenAPI/Swagger via `nestrs-openapi`, with `#[dto]` schemars reflection
- CQRS (`nestrs-cqrs`) and in-process events (`nestrs-events`)
- Production controls: backpressure, bounded caches, rate limiting with proxy-topology awareness, metrics, request tracing, security runbooks
- **Model Context Protocol server** (`nestrs-mcp`) for Claude Code, Cursor, VS Code, Codex CLI — project introspection, live runtime, scaffolding, docs search ([guide](docs/src/mcp.md))
- Performance hardening workflows: benchmark gating (HTTP, DI, validated JSON), history tracking, dashboard artifacts; scheduled libFuzzer smoke runs

## Ownership and release

- Maintainer / code owner: @Joshyahweh
- Current workspace version: `1.3.0` (from `VERSION` and workspace package settings) — a minor release on the 1.0 stable contract; the public API is covered by full semver (see `STABILITY.md`)
- Release notes template: `.github/release-template.md`
- Changelog: `CHANGELOG.md`
- Contribution guide: `CONTRIBUTING.md`
- Release process: `RELEASE.md`
- Code of conduct: `CODE_OF_CONDUCT.md`
- Security disclosure policy: `SECURITY.md`
- Licenses: `LICENSE-MIT` and `LICENSE-APACHE`

## Toolchain policy

- Rust edition: `2021` (workspace-level)
- MSRV: `1.88` (tested in CI as `1.88.0`)
- CI matrix: MSRV + `stable` + `beta`
- Contributor note: keep new crates on `edition.workspace = true` and `rust-version.workspace = true` unless there is a documented exception


## Project Layout

- `nestrs/` - main framework crate (public runtime API)
- `nestrs-core/` - runtime primitives (context, traits, metadata, strategy)
- `nestrs-macros/` - proc macros and helper attributes
- `nestrs-cli/` - scaffold/generate CLI + `db` migrations/seeding (crates.io package name: **`nestrs-scaffold`**, binary: `nestrs-cli`)
- `nestrs-prisma/` - Prisma integration crate
- `nestrs-microservices/` - transport/client/event primitives (NATS, Redis, Kafka, RabbitMQ, MQTT, TCP, gRPC)
- `nestrs-cqrs/` - CQRS command/query bus primitives
- `nestrs-events/` - in-process event bus (Nest-style `@OnEvent` analogue)
- `nestrs-oauth2/` - OAuth2 client, JWKS-backed resource server, social providers, `OAuth2Guard`
- `nestrs-storage/` - multi-cloud object storage (S3 / GCS / Azure / Local)
- `nestrs-openapi/`, `nestrs-graphql/`, `nestrs-ws/` - parity extension crates
- `nestrs-mcp/` - Model Context Protocol server (stdio + Streamable HTTP)
- `nestrs-mongodb/` - Mongoose-style MongoDB adapter (`MongoModule`, `MongoRepository<T>`)
- `nestrs-http/` - outbound HTTP client (`HttpModule` / `HttpService`)
- `nestrs-throttle/` - per-route rate limits (`ThrottlerGuard`, in-memory or Redis)
- `nestrs-health/` - readiness / liveness indicators
- `nestrs-security/` - Bearer parsing, helmet headers, CSRF middleware
- `nestrs-socketio/` - Socket.IO adapter (socketioxide)
- `nestrs-lambda/` - AWS Lambda / API Gateway adapter
- `nestrs-better-auth/` - Better Auth session-cookie guard + router nest
- `nestrs-bullmq/` - BullMQ Redis key-layout producer
- `nestrs-auth-strategy/` - Passport-style `AuthStrategy` adapters
- `nestrs-saml/` / `nestrs-ldap/` - SAML SP redirect and LDAP simple bind
- `nestrs-sea-orm/` - SeaORM `for_root_async` (TypeORM/Sequelize analogue)
- `website/` - landing page + docs hub (light/dark theme)

## Quick Start

### 1) Build and test

```bash
cargo check --workspace
cargo test --workspace
```

### 2) Run an example app

```bash
cargo run -p hello-app
```

If the example package name differs in your local setup, run:

```bash
cargo run --manifest-path examples/hello-app/Cargo.toml
```

### 3) Preview website/docs locally

```bash
python3 -m http.server 4173
```

Then open:

- `http://localhost:4173/website/` (landing page)
- `http://localhost:4173/website/docs.html` (documentation hub)

## Documentation Index

### Core docs

- `website/docs.html` - docs portal entrypoint
- **NestJS → nestrs** — mdBook: [`docs/src/nestjs-migration.md`](docs/src/nestjs-migration.md); website hub: [`website/docs/migration/nestjs-to-nestrs.md`](website/docs/migration/nestjs-to-nestrs.md)
- **Security defaults & ordering** — mdBook: [`docs/src/secure-defaults.md`](docs/src/secure-defaults.md), [`docs/src/http-pipeline-order.md`](docs/src/http-pipeline-order.md)
- **MCP / AI editor integration** — mdBook: [`docs/src/mcp.md`](docs/src/mcp.md); Mintlify: [`mintlify-docs/guides/mcp.mdx`](mintlify-docs/guides/mcp.mdx)
- `CHANGELOG.md` - release history
- `STABILITY.md` - semver, public vs `#[doc(hidden)]` API, **`test-hooks`** / global registries

### Platform/operations

- `PRODUCTION_RUNBOOK.md` - deployment/operations runbook
- `SECURITY.md` - security guidance and controls
- `MICROSERVICES.md` - microservices/event-driven patterns

### Performance and benchmark ops

- `benchmarks/BASELINE.md` - how to run, compare, and track benchmarks
- `benchmarks/relative_thresholds.json` - active relative regression gate config
- `nestrs/fuzz/` and `nestrs-microservices/fuzz/` - `cargo-fuzz` targets (see `PRODUCTION_RUNBOOK.md`)

### Storage + publishing

- `BENCHMARK_STORAGE_PLAYBOOK.md` - long-term storage layout and restore workflow
- `BENCHMARK_STORAGE_SECRETS_CHECKLIST.md` - provider setup checklist (OIDC/least privilege)
- `.github/workflows/benchmark-storage-sync.yml` - manual-dispatch storage sync template

## Common Commands

```bash
# benchmark gates
python3 scripts/load/check_benchmark_thresholds.py
python3 scripts/load/check_benchmark_relative_regression.py

# benchmark reports and recommendation artifacts
python3 scripts/load/export_benchmark_report.py
python3 scripts/load/maintain_benchmark_history.py
python3 scripts/load/build_benchmark_dashboard.py
python3 scripts/load/recommend_relative_thresholds.py
python3 scripts/load/evaluate_threshold_reassessment.py
```

## CI Workflows

- `.github/workflows/security.yml` - security checks
- `.github/workflows/ci.yml` - PR/push checks on MSRV + stable + beta, plus fmt/clippy/docs/audit
- `.github/workflows/performance.yml` - performance benches, gating, reporting, optional publishing
- `.github/workflows/fuzz.yml` - weekly libFuzzer smoke (wire JSON, auth header, URI/JSON)
- `.github/workflows/benchmark-storage-sync.yml` - storage sync template for S3/GCS/Azure
- `.github/workflows/release-version-check.yml` - enforces `VERSION` and latest `CHANGELOG.md` release heading stay in sync
- `.github/workflows/publish-crates.yml` - tag-driven crates.io publish after preflight; uses `CARGO_REGISTRY_TOKEN` (optional OIDC/trusted publishing can replace this)

## GitHub community templates

- `.github/ISSUE_TEMPLATE/bug_report.yml`
- `.github/ISSUE_TEMPLATE/feature_request.yml`
- `.github/ISSUE_TEMPLATE/config.yml`
- `.github/pull_request_template.md`

## Status

The tracked roadmap implementation is complete. Ongoing work is maintenance mode: periodic benchmark history accumulation and threshold re-evaluation when data changes.
