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
- DI + application context
- DTO validation pipeline and class-validator-style ergonomics
- Cross-cutting pipeline: guards, pipes, interceptors, exception filters, strategies
- Production controls: backpressure, metrics, request tracing, security runbooks
- Performance hardening workflows: benchmark gating (HTTP, DI, validated JSON), history tracking, dashboard artifacts; scheduled libFuzzer smoke runs

## Ownership and release

- Maintainer / code owner: @Joshyahweh
- Current workspace version: `0.2.0` (from `VERSION` and workspace package settings)
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
- `nestrs-cli/` - scaffold/generate CLI (crates.io package name: **`nestrs-scaffold`**, binary: `nestrs`)
- `nestrs-prisma/` - Prisma integration crate
- `nestrs-microservices/` - transport/client/event primitives
- `nestrs-openapi/`, `nestrs-graphql/`, `nestrs-ws/` - parity extension crates
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
