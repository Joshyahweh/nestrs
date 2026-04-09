# nestrs

NestJS-like API framework for Rust built on Axum and Tower.

`nestrs` gives you a familiar module/controller/provider mental model with Rust performance and explicit typing.

## Highlights

- Module-oriented architecture (`module`, `controller`, `injectable` macros)
- HTTP route macros (`get`, `post`, `put`, `patch`, `delete`, `options`, `head`, `all`)
- DI + application context
- DTO validation pipeline and class-validator-style ergonomics
- Cross-cutting pipeline: guards, pipes, interceptors, exception filters, strategies
- Production controls: backpressure, metrics, request tracing, security runbooks
- Performance hardening workflows: benchmark gating, history tracking, dashboard artifacts

## Ownership and release

- Maintainer / code owner: @Joshyahweh
- Current workspace version: `0.1.0` (from `VERSION` and workspace package settings)
- Release notes template: `.github/release-template.md`
- Changelog: `CHANGELOG.md`
- Contribution guide: `CONTRIBUTING.md`
- Release process: `RELEASE.md`
- Code of conduct: `CODE_OF_CONDUCT.md`
- Security disclosure policy: `SECURITY.md`

## Project Layout

- `nestrs/` - main framework crate (public runtime API)
- `nestrs-core/` - runtime primitives (context, traits, metadata, strategy)
- `nestrs-macros/` - proc macros and helper attributes
- `nestrs-cli/` - scaffold/generate CLI
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
- `nestrs-plan-2.md` - full framework roadmap and parity plan
- `HARDENING_STATUS.md` - implementation checkpoint summary
- `PHASE5_OPTIONAL_CLOSEOUT.md` - optional hardening closeout

### Platform/operations

- `PRODUCTION_RUNBOOK.md` - deployment/operations runbook
- `SECURITY.md` - security guidance and controls
- `MICROSERVICES.md` - microservices/event-driven patterns

### Performance and benchmark ops

- `benchmarks/BASELINE.md` - how to run, compare, and track benchmarks
- `benchmarks/relative_thresholds.json` - active relative regression gate config
- `benchmarks/recommended_relative_thresholds.md` - helper recommendation output
- `benchmarks/threshold_reassessment_status.md` - readiness/status for next threshold retune

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
- `.github/workflows/performance.yml` - performance benches, gating, reporting, optional publishing
- `.github/workflows/benchmark-storage-sync.yml` - storage sync template for S3/GCS/Azure
- `.github/workflows/release-version-check.yml` - enforces `VERSION` and latest `CHANGELOG.md` release heading stay in sync

## GitHub community templates

- `.github/ISSUE_TEMPLATE/bug_report.yml`
- `.github/ISSUE_TEMPLATE/feature_request.yml`
- `.github/ISSUE_TEMPLATE/config.yml`
- `.github/pull_request_template.md`

## Status

The tracked roadmap implementation is complete. Ongoing work is maintenance mode: periodic benchmark history accumulation and threshold re-evaluation when data changes.
