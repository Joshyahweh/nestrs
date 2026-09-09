# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — Wave 4.2: GraphQL federation gateway (`graphql-federation-gateway` feature)

- **Lightweight Apollo Federation gateway** in `nestrs-graphql::federation`,
  behind a default-off `graphql-federation-gateway` feature on `nestrs`.
  Stitches subgraph SDLs behind a single Axum endpoint and exposes the
  federation introspection shape:
  - `_service { sdl }` — the merged federation SDL (federation-v2 `@link`
    directive + `_Entity` / `_Any` plumbing), so an Apollo Router / GraphOS
    router in front of the gateway can introspect the stitched shape.
  - `entities(representations: [_Any!]!) -> [_Any]` — dispatch by
    `__typename` to a user-supplied resolver closure on each
    `SubgraphSpec`. Grouped by `__typename` and dispatched as a batch
    (Apollo Federation semantics — one call per typename per request,
    not one per representation, so DataLoader-style batching in the
    resolver actually works).
  - SDL validated at construction time via
    `async_graphql_parser::parse_schema`; refuses to start with
    `FederationError::Parse { subgraph, .. }` on bad SDL,
    `FederationError::NoSubgraphs` on empty config, or
    `FederationError::Merge { typename }` on conflicting dispatch table
    entries.
- **Row-level authorization flows through the gateway hook.**
  `federation_router_with_hook(cfg, "/graphql", Arc<dyn GqlHandlerHook>)`
  mirrors `graphql_router_with_hook` — the hook wraps
  `schema.execute_batch`, so entity resolvers run inside the hook's
  scope and ambient `Ability` / `Principal` / `TransactionSlot` task-locals
  (Wave 3A + 3D) are visible to user closures unchanged.
- **Public API surface (re-exported under `nestrs::graphql::federation::`):**
  `SubgraphSpec { name, sdl, entity_resolver }`,
  `FederationConfig { subgraphs, options, hook }`,
  `FederationError`, `EntityResolver`, and the trio of
  `federation_router` / `federation_router_with_options` /
  `federation_router_with_hook` entry points.
- **13 tests** in `nestrs/tests/graphql_federation_gateway.rs`:
  two-subgraph round-trip + routing; `_entities` dispatch with
  null-on-unknown-typename + batched multi-rep resolution; federation
  SDL export shape + directive stripping; construction-time refusal on
  bad SDL / empty list / merge conflict; row-level predicate survival
  through the hook; ability-scoped routing.
- **Out of scope** (documented in module-level docs): no query planner,
  no Apollo Router wire protocol, no automatic merging of overlapping
  subgraph types — "stitch" here means validate + dispatch + expose.
  Place this gateway behind Apollo Router / GraphOS for the externally
  routed case.

### Added — Wave 3F: validator 0.21 + `#[dto]` schemars reflection

- **Validator 0.20 → 0.21** across `nestrs`, `examples/hello-app`, and
  `examples/lab`. One breaking change surfaced in our usage: validator
  0.21 dropped the `uuid` built-in, so `#[dto]`'s `IsUUID` marker is now a
  type-level no-op (like `IsString`) — express UUID-ness via the
  `uuid::Uuid` type. All other emitted `#[validate(...)]` attrs (email,
  length, range, url, nested, contains, regex) compile unchanged.
- **`#[dto]` derives `schemars::JsonSchema`** (Yoann `#[input]` parity):
  the generated struct now carries serde + validator + JSON Schema in one
  decorator. `nestrs` re-exports `schemars` (`nestrs::schemars`) for
  `schema_for!` use; the derive expansion references the `schemars` path,
  so crates using `#[dto]` need `schemars = "1"` as a direct dependency
  (same as `validator`). Field-level `#[serde(rename)]` is reflected in
  the schema; nested `#[dto]` types come through as `$ref` + `$defs`
  chains.
- **`nestrs-openapi` consumes the schemas**: `OpenApiOptions::schemas`
  (merged into `components.schemas`) + the `schema_entry::<T>()` helper
  and `OpenApiOptions::with_schemas` builder. OpenAPI 3.1
  `components.schemas` values are JSON Schema documents, so
  `schema_for!` output drops in unchanged. 6 tests in
  `nestrs/tests/dto_schemars.rs` (schema round-trip, serde rename,
  nested `$ref` chain, `allow_unknown_fields` variant, validator 0.21
  smoke, `components.schemas` integration).

### Added — Wave 3B: OAuth2 (`nestrs-oauth2`) + Object Storage (`nestrs-storage`)

- **`nestrs-oauth2`**: new workspace member implementing the four OAuth2
  surfaces Yoann ships as separate crates, as one feature-gated crate:
  - `client` — `OAuth2Client` with authorization_code (+ PKCE S256),
    client_credentials, and refresh_token grants; state-parameter CSRF
    protection and token caching.
  - `resource-server` — `JwtVerifier` + `JwksCache` with JWKS rotation
    re-fetch, audience/issuer pinning, and clock leeway.
  - `social` — thin provider wrappers for Google / GitHub / Microsoft /
    Apple over the client.
  - `guard` — `OAuth2Guard` + `OAuth2Module` + the
    `install_oauth2_middleware` bridge, so a validated bearer populates
    the ambient `Principal` and downstream `Ability` checks proceed
    unchanged.
  Feature flags (all default off): `client`, `resource-server`, `social`,
  `guard`; the main crate opts in via `nestrs/oauth2` and
  `nestrs/authn-oauth2` (pre-wires the middleware). 40+ tests in-crate and
  in `nestrs/tests/oauth2_integration.rs`.
- **`nestrs-storage`**: new workspace member with a single `Storage` trait
  over Local filesystem / S3 / GCS / Azure Blob backends (thin newtypes
  around `object_store` adapters, hand-rolled filesystem backend). Includes
  `presign_get` / `presign_put` helpers and the `upload_to` helper consumed
  by the `#[upload_to("bucket", "prefix/{id}")]` decorator, which resolves
  the key template and stuffs the resulting key into the request context.
  Feature flags: `local` (default), `s3`, `gcs`, `azure`, `all`. 27 tests.

### Added — Wave 3C: full MCP protocol surface (`nestrs-mcp`)

- **Prompts / resources / resource templates**: `McpSurfaces` builder adds
  `register_prompt`, `register_resource`, `register_resource_template`
  (URI-templated resources with parameter binding), and
  `register_complete` for argument completion, alongside the existing
  tool registry.
- **Subscriptions**: `register_subscribable_resource` wires
  `resources/subscribe` + `list_changed` notifications.
- **Elicitation (SEP-1034)**: `NestrsMcpServer::elicit` +
  `register_elicitation` let a tool ask the user a question mid-flight;
  gated behind the new `elicitation` feature (pulls rmcp's `elicitation`
  feature).
- **MRTR (SEP-2322)**: multi-round tool refinement helpers so a tool can
  return an intermediate result and carry server-side state across rounds.
- **Tasks (SEP-2663)**: opt-in `tasks/get` / `tasks/update` / `tasks/cancel`
  extension advertised in `get_info`; in-flight task registry with
  client polling.
- **Cache hints + structuredContent**: `with_cache_hints` attaches TTL /
  cache-scope metadata to listings, and tool responses carrying
  `structured_content` are now included in the outbound masking walk
  (`mcp_data_context` masks both the text block and the structured
  payload). 140 tests under `--features "authz,authz-row-level,elicitation"`.

### Added — Wave 3E: production ops polish

- **`#[throttle(n, "second"|"minute"|"hour")]` / `#[skip_throttle]`**
  decorators + `ThrottlerModule` / `use_throttler` /
  `ThrottlerGuard` (Nest `@nestjs/throttler` parity): per-route specs
  override the global limit, `skip_throttle` exempts a route entirely, 429s
  carry `Retry-After` + `X-RateLimit-Limit` + `X-RateLimit-Remaining`.
  Backends: sharded poison-tolerant `InMemoryThrottler` (default) and
  cross-process `RedisThrottler` behind `cache-redis` (atomic INCR+EXPIRE
  Lua with TTL heal, fail-open on backend unavailability). Decorator
  metadata is enforced by the `use_throttler` middleware; the global spec
  is optional (`global: None` ⇒ only decorated routes throttle).
- **`#[liveness]` / `#[readiness]` / `#[startup]` probe decorators** +
  standard indicator set (NestJS terminus parity): stamping a route handler
  with `#[liveness]` / `#[readiness]` / `#[startup]` records probe metadata
  in the standard route pipeline, and `build_router` mounts three fixed
  server-root endpoints — `GET /__nestrs/health/live`, `/ready`,
  `/startup` — that mirror the stamped handler's status (2xx ⇒ up, any
  failure ⇒ 503; endpoints default to up with no stamp). `/ready` falls
  back to aggregating the `enable_readiness_check` indicators when no
  handler is stamped; `/startup` evaluates once per process and serves the
  cached result thereafter. Mirroring is an internal self-request through
  the completed router, so stamped handlers run with their real
  extractors, guards, and middleware. The indicator trio implement the
  existing `HealthIndicator` trait: `DatabaseIndicator` (over the shared
  `DatabasePing` capability, no feature gate), `HttpIndicator`
  (feature `http-client`), and `DiskSpaceIndicator` (new `health-disk`
  feature, unix, `statvfs`-backed).
- **W3C trace context ambient accessors** (`nestrs-core/src/trace.rs` +
  `nestrs::trace_context`): `traceparent` / `tracestate` parsed once and
  installed as an ambient task-local, visible from HTTP, WS, GraphQL, and
  MCP handlers (`nestrs::with_trace_context` / current accessors), not
  just the OTel SDK propagator.
- **Namespaced config** (`NESTRS_<NS>__<KEY>`): `#[config(namespace =
  "db")]` attribute derives `Deserialize` + `Validate` + the
  `ConfigNamespace` marker; `ConfigModule::for_root(vec![
  Config::register::<T>(), ...])` parses each namespace from the env
  overlay with per-namespace isolation, validates at boot (invalid env
  fails startup), honors `NESTRS_ENV_PREFIX`, and exports a typed
  `ConfigService::get::<T>()`. The overlay cascade is
  `.env` → `.env.{env}` → `.env.{env}.local` → process env (highest
  precedence) and never mutates the process environment, keeping config
  loading deterministic under parallel tests.
- **WS RFC 6455 close codes + runtime per-message guard chain**
  (`nestrs-ws`): `CloseCode` enum (1000/1001/1003/1008/1011/1012/1013 +
  4000–4999 application range, with `as_u16` / `from_u16`) and
  `WsClient::close(code, reason)`. `serve_socket` now maps failures to
  coded closes after the usual `error` frame: malformed wire payload →
  **1003** UnsupportedData, runtime guard rejection → **1008**
  PolicyViolation (status ≥ 500 → **1011** InternalError), handler panic
  → **1011** (caught via `catch_unwind`). New object-safe
  `WsMessageGuard` trait + `WsGuardChain` run per-message in the shared
  runtime via the new `WsGateway::message_guards()` hook (default empty —
  opt-in defense in depth on top of `#[use_ws_guards(...)]`; blanket-impl'd
  over `WsCanActivate`). Upgrade-time authorization via
  `ws_route_with_guards(gateway, Vec<Arc<dyn WsUpgradeGuard>>)` —
  rejections accept the upgrade then immediately close with 1008, and any
  `WsCanActivate` guard doubles as an upgrade guard (empty event, null
  payload). 9 end-to-end tests over a real loopback axum server +
  tokio-tungstenite client.

### Added — Wave 3D: row-level authorization (`authz-row-level` feature)

- **Closure row predicates**: `AbilityBuilder::can_with_predicate(action,
  subject_type, fields, predicate)` accepts any closure
  `Fn(&serde_json::Value, &Principal) -> bool` (via the new `RowPredicate`
  trait, blanket-impl'd over `Fn`). Predicates evaluate in `Ability::can`
  for `Subject::Instance` checks against the ambient principal, and in the
  repository post-load. `Rule` gains a `predicate` field (breaking for
  exhaustive `Rule` literals — pre-1.0); `Rule`/`Ability` render
  `predicate: true|false` in `Debug` instead of serializing the closure.
- **`Repository::find_many_authorized(action, FindManyParams)`** under
  `authz-row-level`: limit/offset pagination, caller-supplied
  `extra_where`/`extra_binds` that interleave with policy binds, and
  per-request SQL pushdown of declarative conditions + prebuilt predicate
  conditions. `find_one_authorized`/`find_all_authorized` are retrofitted
  to also post-filter rows through the rule's predicate (leak prevention).
- **10 pre-built predicates** (`nestrs::predicates`, root re-exported):
  `AuthorIsCurrentUser`, `BelongsToUser`, `WithinTenant`, `OwnerOrAdmin`,
  `TenantOrAdmin`, `SelfOrAdmin`, `PublishedOnly`, `NotDeleted`,
  `PublicOrOwner`, `HasRole`. Identity-shaped prebuilts push down a
  `json_extract` WHERE clause per request; OR-shaped ones push down only
  for non-admin principals; post-filter-only ones document it.
- **Mandatory CrudService enforcement** (no opt-out, no `skip_auth`): every
  `create`/`read`/`update`/`delete`/`list` call requires an `Ability` in
  request scope (deny-closed `Protocol` error otherwise), applies the rule
  predicate on create-candidate / current / replacement rows, and fetches
  through the mutation's own action (invisible rows read as `Ok(None)` /
  `Ok(false)`, denied writes as `Err(Protocol("policy denied: …"))`.
  `repo_crud_create` is now an ability-free direct INSERT seeding primitive.
- **Ambient principal plumbing**: `PRINCIPAL_SLOT` task-local in
  `nestrs-core` (mirror of the ability slot), `nestrs::with_principal` /
  `nestrs::policies::current_principal` (module-qualified — `nestrs::Principal`
  stays the authn extractor), `From<PrincipalIdentity>` under `authn`, and
  principal installation in `install_authn_middleware` plus the WS /
  GraphQL / MCP data contexts (`current_ws_principal`,
  `current_gql_principal`, `current_mcp_principal`).
- **`nestrs-mcp` mirror feature** `authz-row-level` for multi-transport
  tests. 40+ new tests across `predicates_module`, `row_level_repository`,
  `crud_service_row_level`, `row_level_http` (full JWT → middleware →
  CrudService stack), and GQL/WS/MCP scope-survival suites. Known
  limitation: `json_extract` pushdown is SQLite-flavored under `AnyPool`
  (inherited from `conditions_to_sql`); dialect-aware rendering is a
  follow-up.

## [0.5.2] - 2026-09-04

### Changed

- Workspace and crate versions bumped 0.5.1 -> 0.5.2. The 0.5.1 publish
  was unable to land `nestrs-mcp` and `nestrs-scaffold` to crates.io
  (10 of 12 crates made it; the remaining two were blocked by a
  crates.io 400 from the 22-char `model-context-protocol` keyword in
  `nestrs-mcp` and a flaky preflight test gating the publish job).
  v0.5.2 ships the keyword fix and retries past the flake.

> Note: v0.5.0 and v0.5.1 are both partial releases on crates.io. v0.5.2
> is the first complete 0.5.x release and the recommended upgrade for
> anyone who pinned to either partial version. v0.5.0 and v0.5.1
> crates remain on crates.io for compatibility.

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
