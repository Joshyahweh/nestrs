# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.5.1] - 2026-09-03

### Fixed

- **CI test compatibility**: `nestrs-scaffold` integration tests now read the binary path through `std::env::var("CARGO_BIN_EXE_nestrs-cli")` (with an underscore-form fallback for Rust ≤ 1.88, which still normalizes dashes to underscores in build-script env vars). Without the fallback, the test-matrix (1.88) CI job failed with `CARGO_BIN_EXE_nestrs-cli ... NotPresent` on every test.
- **crates.io publish order**: `publish-crates.yml` now publishes `nestrs-mcp` before `nestrs-scaffold` so that `nestrs-scaffold`'s optional `nestrs-mcp` dependency can be resolved against crates.io. The previous order published `nestrs-scaffold` 11th, which failed with `no matching package named nestrs-mcp found` because `nestrs-mcp` was 12th.

> Note: v0.5.0 was partially published to crates.io (10 of 12 crates). v0.5.1 is the first complete 0.5.x release and supersedes v0.5.0. The v0.5.0 crates remain on crates.io for anyone who pinned to them; v0.5.1 is the recommended upgrade.

## [0.5.0] - 2026-08-27

### Added

- **`nestrs-mcp`**: new workspace member (`nestrs-mcp/`). Model Context Protocol server exposing project introspection (modules, controllers, providers, routes, DTOs), scaffolding actions (new project, create module, create resource, create DTO, generate CRUD), local docs search, and live runtime queries against the new `nestrs::admin` sidecar. Speaks stdio (default) and Streamable HTTP (`--features http`, mounted at `/mcp`). Build with `cargo install nestrs-mcp` and add the binary to any MCP-aware client.
- **`nestrs-mcp` setup wizard**: new `init` / `setup` subcommand runs a post-install setup wizard that detects installed editors (Claude Code, Cursor, VS Code Copilot, Codex CLI) and writes the right MCP config into each one. Multi-select prompt mirrors the hand-rolled `nestrs-cli` style (no new prompt dependencies). Flags: `--yes` to accept all detected editors, `--no-interactive` for dry-run / scripted use, `--start-http-server` to spawn the server in the background after writing configs. JSON and TOML merges are idempotent and preserve all unrelated keys. Docs updated in `nestrs-mcp/README.md`, `docs/src/mcp.md`, and `mintlify-docs/guides/mcp.mdx` / `mintlify-docs/api/crates/nestrs-mcp.mdx`.
- **`nestrs::admin`**: new `admin` Cargo feature (off by default) wires a localhost-only HTTP sidecar into `NestApplication::use_admin(AdminOptions)`. Exposes `GET /__nestrs/health`, `/__nestrs/providers`, `/__nestrs/routes`, `/__nestrs/openapi.json` over the live registries. Optional bearer token (refuses to bind non-loopback without one). Consumed by `nestrs-mcp`'s `get_app_health` / `get_app_routes` / `get_app_providers` tools.
- **`AdminSnapshot` value type** in `nestrs-core` (always available, no feature gate): a serializable view of the provider, route, and metadata registries for tooling and external introspection.

### Changed

- **BREAKING: `NestApplication::use_admin` now takes `&self` instead of `self`.** Previously the call consumed the application, forcing the admin handle to be the last builder step before `listen*`. Now you can mount the admin sidecar at any point in the builder chain. The signature is the only break — no behavior changed. Code that wrote `let app = NestFactory::create::<AppModule>(); let h = app.use_admin(opts); app.listen(...).await;` now compiles without a `mem::replace` dance. Code that wrote `app.use_admin(opts).serve().await` in one expression is unaffected (the result is still an owned `AdminHandle`).
- **BREAKING: CLI binary renamed from `nestrs` to `nestrs-cli`.** The `nestrs-scaffold` crate now installs as the `nestrs-cli` binary (it was previously `nestrs`). The crate name on crates.io is unchanged because `nestrs-cli` is already owned by another publisher there. Install with `cargo install nestrs-scaffold` and invoke as `nestrs-cli new …` / `nestrs-cli generate …`. The Cargo alias (`cargo nestrs …`) keeps the short name and is unchanged. All docs and the `nestrs-cli` help text reflect the new name.

### Fixed

- **`nestrs-mcp` source parser**: `state` and `controller_guards` from `#[routes(X, state = T, controller_guards = (...))]` are now back-filled onto the struct-form controller (previously they were silently dropped when the controller was declared as a struct + separate `impl`). `body_type` extraction now walks past the `&self` receiver to find the first typed arg. `set_metadata("k", "v")` positional form is now recognized. `#[roles("a", "b", ...)]` positional form is now recognized. `#[openapi(summary = "...", operation_id = "...")]` now collects every kv pair (previously kept only the last one). 20 new tests in `tests/source_parser_coverage.rs` lock these contracts in.
- **`hello-app` smoke harness**: `use_admin` wired into `examples/hello-app/src/main.rs` so the live admin port is reachable in the canonical example app. `NESTRS_HELLO_PORT` env var overrides the listen port (default `3000`) for environments where 3000 is already taken. `NESTRS_ADMIN_TOKEN` enables bearer auth on the sidecar (still refuses non-loopback binds without one).
- **`nestrs::admin` auth error boxing**: `AdminSnapshot::authed` now returns a small `AuthError` enum instead of a full `axum::response::Response` in the `Err` slot, silencing `clippy::result_large_err` on the lint-and-docs CI gate without changing behavior (`AuthError` converts to a `Response` via the existing `From` impl).
- **Exception filter ordering**: the global exception filter is now applied as the outermost layer in the Axum middleware stack, so it observes responses *before* outer middleware does. The `global_exception_filter_runs_before_outer_middleware` ordering contract test in `tests/bootstrap_composition.rs` now passes.
- **`nestrs-scaffold` integration tests** now read the binary path through `std::env::var("CARGO_BIN_EXE_nestrs-cli")` at test time, surviving any future binary renames without source edits.

## [0.4.0] - 2026-08-26

### Added

- **Safe-by-default HTTP stack**: `catch-panic` middleware and request body limits are enabled by default; framework errors no longer leak internal details in production mode (`disable_production_errors` opt-out).
- **Bound-parameter SQL** for `nestrs-prisma`: `prisma_query_rows!` / `prisma_query_scalar!` / `prisma_execute!` macros bind every placeholder through SQLx (injection-safe by construction), plus `PrismaService::pool()` for advanced hand-bound queries.
- **DI lifecycle hooks**: `on_application_bootstrap` now runs automatically inside every `listen*` call, enabling self-wiring provider setup without a `MicroserviceApplication`.
- **GraphQL query limits helper** (`nestrs::graphql::with_default_limits`: depth 64 / complexity 512) for one-line schema hardening.
- **cargo-fuzz targets** for parser-heavy surfaces.
- **Scheduled dependency auditing**: the `security.yml` cargo-audit job now also runs weekly and on demand, so RustSec advisory drift is caught between pushes.

### Fixed

- **Scheduler: sub-second `#[interval(ms)]` jobs silently died** after 1–2 ticks. Root cause was `tokio-cron-scheduler` truncating repeat periods through `Duration::as_secs()`; interval jobs are now driven by native tokio timers with missed-tick skipping, deterministic shutdown, and a regression test asserting sustained tick progress.
- **OpenAPI path parameters were non-compliant**: specs emitted `:name` segments and no `parameters` arrays; paths now convert to `{name}` templates with proper `parameters` entries (OpenAPI 3.1).
- **`nestrs-microservices` `redis` feature did not compile standalone** (missing `dep:uuid` after the correlation-id change).

### Security

- Lockfile bumps: `crossbeam-epoch` 0.9.20 (RUSTSEC-2026-0204), `h2` 0.4.19 (RUSTSEC-2026-0258), `quinn-proto` 0.11.17 (RUSTSEC-2026-0185), un-yanked `spin` 0.9.9. `cargo audit` reports zero vulnerabilities.
- Kafka TLS no longer depends on the unmaintained `rustls-pemfile` crate; CA PEMs are parsed via `rustls::pki_types::PemObject`.
- `nestrs-prisma` macros migrated from the unmaintained `paste` crate to the maintained fork `pastey`.

### Changed

- **`HttpException` is now lint-clean in user handlers**: the rarely-populated `details` payload is boxed (`Option<Box<serde_json::Value>>`) across `HttpException`, `microservices::TransportError`, and the microservice wire types, keeping the error variant below `clippy::result_large_err`'s 128-byte threshold. Code that only reads or constructs details via `with_details(...)` / JSON indexing is unaffected; direct field literals like `details: Some(v)` need `Some(Box::new(v))`. The same fix applies to every nestrs application, not just this workspace.
- Workspace and crate versions aligned to `0.4.0` (the scheduler rewrite changed public API shape).

## [0.3.8] - 2026-04-17

### Added

- **NestJS migration guide** (mdBook `docs/src/nestjs-migration.md`), served on the docs site at `/docs/nestjs-migration` (legacy URL `/docs/migration/nestjs-to-nestrs` redirects), linked from the root README and website sidebar.
- **Secure defaults checklist** (`docs/src/secure-defaults.md`) and **HTTP pipeline ordering** (`docs/src/http-pipeline-order.md`) in mdBook; `SECURITY.md` expanded for CORS + CSRF runtime warnings.
- **Runtime `tracing` warnings** when cookies or in-memory sessions are enabled without CSRF wiring (or without the `csrf` feature).
- **CI job step** `Extension crate integration smoke` running targeted `nestrs` integration tests for OpenAPI, GraphQL, WebSockets, and TCP microservices.
- **Ordering contract tests** (`nestrs/tests/cross_cutting_ordering_contract.rs`) locking guard, interceptor, and route filter sequencing; `impl_routes!` rustdoc updated accordingly.

### Security

- `#[dto]` now applies `#[serde(deny_unknown_fields)]` by default so extra JSON keys fail deserialization; use `#[dto(allow_unknown_fields)]` to opt out.

### Fixed

- Integration tests that rely on `RouteRegistry` / `MetadataRegistry` use shared `RegistryResetGuard` + `serial_test` where needed to avoid order-dependent failures under parallel `cargo test`.

### Changed

- `nestrs new --strict` now prepends `#![deny(unsafe_code)]` instead of a redundant `#[serde(deny_unknown_fields)]` before `#[dto]` (DTO unknown fields are enforced by the macro).
- Workspace and crate versions aligned to `0.3.8` for crates.io publish.

## [0.3.7] - 2026-04-16

### Changed

- Workspace and crate versions aligned to `0.3.7` for crates.io publish.

### Fixed

- `nestrs-prisma` macro internals now treat integer primary keys (`id: i8/i16/i32/i64/u8/u16/u32/u64`) as auto-generated in create/createMany paths, avoiding insert-shape mismatches after widening native integer mappings beyond `i64`-only assumptions.
- `nestrs-prisma` integration coverage now includes an `id: i32` CRUD path to guard against future Prisma/Postgres `INT4` model regressions.

## [0.3.6] - 2026-04-16

### Changed

- Workspace and crate versions aligned to `0.3.6` for crates.io publish.

### Fixed

- `nestrs-prisma` README now documents required optional app dependencies for generated native types (for example `rust_decimal`, `ipnetwork`, and `bit-vec`) so consumer apps can compile generated bindings without guesswork.
- `nestrs-prisma` codegen now treats plain Prisma `DateTime` as provider-aware by default (`chrono::NaiveDateTime` for PostgreSQL/MySQL/SQLite), preventing `TIMESTAMP` vs `TIMESTAMPTZ` decode mismatches when native `@db.Timestamp(...)` is omitted.

## [0.3.5] - 2026-04-16

### Fixed

- `nestrs-prisma` codegen now maps Prisma `DateTime @db.Timestamp(...)` (timestamp without time zone) to `chrono::NaiveDateTime` to match Postgres `timestamp without time zone` columns.
- `nestrs-prisma` codegen now maps Prisma/Postgres native scalar widths more accurately (including `Int`/`BigInt`, `Real`/`DoublePrecision`, `Decimal`, `DateTime` native variants, network/native string types, and scalar lists) to avoid SQLx decode mismatches between generated Rust types and database column types.

## [0.3.4] - 2026-04-15

### Changed

- Workspace and crate versions aligned to `0.3.4` for crates.io publish.

## [0.3.3] - 2026-04-14

### Fixed

- `nestrs-prisma` now targets a concrete SQLx backend (`sqlx-sqlite` / `sqlx-postgres` / `sqlx-mysql`) instead of hardcoding `sqlx::Any`, restoring typed scalar compatibility for generated `DateTime`, `Json`, and similar fields.

## [0.3.2] - 2026-04-14

### Fixed

- `nestrs-prisma` schema bridge now supports additional Prisma scalar generation (`DateTime`, `Json`, `Bytes`) and generates clearer skip-reason comments for unsupported fields.
- `nestrs-prisma` schema bridge now emits Prisma enums/composite types and broader native type mappings in generated Rust bindings.

## [0.3.1] - 2026-04-14

### Fixed

- `nestrs-prisma` schema bridge now generates a valid `relation_schema()` function instead of an invalid top-level `let` binding in generated bindings.
- `nestrs-prisma` quickstart/readme guidance improved for crate consumers running examples outside this monorepo.

## [0.3.0] - 2026-04-14

### Added

- Full documentation surface expansion across all sidebar entries with practical examples.
- Next.js docs experience upgrades: unified shadcn-based UI primitives, improved theming, and polished navigation/search interactions.

## [0.1.3] - 2026-04-11

### Added

- **`nestrs-scaffold`**: `generate resource` / `generate resources` scaffolds full **CRUD** examples per transport — **REST** (`#[routes]` + JSON), **GraphQL** (Query/Mutation + `SimpleObject` rows), **WebSockets** (`#[ws_routes]` / `subscribe_message`), **TCP microservice** and **gRPC** (`#[micro_routes]` / `message_pattern` + HTTP health). Shared in-memory `Service` + DTOs across transports.

## [0.1.2] - 2026-04-11

### Added

- Dedicated **`README.md`** for each published crate with install snippets and examples; each package’s `readme` in `Cargo.toml` points at its own file so [crates.io](https://crates.io) shows crate-specific documentation instead of the workspace root README.
- `publish-crates` workflow: **GitHub Release** job after successful tag publish (with generated release notes).

### Fixed

- Rustdoc and Clippy issues affecting `lint-and-docs` CI (private intra-doc links, redundant links, duplicated `cfg` attrs, format/clippy lints).

## [0.1.1] - 2026-04-11

### Added

- `nestrs-prisma`: `PrismaService::query_all_as`, `execute`; crate `README.md`; Prisma model / SQLx workflow docs.
- `nestrs`: `microservices-metrics` feature; prelude re-exports for Kafka connection/SASL/TLS helpers and MQTT socket/TLS options.
- `nestrs-graphql`: `limits` module (`with_default_limits`, default depth/complexity constants, `Analyzer` re-export).
- `nestrs-macros`: `#[dto]` mappings for `Min` / `Max` / `IsUrl` / `ValidateNested`; Nest-like markers stripped for `IsInt` / `IsNumber` / `IsOptional`.

### Fixed

- `nestrs-microservices`: resolve `rumqttc::Transport` vs crate `Transport` trait name clash in MQTT live transport.

### Changed

- `nestrs-openapi`: default OpenAPI `info.version` uses `CARGO_PKG_VERSION` (stays aligned with the published crate).

## [0.1.0] - 2026-04-09

### Added

- Initial public workspace with `nestrs`, core/runtime crates, macros, CLI, and parity extensions.
- Nest-like module/controller/provider model with Axum/Tower runtime wiring.
- DTO validation, Prisma integration, security runbook, microservices guidance.
- Performance hardening pipeline with benchmark/reporting workflows.
