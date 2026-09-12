# Stability and public API policy

This document applies to the **`nestrs` workspace** crates published on crates.io, primarily:

- **`nestrs`** — application entrypoint, `NestFactory`, HTTP stack, feature-gated modules  
- **`nestrs-core`** — DI container, `RouteRegistry`, `MetadataRegistry`, guards/pipes traits  
- **`nestrs-macros`** — proc macros (`#[module]`, `#[controller]`, `#[routes]`, `#[get]`, …)  
- Extension crates (`nestrs-openapi`, `nestrs-graphql`, `nestrs-ws`, `nestrs-microservices`, …) follow the same semver spirit; see each crate’s changelog.

## Semantic versioning (semver)

We follow **[Cargo’s semver](https://doc.rust-lang.org/cargo/reference/semver.html)** for published versions:

- **MAJOR** — breaking public API changes, or macro expansions that require **source changes** in typical consumer code (e.g. renamed public items, removed features, different trait bounds on public types).  
- **MINOR** — new functionality in a backward-compatible way: new optional features, new public types/functions, expanded macro **accepted** syntax (old code still compiles).  
- **PATCH** — bug fixes, docs, compatible dependency bumps that do not change the public contract.

**Proc macros:** A change that only affects **generated** code (e.g. more efficient expansion) without changing **documented** macro input syntax is usually **PATCH** or **MINOR**. A change that **breaks** existing macro invocations (attributes, token patterns) is **MAJOR**.

**1.0 and beyond:** the public API is covered by full semver — breaking changes ship only in a new **MAJOR**. Before 1.0, minor releases (`0.y`) could include breaking changes per common Rust ecosystem practice; every such transition is documented in `CHANGELOG.md`. The `0.5.x → 1.0.0` jump is itself the major bump that legitimizes the last pre-1.0 breaking changes (e.g. `CacheOptions::InMemory` gaining `max_entries`, `Subject::Type` moving from `&'static str` to `String`) — migration notes are in their `CHANGELOG.md` entries.

## What counts as public API

**Stable (semver-protected):**

- All **documented** items in `nestrs` and `nestrs-core` public modules on [docs.rs](https://docs.rs) (types, traits, fns, features).  
- **Macro invocation forms** shown in the book, READMEs, and rustdoc (e.g. `#[module(controllers = [...], ...)]`).  
- **Cargo features** listed in each crate’s `Cargo.toml` (enabling/disabling features should not break default builds across **PATCH**).

**Not stable / implementation detail:**

- **`#[doc(hidden)]`** items (including `__nestrs_*` helpers and macro-internal exports). These may change in any release; do not depend on them outside the `nestrs` repo.  
- **Private modules** and any type or function not reachable from the public crate root without `pub use`.  
- **Relying on exact generated code** (private struct names, closure shapes) from proc macros — only the **observable behavior** of the public API is stable.  
- **Test-only hooks** (see below).

## Process-global state

`nestrs-core` keeps a few **process-wide** registries (for example **`RouteRegistry`** for OpenAPI path discovery and **`MetadataRegistry`** for handler metadata). This matches a single long-lived server process and is **not** thread-local.

**Implications:**

- **Unit and integration tests** in the **same process** may see **accumulated** routes/metadata if controllers are registered repeatedly.  
- Parallel tests can interleave registrations; prefer **deterministic** assertions or isolation strategies.

### `test-hooks` feature (tests only)

The optional feature **`test-hooks`** on **`nestrs-core`** / **`nestrs`** exposes:

- `RouteRegistry::clear_for_tests()`  
- `MetadataRegistry::clear_for_tests()`  

**Do not enable `test-hooks` in production binaries.** Clearing registries at runtime would break OpenAPI and metadata-driven guards.

CI runs `cargo test --workspace --all-features`, which enables `test-hooks` for the `nestrs` crate so integration tests can reset state. The dedicated **`integration_matrix`** test target requires `openapi`, `csrf`, and `test-hooks`.

## Performance gates and fuzzing

- **Criterion** micro-benchmarks for HTTP hot path, middleware stack, **DI resolution**, and **validated JSON** bodies are run in **`.github/workflows/performance.yml`** (see **`PRODUCTION_RUNBOOK.md`** and **`benchmarks/thresholds.json`**).
- **libFuzzer** jobs in **`.github/workflows/fuzz.yml`** exercise JSON wire types, `Authorization` parsing, and URI/JSON decode boundaries (see **`nestrs/fuzz/`** and **`nestrs-microservices/fuzz/`**). Fuzz failures indicate a crash or abort to investigate; they are not a semver API contract.

## Public-API snapshot test (CI gate)

The **`public-api-snapshot`** job in **`.github/workflows/ci.yml`** regenerates `tests/api-snapshots/<crate>.txt` for each published crate (`nestrs`, `nestrs-core`, `nestrs-graphql`, `nestrs-ws`, `nestrs-mcp`) by reading `rustdoc --output-format json` on the nightly toolchain (rustdoc JSON is nightly-only), and fails the build on drift from the committed snapshots. The extractor (`scripts/ci/public_api_snapshot.py`) filters out `#[doc(hidden)]` items and `__nestrs_*` macro-internal helpers — both are explicitly **not** stable per this document.

**Escape hatch (intended for non-breaking expansion):** edit the affected snapshot file directly and commit alongside the code change, with a one-line note in the PR description explaining the additions.

## Where to report breakage

If a **minor** or **patch** release breaks your build without a changelog entry, open an issue with the crate version, rustc version, and a minimal reproducer.
