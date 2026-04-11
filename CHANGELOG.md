# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Landing page and documentation hub under `website/` with light/dark mode.
- Contribution and release process docs (`CONTRIBUTING.md`, `RELEASE.md`).
- Phase 5 hardening optional extension artifacts and maintenance helpers.

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
