# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.6] - 2026-04-16

### Changed

- Workspace and crate versions aligned to `0.3.6` for crates.io publish.

### Fixed

- `nestrs-prisma` README now documents required optional app dependencies for generated native types (for example `rust_decimal`, `ipnetwork`, and `bit-vec`) so consumer apps can compile generated bindings without guesswork.

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
