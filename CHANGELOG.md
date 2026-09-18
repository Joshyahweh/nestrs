# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.3.0] - 2026-09-17

Wave 9 NestJS gap close: rebuild what maps onto Axum, adapter crates for
the rest. Additive minor on the 1.2 contract.

### Added — rebuilds

- **`MiddlewareConsumer`** — Nest `configure(consumer)` analogue.
  `NestApplication::configure_middleware` applies an `apply_fn` to path
  prefixes (`for_routes` / `exclude`). Non-matching paths skip the function.
- **Admin Devtools HTML** — `GET /__nestrs` and `/__nestrs/devtools` on the
  admin sidecar (route table + JSON links). Nest Devtools analogue, not a
  full UI product.

### Added — adapter crates

- **`nestrs-socketio`** — Socket.IO via socketioxide (`@nestjs/platform-socket.io`).
- **`nestrs-lambda`** — AWS Lambda / API Gateway (`listen_lambda`).
- **`nestrs-better-auth`** — nest better-auth.rs Axum router + `BetterAuthGuard`.
- **`nestrs-bullmq`** — BullMQ Redis key-layout producer (Node workers can consume).
- **`nestrs-passport`** — `JwtStrategy` / `LocalBasicStrategy` on `AuthStrategy`.
- **`nestrs-saml`** — SP-initiated redirect + `SamlResponseValidator` trait.
- **`nestrs-ldap`** — LDAP simple bind (`passport-ldap` analogue).
- **`nestrs-sea-orm`** — SeaORM `for_root_async` (TypeORM/Sequelize analogue).

Header/media-type versioning already dispatches unversioned paths (1.2.0).
Fastify remains out of scope (ADR-0001, Axum-only). `nestrs-drizzle` stays
carved out.

### Docs

- rustdoc + mintlify synced to **1.3.0** (`html_root_url`, install snippets,
  MCP scaffold template).
- Migration / ecosystem pages cover `MiddlewareConsumer` and the eight
  adapter crates (`mintlify-docs/ecosystem/adapters.mdx`).

### Fixed

- **`cargo fmt --all`** so `ci / lint-and-docs` rustfmt check is clean.
- **`rustls` ≥ 0.23.45** (`RUSTSEC-2026-0285`: TLS 1.3 handshake messages
  accepted across encryption-level boundaries). Direct pin in
  `nestrs-microservices`; lockfile refresh covers transitive copies.
- **rustdoc** — `nestrs-passport` crate docs link `PassportGuard` instead of
  the unaliased `AuthStrategyGuard` (`-D rustdoc::broken-intra-doc-links`).
- **Fuzz CI** — install `cargo-fuzz` 0.13.2 with `--no-locked` so nightly
  does not compile rustix 0.36.5's reserved `rustc_*` attributes.

## [1.2.0] - 2026-09-17

Wave 8 NestJS-parity depth leftovers plus a production/security pass on
the new surfaces. Additive minor on the 1.0 / 1.1 contract.

### Changed

- **Crate rename `nestrs-throttler` → `nestrs-throttle`.** crates.io already
  has `nestrs-throttler` (yanked `0.1.0`, different owner). Types, macros,
  and umbrella features are unchanged (`#[throttle]`, `ThrottlerModule`,
  `throttler` / `throttler-redis`). Direct deps use
  `nestrs-throttle = "1.2.0"` / `use nestrs_throttle::…`.

### Added

- **`#[intersection_type]`** — NestJS `IntersectionType(A, B)` analogue.
  Parent DTO fields are `#[serde(flatten)]`'d into one JSON object with
  nested `Validate`. Parent DTOs must use `#[dto(allow_unknown_fields)]`.
- **`session-redis`** — `RedisSessionStore` + `NestApplication::use_session_redis`.
  Redis `SET`/`GET`/`DEL` with `PX` TTL. Session cookie is `Secure` in
  production. URL userinfo is redacted in `Debug`; session ids are never
  logged. Redis wins if both memory and Redis are configured.
- **`nestrs-mcp` stock binary tools** — `list_tools` / `get_tool` /
  `call_tool` dispatch to the four area handlers (rmcp routers cannot
  merge across handler types). The stock binary advertises the full
  24-tool surface.
- **`MongoModule::for_root_async`** and **`MongoService::model::<T>()`**
  (`@InjectModel` analogue over `for_feature`).
- **`nestrs-cli repl live`** — dump live providers + routes from a running
  admin sidecar. Bearer is a header only; curl argv uses `--` before the URL.

### Documented

- `queues-redis` is the multi-instance production queue path (Bull-style
  LPUSH/BRPOP, not the BullMQ protocol).
- Federation SDL export remains HTTP `_service { sdl }` or build-time
  `export_schema_sdl`; non-federation HTTP SDL is unsupported.
- **`nestrs-drizzle` stays carved out** of the workspace: unpublished
  `drizzle-orm 0.36` on crates.io, and crates.io `drizzle` 0.1.x requires
  MSRV 1.95 > workspace 1.88.

### Docs — rustdoc + mintlify synced to 1.2.0

- Workspace version, path-dep pins, crate READMEs, mintlify/mdBook
  install snippets, and `html_root_url` all read `1.2.0`.
- `publish-crates.yml` publishes all **20** workspace crates (including
  `nestrs-oauth2-macros`, `nestrs-security`, `nestrs-throttle`,
  `nestrs-health`, `nestrs-mongodb`, `nestrs-http`). `nestrs-drizzle`
  remains unpublished.
- Mintlify Wave 8 pages: introduction is 1.2; `use_session_redis` /
  `use_throttler` on NestApplication; Mongo `for_root_async` /
  `model::<T>()` examples; mapped-types / IntersectionType on
  validation + NestJS migration; `repl live` curl/bearer/`--` URL;
  federation HTTP SDL is subgraph-only; throttling guide matches
  `ThrottleSpec { limit, window_secs }` (no builder, no `window_ms`).

### Fixed

- **`nestrs-oauth2` keywords** trimmed to 5 (`oauth2`, `oidc`, `auth`,
  `nest`, `axum`). crates.io rejected 1.2.0 with HTTP 400 (`expected at
  most 5 keywords per crate`). Publish preflight now checks keyword and
  category limits.
- **`throttler_middleware`** honors `#[throttle]` / `#[skip_throttle]` on the
  app-level layer. Route-level `HandlerKey` is not available there, so the
  middleware resolves the handler via `RouteRegistry::handler_for` and applies
  the same `skip → decorated → global` precedence as `ThrottlerGuard`.
  `ThrottlerRequest.handler` is `&str` (tied to the request) rather than
  `&'static str`, so the looked-up handler id can be passed through without
  leaking.

## [1.1.0] - 2026-09-17

Wave 7 NestJS-parity surfaces plus the post-wave clippy/test gate and a
production/security pass on those new APIs. Additive minor: new crates
and features, no breaking changes to the 1.0.0 contract.

### Docs — rustdoc + mintlify synced to the shipped 1.1.0 API

- Workspace version, path-dep pins, crate READMEs, mintlify/mdBook
  install snippets, and `html_root_url` all read `1.1.0`.
- SSE docs use `serialize_to_event` (no `IntoSseEvent for T: Serialize`)
  and `from_stream` / `from_fallible_stream` as aliases over
  `Stream<Item = Result<Event, E>>`.
- ALS docs match tokio 1.51 `task_local!` (no `= const` initializer)
  and a single generic `FromRequestParts<S>` impl via the
  `nestrs_core::als::async_trait` re-export.
- Mongo recipe boot uses `DynamicModule::from_module::<MongoModule>()`
  plus `for_root` / `for_feature`, matching the `Module` impl.
- Umbrella `nestrs` re-exports `#[als]` and forwards `feature = "sse"`
  to `nestrs-core/sse`.
- Public-API snapshots updated: `nestrs-core` (ALS + SSE + client-ip),
  `nestrs-graphql` (federation v2 / SDL file export), `nestrs-oauth2`
  (password hashing). The `nestrs` snapshot follows current rustdoc
  JSON (cross-crate `pub use` items are attributed to the defining
  crate — `nestrs-health`, `nestrs-throttle`, `nestrs-mongodb`,
  `nestrs-http`, `nestrs-security` — and remain public re-exports).

### Fixed — production/security audit 2026-09-17 (Wave 7 surfaces)

- **`nestrs-cli graphql sdl` curl argv** — pass the endpoint after `--` so a
  URL that starts with `-` cannot be interpreted as extra curl flags.
- **`MongoOptions` Debug** — redacts `user:pass@` from the connection URI so
  traces / `dbg!` cannot leak database credentials.
- **`ThrottlerBackendKind::Redis` / `RedisThrottler` Debug** — same userinfo
  redaction; `RedisThrottler` no longer Debug-prints the `redis::Client`.

### Added — Wave 7.16: Async-local-storage `#[als]` proc-macro + `nestrs_core::als` runtime

A typed cell that propagates a value through every `.await` on the
current task without threading it through every function signature
— the Rust analogue of `nestjs-cls` / `cls-hooked`. Middleware
installs the value once, downstream handlers and services read it
without injecting it. The runtime is always-on (no feature gate);
the proc-macro is sugar over it that adds per-type generated names
and an axum extractor.

- **`nestrs_core::als` module** — always-on (no feature flag, since
  `tokio::task_local!` is already in scope via the existing `tokio`
  workspace dep). Three pieces of public surface:
  - `AlsError` — single-variant rejection enum
    (`NotSet`, no value installed on the current task).
    `Clone + Copy + PartialEq + Eq` so handler tests can
    `assert_eq!(res, Err(AlsError::NotSet))` without wrapping.
    `Display` impl names the fix in the failure message
    ("middleware should set it via `with_<name>(value, future).await`").
    `std::error::Error` impl.
  - `task_local!` — re-export of `tokio::task_local!` so the macro's
    emitted code has a stable path
    (`::nestrs_core::als::task_local!`) that doesn't require `tokio`
    to be a direct dependency of the user's crate (transitive
    visibility from `nestrs-core` isn't enough on its own).
  - `AlsContext<T>` — manual helper for users who don't want the
    proc-macro. `new(&cell)` constructor + `async fn with(value, future) -> R`
    + `fn current() -> Option<T>`. Wraps a `tokio::task_local!` cell
    with `RefCell<Option<T>>` storage, drops the manual `scope` /
    `try_with` plumbing. The macro is sugar over this surface; pick
    whichever fits your codebase.
- **`#[als]` proc-macro** in `nestrs-macros` — applied to a struct,
  generates alongside the original:
  - `task_local!` cell `<SNAKE_UPPER>_ALS: Option<Self>` (no
    `=` initializer — tokio 1.51 `task_local!` takes a type, not a
    value) — names derived from the type itself so two ALS values
    can't share state.
  - `with_<snake>(value, future) -> R` — install the value for the
    duration of `future`. After `future` completes (success, error,
    or panic), the prior value (or absence) is restored.
  - `current_<snake>() -> Option<Self>` — read the current value
    (where `Self: Clone`), returns `None` outside any active scope.
    Cheap (single task-local lookup + clone).
  - generic `impl<S> FromRequestParts<S> for Self` (any state type
    `S: Send + Sync`) via `#[::nestrs_core::als::async_trait]` —
    axum 0.7's extractor trait is `async_trait`-based; the re-export
    means callers don't add `async-trait` themselves.
    `Rejection = AlsError` (maps to HTTP 500).
  - Generic-aware — preserves `impl_generics`, `ty_generics`,
    `where_clause` through the generated impls.
  - Inline `to_snake_case` helper (handles `RequestContext` →
    `request_context`, `UserID` → `user_id`, `HTTPRequest` →
    `http_request`). No `heck` dependency added.
- **Bounded to `Self: Clone + Send + Sync + 'static`** — the value is
  stored by `task_local!`, returned by clone, and propagates across
  `.await` boundaries automatically. `task_local`'s own bounds cover
  the rest.
- **Tests** — 7 integration tests in `nestrs-core/tests/als.rs`
  (runtime: `with` installs value for future duration, nested
  scopes join the outer, `current` returns `None` off-scope, empty
  string round-trip, `Display` mentions the fix, `Eq` supports match
  arms) + 10 macro expansion tests in `nestrs-macros/tests/als.rs`
  (named-field struct, single-field struct, tuple struct, unit
  struct; `with_*` / `current_*` round-trip; nested scope joins;
  extractor reads from middleware-installed value; extractor
  rejects with `AlsError::NotSet` when not installed; extractor
  works for single-field / tuple / unit structs; two distinct ALS
  types don't interfere). 3 inline unit tests in
  `nestrs-core/src/als.rs` mirror the runtime assertions in
  isolation (incl. panic restore via `catch_unwind`).
- **Docs** — `mintlify-docs/concepts/als.mdx` (define an ALS type,
  install it from middleware, read it from a handler, read it from
  anywhere on the task, the runtime helper, `AlsError`, multiple
  ALS types, `tokio::task_local` re-export, "why these choices") +
  `docs.json` entry under `Core Concepts` next to `sse`.

### Added — Wave 7.15: Server-Sent Events (`nestrs-core`, feature `sse`)

A thin nestrs-flavored wrapper around axum's SSE primitive — the
missing runtime primitive for handlers that stream events to the
browser. Behind a feature flag so apps that *consume* SSE payloads
from upstream services don't pay the compile cost of `bytes` /
`futures-core` / `serde`.

- **`nestrs_core::sse` module** — gated on `feature = "sse"`. Re-exports
  `axum::response::sse::{Sse, Event, KeepAlive}` as `SseResponse`,
  `SseEvent`, `SseKeepAlive` so handlers don't have to depend on
  axum's SSE module directly. `SseResponse<S>` is a newtype around
  `Sse<S>` that implements `IntoResponse` (delegates to axum, so the
  `text/event-stream` content-type, chunked transfer encoding, and
  retry semantics stay identical).
- **Constructors** —
  `SseResponse::from_stream(stream)` and
  `SseResponse::from_fallible_stream(stream)` are aliases: both
  take `Stream<Item = Result<Event, E>>` where `E: Into<Box<dyn Error
  + Send + Sync>>` (axum 0.7's SSE contract — there is no infallible
  stream constructor). `.keep_alive(KeepAlive)` attaches a heartbeat
  policy, `From<Sse<S>> for SseResponse<S>` wraps an already-constructed
  `Sse<S>`, plus `into_inner` / `as_inner` accessors for axum-only APIs.
- **`IntoSseEvent` trait** — `&str` / `String` / `Bytes` / `Event`
  (passthrough). JSON payloads use the free function
  `serialize_to_event` instead of a `T: Serialize` blanket impl —
  `Event` itself implements `Serialize`, so a blanket impl would
  conflict with the passthrough. Serialization failures (e.g.
  `f64::NAN`) still emit a structured `event: error` rather than
  crashing the stream.
- **Feature flag** — `sse = ["dep:bytes", "dep:futures-core",
  "dep:serde"]`. Off by default. `nestrs-core`'s default feature set
  is unchanged — apps opt in only when they actually emit SSE.
- **Tests** — 17 integration tests in `nestrs-core/tests/sse.rs`
  (gated on `feature = "sse"`): content-type header is
  `text/event-stream`; single-event body ends with the SSE blank-line
  terminator; multi-event streams write each `data:` line in order;
  named events emit an `event:` field; retry field is serialized as
  `retry: 250`; `KeepAlive` attaches without breaking conversion;
  empty streams produce no `data:` lines; `IntoSseEvent` impls for
  `&str` / `String` / `Bytes` use the default event name;
  `serialize_to_event` uses `message`; serialization
  failure emits `error`; `Event` passthrough preserves name and
  data; `From<Sse<S>>` works; `into_inner` / `as_inner` return the
  inner `Sse<S>`; `from_fallible_stream` accepts a stream of
  `Result<Event, axum::Error>`. 5 inline unit tests in
  `nestrs-core/src/sse.rs` mirror the same assertions in isolation.
- **Docs** — `mintlify-docs/concepts/sse.mdx` (enable feature, return
  type, `IntoSseEvent` table with notes on each payload shape, error
  event semantics, fallible streams, `KeepAlive`, conversion from
  `Sse<S>`, accessors, "why these choices") + `docs.json` entry
  under `Core Concepts` next to `middleware-pipeline`.

### Added — Wave 7.14: Cookies + sessions + CSRF (umbrella `nestrs`, features `cookies` / `session` / `csrf`)

The cookies / sessions / CSRF surface was always in `nestrs/src/lib.rs`
behind three feature flags but lacked docs and tests. This wave keeps
the existing implementation (extracted from Wave 7.3's
`nestrs-security::csrf` module and the `nestrs/src/security` re-export)
and adds the documentation + test coverage to make it discoverable and
trustworthy. No breaking changes to the builder API.

- **`use_cookies()`** — installs `tower_cookies::CookieManagerLayer`
  so handlers can take a `tower_cookies::Cookies` extractor. Feature
  flag `cookies` (single dep on `tower-cookies`).
- **`use_session_memory()`** — installs
  `tower_sessions::SessionManagerLayer::new(MemoryStore::default())`
  on top of the cookie layer (session implies cookies). Feature
  flag `session` (`cookies` + `tower-sessions`). Production should
  swap the `MemoryStore` for a persistent backend before going
  multi-process.
- **`use_csrf_protection(CsrfProtectionConfig)`** — installs the
  double-submit middleware from `nestrs-security::csrf`. Layer
  ordering is CSRF *inside* `CookieManagerLayer` so the `Cookies`
  extractor is populated before the check. Feature flag `csrf`
  (`cookies` + `subtle`).
- **`CsrfProtectionConfig`** — `{ cookie_name: &'static str,
  header_name: HeaderName }`. Default `("csrf_token", "x-csrf-token")`.
  Override for upstream-named tokens (`X-XSRF-TOKEN`, etc.).
- **Footgun guard** — when `cookies` or `session` is enabled and
  `csrf` is not (either the feature flag or the builder call),
  `nestrs` emits a `tracing::warn!` at startup pointing at the
  `use_csrf_protection(...)` builder call. Cookie-authenticated
  browser clients without CSRF are forgeable on POST/PUT/PATCH/DELETE.
- **Docs** — `mintlify-docs/guides/cookies-sessions-csrf.mdx` (feature
  flags, builder calls, `Cookies` / `Session` extractors, CSRF token
  issuance + double-submit pattern, custom cookie/header names, the
  startup warning) + `docs.json` entry under `Guides` next to
  `guides/security`.
- **Tests** — `nestrs/tests/cookies_sessions_csrf.rs` (gated on
  `cookies` + `session` + `csrf`) complements the focused
  `csrf_middleware.rs`. 5 cases: `Cookies` extractor writes a
  `Set-Cookie`, `Session` extractor round-trips a typed value across
  requests, GET / HEAD bypass the CSRF check, and PUT / PATCH / DELETE
  are gated alongside POST.

### Added — Wave 7.13: Password hashing helpers + `#[derive(HashOnNew)]` (`nestrs-oauth2`, feature `password`)

Bcrypt / Argon2id hashing with prefix auto-detection on verify, plus
a proc-macro that hashes marked fields on row construction. The
small focused alternative to pulling in `bcrypt` + `argon2` directly
— pick a backend feature (or both), call `hash` / `verify`, or let
`#[derive(HashOnNew)]` do it for you.

- **Backends behind per-feature flags** —
  `password-bcrypt` (pure-rust bcrypt 0.15, `DEFAULT_COST` = 12),
  `password-argon2` (pure-rust argon2 0.5, `Argon2id` defaults:
  m = 19 456 KiB, t = 2, p = 1), `password-macros` (pulls in the
  new `nestrs-oauth2-macros` proc-macro crate), `password`
  umbrella pulling all three. Default feature set is unchanged —
  opt in explicitly.
- **`nestrs_oauth2::password`** — module gated on
  `any(password-bcrypt, password-argon2)` (one backend is enough):
  free functions `hash(plain) -> Result<String, HashError>`
  (defaults to `Backend::Argon2`), `hash_with(plain, backend)`,
  `verify(plain, hash)` (prefix-dispatch), `verify_any(plain, hash)`
  (same dispatch, public alias), `verify_with(plain, hash, backend)`;
  `Backend` enum (`Bcrypt` / `Argon2`, `Default = Argon2`) with
  `Display`; `PasswordHasher` trait + `BcryptHasher` / `Argon2Hasher`
  concrete impls (`Copy + Default + Debug`, slot into a DI registry
  as `Arc<dyn PasswordHasher>`); `HashError` (`BcryptDisabled` /
  `Argon2Disabled` for missing-feature paths, `SaltGeneration`,
  `InvalidHash` / `InvalidPassword`, `UnknownPrefix` for hashes
  that aren't `$2...` or `$argon2...`).
- **Prefix auto-detection in `verify` / `verify_any`** — bcrypt
  matches `$2` (covers `$2a` / `$2b` / `$2x` / `$2y`), argon2
  matches `$argon2` (covers argon2id / argon2i / argon2d). Lets
  legacy Bcrypt rows and new Argon2id rows coexist in the same
  column during migration without branching at every call site.
- **`#[derive(HashOnNew)]` + `#[hash]` field attribute** — new
  workspace member `nestrs-oauth2-macros` (proc-macro sub-crate
  mirroring `nestrs-macros`). Derive emits an inherent
  `new_with_hashed(...)` constructor that takes every named field
  in declaration order and hashes each `#[hash]`-marked field
  via the absolute path `::nestrs_oauth2::password::hash(...)`
  before storage. The attribute is **inert without the derive**
  (a bare `#[hash]` outside `#[derive(HashOnNew)]` is a compile
  error, not a silent no-op). Multiple marked fields are
  supported; generics are preserved; `#[hash(args)]` is rejected
  with an actionable error — backend selection happens at hash
  time, not derive time. Field ordering is preserved verbatim
  (pinned by test).
- **Disabled-backend error semantics** — `hash_with(plain, Argon2)`
  on a build with only `password-bcrypt` returns
  `Err(HashError::Argon2Disabled)` (not `Err`, not silent `false`).
  This surfaces as a 500 to operations so missing-feature
  configuration can't be mistaken for "wrong password".
- **Re-exports** — `hash` / `hash_with` / `verify` / `verify_any` /
  `verify_with` / `Backend` / `HashError` / `PasswordHasher` /
  `BcryptHasher` / `Argon2Hasher` from `nestrs_oauth2::*`; behind
  `password-macros`, also `HashOnNew` (the derive) and
  `hash_attr` (the inert attribute, renamed to avoid colliding
  with the free `hash` function).
- **Tests** — 8 integration tests in
  `nestrs-oauth2/tests/password.rs` (gated per backend feature):
  per-backend round-trip via the public API, default hash uses
  argon2, `verify_any` prefix dispatch, `UnknownPrefix` rejection,
  disabled-backend error paths (only run in single-backend
  builds), `PasswordHasher` trait object round-trip, plus three
  `HashOnNew` derive tests — single marked field, multiple
  marked fields, field-order preservation. 7 inline unit tests
  inside `src/password.rs` for the bcrypt / argon2 impls.
- **Docs** — `mintlify-docs/oauth2/hashing.mdx` (full reference:
  feature table, hash/verify, backend selection, `PasswordHasher`
  trait, `#[derive(HashOnNew)]` examples, migration story with
  the bcrypt → argon2id rehash pattern, "why these choices",
  see-also) + `docs.json` entry under `Guides`.

### Added — nestrs as an OAuth2 **authorization server** (`nestrs-oauth2`, feature `authorization-server`)

The third OAuth2 role alongside the existing client and resource
server: the IdP itself (RFC 6749), for apps that want nestrs to be
their identity provider — first-party auth without an external
Auth0/Keycloak. Issues **Ed25519-signed (`EdDSA`) access JWTs** with
the standard claims plus `scope`/`client_id`, verifiable by any
compliant resource server including this crate's own
`JwtVerifier`/`JwksCache`.

- **Endpoints** — `GET /oauth/authorize` (§4.1.1 + PKCE RFC 7636,
  S256 only), `POST /oauth/token` (authorization_code, rotating
  refresh_token, client_credentials), `POST /oauth/introspect`
  (RFC 7662), `POST /oauth/revoke` (RFC 7009), RFC 8414 discovery at
  `/.well-known/oauth-authorization-server`, and `/.well-known/jwks.json`
  (Ed25519 `OKP` JWK). Mount via `OAuth2AuthorizationServerModule`
  (nestrs DI) or `routes::router` into any axum app; every path and TTL
  is configurable (`AuthorizationServerConfig` builders).
- **Security model** — PKCE S256 required for public clients and (by
  default, OAuth 2.1 posture) confidential ones too; authorization
  codes are single-use, 32 random bytes, stored SHA-256-hashed, and
  **replaying one revokes the refresh family it seeded**; refresh
  tokens rotate on every use with **reuse detection** — a reused token
  is theft with high probability, so the entire family dies, including
  outstanding access JWTs (via the `jti` revocation list). Client
  secrets and PKCE verifiers are stored hashed and compared
  constant-time.
- **Open-redirect safe** — `/authorize` never redirects until client +
  `redirect_uri` are validated, and then only to a byte-for-byte
  registered URI; client-auth mistakes (unknown client, wrong secret,
  Basic+form at once, mismatched ids) are answered in place, never via
  redirect.
- **Pluggable state** — clients, codes, refresh tokens, and the
  access-token revocation list live behind four small async traits
  (`stores`); in-memory implementations ship for dev/tests/single
  instance, Postgres/Redis-backed ones compose for production scale.
  Codes and refresh tokens are only ever handled hashed — a store
  compromise yields unredeemable digests.

### Fixed — introspection and revocation never validated access JWTs (`authorization-server`)

jsonwebtoken 10's `Validation::new(EdDSA)` defaults `validate_aud: true`
with no expected audience, and per RFC 7519 rejects any token carrying
an `aud` claim against an empty expected set — so every
`/oauth/introspect` call returned `active: false` for valid access
JWTs, and `/oauth/revoke` silently no-oped for them. Both endpoints now
disable audience validation: the signature check against the server's
own key is the authenticity proof, and `aud` is client-specific data
echoed back (RFC 7662 §2.2), not something these server-side endpoints
validate against a configured value. Caught by the new integration
suite's introspection round-trips.

### Tests — 22 integration + 4 model tests (feature `authorization-server`)

- Every security-critical transition: PKCE enforcement (missing/plain
  verifier/wrong verifier burns the code), single-use codes with
  replay → family revocation (while a *different* family survives),
  refresh rotation + reuse → family + access-JWT death, scope narrowing
  allowed / widening rejected, open-redirect matrix (unregistered URI,
  unknown client, query-string mismatches, cross-client redirect),
  client-auth failures (401 + `WWW-Authenticate`, Basic+form rejection,
  public-client-with-secret), revocation semantics (idempotent, kills
  refresh families), refresh-token client binding, RFC 8414 discovery,
  the Ed25519 `OKP` JWKS document, and a full round-trip: tokens minted
  by the authorization server verified over real HTTP by this crate's
  own `JwtVerifier::from_url` (tampered signature rejected).

### Added — full-stack OTLP observability (traces + metrics + logs, feature `otel`)

- **OTLP metrics export** — `OpenTelemetryConfig::metrics()` pushes every
  `metrics`-facade instrument (the framework's RED metrics *and* your own)
  to the OTLP collector via a `SdkMeterProvider` + `PeriodicReader`
  pipeline (`otel::install_otlp_meter` / `shutdown_meter_provider` for
  direct lifecycle control).
- **OTLP logs export** — `OpenTelemetryConfig::logs()` bridges the
  `tracing` facade into the OTLP log pipeline
  (`opentelemetry-appender-tracing`), so every `tracing::*!` event becomes
  an OpenTelemetry log record correlated with the active trace/span
  (`otel::install_otlp_logger` / `shutdown_logger_provider`).
- **Composite metrics fan-out** — nestrs now owns the process-global
  `metrics` recorder slot with a fan-out recorder. `enable_metrics` and the
  OTLP metrics bridge register as members in any order; an app with both
  gets Prometheus pull **and** OTLP push from one recording surface
  (labels → OTel attributes, `Unit::Seconds`→`s` / `Unit::Count`→`1`,
  facade gauge deltas accumulate to absolute OTel gauges, counter
  `absolute` maps to monotonic `add(v - last)`).
- **`nestrs::metrics`** — the `metrics` facade re-exported so apps can
  instrument with `nestrs::metrics::counter!(...)` without adding the
  dependency; with `otel`, `nestrs::opentelemetry` re-exports the OTel API
  for custom instruments on the same pipeline.
- Framework RED metrics are now declared (unit + description) via
  `metrics::describe_*` so both backends render them with metadata from the
  first scrape/export.
- Graceful shutdown (`listen*` paths) flushes and stops the meter and
  logger pipelines alongside the existing tracer shutdown.

### Tests — 3 new (unit fan-out/bridge + integration dual-export)

- `metrics_export` unit tests: fan-out forwards registrations and
  recordings to every member (counters, gauges, histograms); the OTel
  bridge exports counters (u64 sums with attributes), gauge
  delta→absolute conversion, and histogram records through the SDK's
  `InMemoryMetricExporter`.
- `tests/otel_metrics.rs`: offline install of the meter/logger pipelines
  from async context, and a dual-export integration test — OTLP opt-ins +
  `enable_metrics` + real middleware traffic renders the framework RED
  metrics at `/metrics` with both backends attached.

### Added — Mongoose-style MongoDB adapter (`nestrs-mongodb`, feature `mongo`)

Wave 7.6. The third ORM in the persistence layer alongside
`nestrs-prisma` and `nestrs-storage`. Mirrors NestJS's
`@nestjs/mongoose` in standalone-crate form.

- **New workspace member `nestrs-mongodb`** — `MongoModule::for_root(uri)`
  / `for_root_with_options(opts)` / `for_feature(db_name)`,
  `MongoService` injectable, typed `MongoRepository<T>` CRUD wrapper
  (`find_one` / `find_by_id` / `find` / `find_with_options` /
  `count_documents` / `estimated_document_count` / `insert_one` /
  `insert_many` / `update_one` / `update_many` / `find_one_and_update`
  / `replace_one` / `delete_one` / `delete_many` / `delete_by_id`,
  plus `from_service` and `for_feature` helpers), `MongoOptions` builder
  (URI / app_name / timeouts / direct_connection / default_database),
  `MongoError` enum wrapping `mongodb::error::Error` + `bson::ser::Error`
  + `bson::de::Error` + `NotConfigured` / `Timeout` / `InvalidArgument`,
  `Document` trait + `#[derive(Document)]` derive macro + `#[schema]`
  / `#[prop]` attributes. The `bson` and `mongodb` crates are
  re-exported from the crate root so callers don't need direct deps for
  BSON documents or driver types.
- **Umbrella `nestrs` (`feature = "mongo"`)** — `MongoModule` /
  `MongoService` / `MongoRepository` / `MongoOptions` / `MongoError`
  / `Document` / `Schema` / `Filter` / `Update` re-exported from
  `nestrs::mongo::*`. The `mongo` feature now pulls
  `dep:nestrs-mongodb` instead of the driver directly; `mongo-dns`
  flips to `nestrs-mongodb/dns-resolver`. The legacy `mongodb` direct
  dep is removed from the umbrella (Phase B's 1-line shim replaces
  the inline `nestrs/src/mongo.rs`).
- **Tests** — `nestrs-mongodb/tests/document_derive.rs` covers the
  `#[derive(Document)]` emit, the `#[schema(collection = …)]`
  override, the default snake_case-plural fallback
  (`User` → `"users"`, `BlogPost` → `"blog_posts"`), `#[prop(...)]`
  attribute parsing for `rename` / `unique` / `default` / `skip` /
  `sparse` / `index` / `ref` keys, and the `to_bson` round-trip.
- **Docs** — `mintlify-docs/recipes/mongodb.mdx` (full reference with
  install, boot, schema, repository usage, `MongoOptions` builder,
  feature flags, full CRUD surface table) + `docs.json` entry under
  `recipes` "Backend stacks".

### Added — Drizzle ORM adapter (`nestrs-drizzle`)

Wave 7.7. The fourth persistence option alongside `nestrs-prisma`,
`nestrs-storage`, and `nestrs-mongodb` — typed SQL query builder
for apps that prefer Drizzle's query-first DSL.

- **New workspace member `nestrs-drizzle`** — `DrizzleModule::for_root(url)`
  / `for_root_with_options(opts)` static setters, `DrizzleService`
  injectable handle with `url()`, `is_postgres()`, `is_mysql()`,
  `is_sqlite()`, `parsed_url()`. `DrizzleOptions` builder wraps the URL
  with auto-detected driver (`postgres` / `postgresql` / `mysql` /
  `mariadb` / `sqlite` / `sqlite:`) plus `max_pool_size` and
  `connect_timeout`. `DrizzleError` enum (`NotConfigured` /
  `InvalidUrl` / `Driver` / `Timeout`). `drizzle_orm` re-exported at
  the crate root so callers don't need a direct dep. `schema::table!`
  re-exports `drizzle_orm::table!` plus common column types
  (`Int4`, `Int8`, `Int2`, `Text`, `Bool`, `Timestamp`, `Varchar`).
- **Feature flags** — `postgres` / `mysql` / `sqlite` / `all` map
  directly to `drizzle-orm/*` features. Default is empty (base
  crate, no SQL backend enabled — pick at least one).
- **Tests** — `nestrs-drizzle/tests/drizzle_module.rs` (6 tests):
  `for_root` sets options, URL-scheme auto-detection for all three
  backends, builder overrides for `max_pool_size` / `connect_timeout`,
  `parsed_url` resolves scheme / host / port / path.
- **Docs** — `mintlify-docs/recipes/drizzle.mdx` (full reference
  with install, boot, schema definition, query-builder usage,
  configuration, API surface table) + `docs.json` entry under
  `recipes` "Backend stacks".

### Added — Outbound HTTP client (`nestrs-http`, feature `http-client`)

Wave 7.8. Extracted the existing `nestrs::HttpService` /
`HttpModule` into a focused workspace member. The umbrella keeps
the same paths via a 1-line re-export shim.

- **New workspace member `nestrs-http`** — `HttpService` wraps a
  shared `reqwest::Client` with bounded default timeouts
  (`DEFAULT_REQUEST_TIMEOUT = 30s`, `DEFAULT_CONNECT_TIMEOUT = 10s`).
  `.get(url)` / `.post(url)` / `.put(url)` / `.patch(url)` /
  `.delete(url)` return `reqwest::RequestBuilder`s;
  `.client()` exposes the raw client. `HttpServiceOptions` builder
  tunes `request_timeout` / `connect_timeout`. `HttpModule::register()`
  installs the singleton provider. `reqwest` re-exported at the
  crate root under feature flag `reqwest`.
- **Umbrella `nestrs` (`feature = "http-client"`)** — `HttpService` /
  `HttpModule` / `HttpServiceOptions` / `DEFAULT_REQUEST_TIMEOUT` /
  `DEFAULT_CONNECT_TIMEOUT` re-exported from `nestrs::http_client::*`.
  The `http-client` feature now pulls `dep:nestrs-http` instead of
  `dep:reqwest` directly; `nestrs-health/http` stays as-is. The
  legacy `reqwest` direct dep is removed from the umbrella entirely.
- **Tests** — `nestrs-http/tests/timeouts.rs` (3 tests): default
  options carry sane timeouts (30s / 10s), `from_options` produces a
  working client, the five request-builder helpers (`get` / `post` /
  `put` / `patch` / `delete`) compile and return typed builders.
- **Docs** — `mintlify-docs/recipes/http-client.mdx` (full reference
  with install, "why default timeouts matter", boot, configuration,
  API surface table) + `docs.json` entry under `recipes` "Backend
  stacks".

### Note — pre-existing umbrella tests

`nestrs/tests/http_client_timeouts.rs` (the regression test that
moved over) keeps compiling unchanged via the 1-line shim — the
file's content already lives in `nestrs-http/tests/timeouts.rs`.

### Added — `nestrs-cli new app|lib|resource` scaffolder (Tier 3.1)

Wave 7.9. The pre-1.1 `nestrs-cli new <name>` signature kept
working as an alias; this wave adds explicit subcommands and a
third scaffolding shape (resource modules).

- **`nestrs-cli new app <name>`** — generates a binary crate at
  `./<name>/`: `Cargo.toml` (with release profile flags), `src/main.rs`
  with `AppController` / `AppService` / `PingDto`, the standard
  middleware stack (`set_global_prefix` / `use_request_id` /
  `use_request_tracing` / `enable_metrics` / `enable_health_check` /
  `enable_production_errors_from_env`), `README.md`,
  `.env.example`, `.gitignore`, `Dockerfile` (multi-stage
  `rust:1.75` → `debian:bookworm-slim`).
- **`nestrs-cli new lib <name>`** — generates a library crate at
  `./<name>/`: `Cargo.toml`, `src/lib.rs` declaring `pub mod
  controllers; pub mod services; pub mod dto;`, `README.md`,
  `.gitignore`.
- **`nestrs-cli new resource <name>`** — generates a full resource
  module under `./<name>/src/<name>/`: `dto.rs` (entity,
  `Create<Name>Dto` with `#[dto]`, `Update<Name>Dto` with Wave
  7.5's `#[nestrs::partial_type]`), `service.rs` (with
  `#[injectable]`), `controller.rs` (with `#[controller(prefix =
  "/<name>")]`, `list` / `create` / `update` handlers using
  `ValidatedBody<>`), `module.rs` (with `#[module(...)]` listing
  controllers / providers / exports), `mod.rs`. Drop the directory
  into an existing app's `src/` and import `<Name>Module` from
  `AppModule`.
- **Back-compat** — `nestrs-cli new <name>` (no `app` / `lib` /
  `resource` keyword) still dispatches to `new app`. No existing
  scripts break.
- **Common flags** — `--no-git` (skip `git init`), `--strict`
  (generated crate starts with `#![deny(unsafe_code)]`).
- **Tests** — `nestrs-cli/tests/scaffolder_cli.rs` (4 tests):
  `new_app_creates_binary_crate` (asserts main.rs boots
  `AppModule` + has `#[controller]` + `#[dto]`),
  `new_lib_creates_library_crate` (asserts lib.rs declares
  controllers / services / dto modules),
  `new_resource_emits_controller_service_dto_module` (asserts
  dto.rs uses `#[dto]` and `#[nestrs::partial_type]`, controller.rs
  uses `#[controller]` and `ValidatedBody`, module.rs lists
  controllers / providers / exports, service.rs uses
  `#[injectable]`), `new_without_kind_keyword_still_creates_app`
  (back-compat).
- **Docs** — `mintlify-docs/cli/scaffolder.mdx` (full reference for
  all three subcommands with examples, common flags, back-compat
  section, see-also) + `docs.json` entry under `CLI`.

### Added — `nestrs-cli repl graph|routes|providers|dtos` (Tier 3.2)

Wave 7.10. A static source-level DI graph explorer. Mirrors the
NestJS "print application graph" debugging tool but works without
launching the app — `nestrs-cli repl` reads the user's `.rs`
files and extracts the module / controller / provider / DTO tree
via regex (no `syn` dependency).

- **Subcommands**
  - `nestrs-cli repl graph [--path <dir>] [--format text|json]` —
    tree view of modules with imports / controllers / providers
    and each controller's routes. JSON output is serde-round-trip.
  - `nestrs-cli repl routes` — flattened, sorted HTTP route table
    (METHOD / PATH / HANDLER columns). Ideal for piping into a
    Markdown doc or an OpenAPI generation script.
  - `nestrs-cli repl providers` — every `#[injectable]` type,
    grouped by the module that declares it (orphans fall under
    `unassigned:`).
  - `nestrs-cli repl dtos` — every `#[dto]`-decorated struct
    (heuristic: name ends with `Dto` / `Entity` / `Model`).
- **Static analysis** — the parser recognises `#[module(...)]`,
  `#[controller(prefix = "/...")]`, `#[injectable]`, `#[dto]`,
  and HTTP-method macros (`#[get]` / `#[post]` / `#[put]` /
  `#[patch]` / `#[delete]`). Struct names are captured
  positionally — only types immediately following the attribute
  are added to the graph (no false positives from same-file
  bystander structs). Multi-line `#[module(...)]` lists parse
  correctly.
- **Flags** — `--path <dir>` (defaults to `./src`),
  `--format text|json`.
- **Tests** — `nestrs-cli/tests/repl_cli.rs` (8 tests): the
  `graph_finds_modules_with_imports_controllers_providers` test
  verifies a two-module fixture with imports is parsed; the
  `graph_finds_controllers_with_routes` test asserts GET / POST /
  PATCH routes are extracted with handlers; the
  `graph_finds_injectable_providers` test asserts both providers
  are detected and bystander `AppController` is NOT marked as a
  provider; the `graph_finds_dtos` test asserts CreateUserDto and
  UpdateUserDto are captured. Dispatch tests cover unknown
  subcommand errors, `graph` / `routes` path flag handling, and
  the missing-path error.
- **Deps** — added `regex = "1"` and `serde = { version = "1",
  features = ["derive"] }` to `nestrs-cli/Cargo.toml`. Existing
  `serde_json` dep powers JSON output.
- **Docs** — `mintlify-docs/cli/repl.mdx` (full reference with
  examples, flag table, limitations) + `docs.json` entry under
  `CLI`.

### Added — GraphQL SDL export (`nestrs-graphql` + `nestrs-cli graphql sdl`, Tier 3.3)

Wave 7.11. Two complementary SDL-export paths:

1. **Build-time** (`nestrs_graphql::export_sdl_to_file`,
   `export_sdl_with_options_to_file`) — wraps
   `async_graphql::Schema::sdl()` /
   `schema.sdl_with_options(SDLExportOptions)` and writes the SDL
   to disk. Federation v2 subgraphs use
   `SDLExportOptions::default().federation().compose_directive()`
   to emit the `@link` / `@key` directives and `_Entity` /
   `_service` plumbing an Apollo Router expects. Re-exported at
   the umbrella `nestrs::graphql::*` path via the existing
   `pub use nestrs_graphql as graphql;`.
2. **Runtime** (`nestrs-cli graphql sdl --url <http> --out
   <path>`) — POSTs the standard federation introspection query
   (`{_service{sdl}}`) against a running subgraph and writes the
   response SDL to disk. Shells out to `curl` (no Rust HTTP
   client dep) with `--max-time 15`. `--bearer-token` adds an
   `Authorization` header for protected endpoints.

- **Files** —
  - `nestrs-graphql/src/sdl.rs` — added
    `export_sdl_to_file<Q,M,S>(schema, path) -> Result<usize, String>`
    and `export_sdl_with_options_to_file<Q,M,S>(schema, options,
    path)`. Internal helper `write_sdl_to_file` creates parent
    dirs as needed.
  - `nestrs-graphql/src/lib.rs` — re-exports the two new helpers
    alongside the existing `export_schema_sdl` /
    `export_schema_sdl_with_options`.
  - `nestrs-cli/src/graphql_sdl.rs` — new module. `run` dispatches
    `--url` / `--out` / `--bearer-token` / `--federation` /
    `--no-federation`. `fetch_sdl` shells to `curl`. `write_sdl`
    creates parent dirs and writes bytes. `parse_sdl_body` is a
    pure helper for embedding in custom tooling. Module exposes
    `pub` so integration tests can import it.
  - `nestrs-cli/src/main.rs` — added `mod graphql_sdl;`, dispatcher
    arm `"graphql" => graphql_sdl::run(&args[1..])`, and a help
    line describing the subcommand.

- **Tests** —
  - Unit tests in `nestrs_cli::graphql_sdl` (3 tests):
    `parse_sdl_body_extracts_federation_sdl_field`,
    `parse_sdl_body_rejects_missing_service_field`,
    `parse_sdl_body_rejects_malformed_json`.
  - `nestrs-cli/tests/graphql_sdl_cli.rs` (8 tests):
    `parse_sdl_body_extracts_federation_sdl_field`,
    `parse_sdl_body_rejects_missing_service_field`,
    `write_sdl_creates_parent_dirs_and_writes_bytes`,
    `write_sdl_overwrites_existing_file`,
    `run_rejects_missing_url`,
    `run_rejects_missing_out`,
    `run_rejects_unknown_option`,
    `fetch_sdl_against_mock_server` (smoke test against a local
    Python HTTP server that responds to the federation SDL query),
    `run_full_subcommand_writes_file` (end-to-end through the
    `run` entry point), `run_full_subcommand_with_bearer_token`
    (verifies the bearer-token flag doesn't break the path). The
    Python mock server checks for `python3 --version` and skips
    cleanly when Python is unavailable.

- **Deps** — no new external deps. The CLI reuses its existing
  `serde_json` for response parsing; the SDL writer is hand-rolled
  to avoid pulling a heavy HTTP-client crate into the CLI.

- **Docs** — `mintlify-docs/graphql/sdl-export.mdx` (full
  reference with code examples for both paths, flag table,
  when-to-use-which, programmatic-access example, see-also) +
  `docs.json` entry under `Guides`.

### Added — GraphQL Federation v2 subgraph SDL helpers (`nestrs-graphql` + `nestrs-cli graphql federation export`, Tier 3.4)

Wave 7.12. The Apollo Federation v2 shape (`@link` / `@key` /
`_Entity` / `_service`) — the form Apollo Router and GraphOS
Studio expect — gets first-class helpers on both the build-time
and CLI side, plus a strict-by-default validation gate so CI
catches the common mistake of exporting from a federation v1 or
non-federation endpoint.

- **Build-time** — `nestrs_graphql::export_subgraph_v2_sdl<Q,M,S>(schema) -> String`
  emits a federation v2 SDL with a Wave 7.12 header comment. The
  output contains the `@link(url: "https://specs.apollo.dev/link/v1.0")`
  directive, `@key(fields: "...")` on every entity, the `_Entity`
  union type and `_service { sdl }` query field, and
  `@composeDirective` plumbing for custom directives you want to
  expose to the router. `export_subgraph_v2_sdl_to_file(schema, path)`
  is the disk variant — creates parent dirs and returns the byte
  count. `is_federation_v2_sdl(sdl)` is the substring check helper
  for embedding in custom tooling. All three are re-exported at the
  umbrella `nestrs::graphql::*` path behind the `federation-gateway`
  feature.
- **Runtime** — `nestrs-cli graphql federation export --url <http>
  --out <path> [--bearer-token <token>] [--lenient]` fetches the
  federation v2 subgraph SDL via `{_service{sdl}}` (Apollo
  Federation v2 introspection) and **validates the response
  contains `@link`** before writing to disk. Strict-by-default:
  SDLs without `@link` are rejected with an actionable error
  pointing at `--lenient`. The CLI reuses `graphql_sdl::fetch_sdl`
  for the HTTP fetch (still `curl`, no Rust HTTP client dep) and
  duplicates the trivial `is_federation_v2_sdl` substring check
  inline (no need to pull `nestrs-graphql` into the CLI).
- **Files** —
  - `nestrs-graphql/src/federation.rs` — added
    `export_subgraph_v2_sdl<Q,M,S>(schema) -> String`,
    `export_subgraph_v2_sdl_to_file<Q,M,S>(schema, path) -> Result<usize, String>`,
    and `is_federation_v2_sdl(sdl) -> bool`.
  - `nestrs-graphql/src/sdl.rs` — `write_sdl_to_file` is now
    `pub(crate)` so `federation.rs` can reuse the same
    `create_dir_all` + `File::create` + `write_all` sequence.
  - `nestrs-graphql/src/lib.rs` — re-exports the three new helpers
    behind `#[cfg(feature = "federation-gateway")]`.
  - `nestrs-cli/src/graphql_federation.rs` — new module. `run`
    dispatches `--url` / `--out` / `--bearer-token` / `--lenient`,
    reuses `graphql_sdl::fetch_sdl` + `graphql_sdl::write_sdl`,
    and duplicates the trivial `is_federation_v2_sdl` substring
    check. `dispatch` is the sub-dispatch entry point wired into
    `main.rs`. Module exposes `pub` so integration tests can
    import it. 3 unit tests for the substring check.
  - `nestrs-cli/src/main.rs` — added `pub mod graphql_federation;`,
    reworked `"graphql"` arm to sub-dispatch on
    `sdl` / `federation` (bare `graphql` still routes to
    `graphql_sdl` for back-compat — SDL exporter errors with
    missing-flags message). New help line for the
    `nestrs-cli graphql federation export` subcommand.
- **Tests** —
  - Unit tests in `nestrs_graphql::federation` (5 tests):
    `is_federation_v2_sdl_detects_at_link_directive` (v2 fixture),
    `is_federation_v2_sdl_rejects_v1_or_non_federation_sdl`
    (v1 + plain fixtures),
    `is_federation_v2_sdl_handles_empty_input`,
    `export_subgraph_v2_sdl_emits_at_link_header_and_body` (real
    `Schema` round-trip via `Schema::build(PingQuery, EmptyMutation,
    EmptySubscription)`),
    `export_subgraph_v2_sdl_to_file_writes_and_returns_byte_count`
    (process-id-scoped tmp file write).
  - Unit tests in `nestrs_cli::graphql_federation` (3 tests):
    substring check fixtures for `@link` / v1 / plain / empty.
  - `nestrs-cli/tests/graphql_federation_cli.rs` (10 tests):
    dispatch tests for unknown subcommand / missing subcommand /
    `export` routing; flag-validation tests for missing `--url`,
    missing `--out`, unknown option; the
    `run_rejects_non_federation_v2_sdl_by_default` test calls the
    helper directly to confirm rejection; end-to-end tests with a
    Python mock server that returns the SDL payload from a tmp
    file (avoiding shell-quoting issues with the multiline
    fixture): `run_writes_federation_v2_sdl_when_at_link_present`
    (strict accept), `run_rejects_v1_sdl_in_strict_mode` (asserts
    error mentions `--lenient` and no file is written),
    `run_accepts_v1_sdl_in_lenient_mode` (v1 written when
    `--lenient`), and `run_with_bearer_token_succeeds` (bearer
    plumbing). Mock server checks for `python3 --version` and
    skips cleanly when Python is unavailable.
- **Deps** — no new external deps. `write_sdl_to_file` was
  already private inside `sdl.rs`; promoting it to `pub(crate)` is
  a visibility-only change with no compile-time impact on
  downstream crates.
- **Docs** — `mintlify-docs/graphql/federation-v2.mdx` (full
  reference with code examples for both build-time and CLI paths,
  flag table, when-to-use-which, the federation-gateway vs
  subgraph distinction, see-also linking the existing
  `sdl-export` doc) + `docs.json` entry under `Guides`.

## [1.0.0] - 2026-09-12

First stable release. Everything since 0.5.2 is in this version: the
NestJS-parity waves (federation gateway, DB migrations + seeding CLI,
`#[crud]`, public-API snapshot gate, MCP protocol surface, OAuth2,
row-level authorization, `#[use_pipes]` extraction-time chains) and a
full production/security audit (2026-09-10, fix-as-we-go) whose 25
findings — plus 2 discovered during fixes — are all landed below.

**Stability:** from 1.0.0 onward the public API is covered by full
[semver](https://semver.org) (see `STABILITY.md`) — breaking changes
only in a new major. The 0.x-era breaking changes that had been
flagged "pre-1.0" in earlier entries (`CacheOptions::InMemory` gaining
`max_entries`, `Subject::Type` moving from `&'static str` to `String`)
are folded into this major bump; migration notes are in their entries.

### Docs — every surface synced with the landed behavior; version strings to 1.0.0

- Mintlify + mdBook now document the audit-landed semantics: request-scope
  resolution off-scope (`try_get` → `None`, `get` → panic naming the fixes)
  and `spawn_with_request_scope` snapshot semantics; the bounded in-memory
  cache (`in_memory()`, `in_memory_with_max_entries(n)`, FIFO eviction);
  `register_use_value_with_lifecycle` / `register_use_factory_with_lifecycle`
  and the `ProviderLifecycle` hooks; dependency-first hook ordering (reversed
  on shutdown); and the `Piped*` extractor pipes (`TrimPipe`, explicit chains,
  first error short-circuits).
- **New pages** — `#[crud]` generation (guides/crud + docs/src/crud.md),
  OAuth2 client/resource-server/social flows (guides/oauth2 + docs/src/oauth2.md),
  authorization abilities/guards/row-level (guides/authorization +
  docs/src/authorization.md), and the `nestrs-cli db` migrations-and-seeding
  reference (cli/overview + docs/src/cli.md, ecosystem/database). Nav
  registered in both docs.json and SUMMARY.md.
- **Ops hardening documented** — route-level throttling
  (`use_throttler`, `#[throttle]` / `#[skip_throttle]`, 429 headers),
  trusted-proxy client identity (`use_trusted_proxy_headers`, right-most-first
  XFF, rate-limit/throttler inheritance), production error sanitization
  (default-on in production, force/opt-out builders), probe decorators
  (`#[liveness]` / `#[readiness]` / `#[startup]` mirrored under
  `/__nestrs/health/*`, 5 s cache), request-id charset gate, TCP microservice
  bounds (1 MiB frames, 30 s idle/client timeouts, 1024 connection cap),
  RabbitMQ nack-drop/poison-message semantics, and the
  `#[config(namespace)]` namespaced env system — across security,
  observability, microservices, and fundamentals pages on both surfaces.
- MCP crate docs (guide + api/crates page + mdBook) expanded to the full
  protocol surface: server, tools/surfaces, elicitation, tasks, source
  introspection.
- All version strings across mintlify, mdBook, README, and the website
  updated from 0.3.8/0.5.2-era references to 1.0.0.
- Removed dead `documentation`/`readme`/`description` fields from
  `[workspace.package]` (nightly cargo warned on every build; every member
  crate sets its own crate-specific values), the redundant workspace/member
  `homepage` (identical to `repository`), and the 12 explicit
  `readme = "README.md"` lines (auto-inferred when README.md sits at the
  crate root) — silencing the nightly `redundant_homepage` / `manual_readme`
  manifest lints across the workspace.
- Rustdoc: new module-level docs for `#[nestrs::crud]` runtime support
  (adapter rationale, serde_qs contract) and a full `#[crud]` macro
  reference in nestrs-macros; every intra-doc link in the workspace passes
  `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features`.

### Fixed — production/security audit: masking interceptor leaked one `Box` per masked object per response

- **`mask_value` leaked the response's `"type"` field.** For every JSON
  object carrying a `"type"` key that went through the
  `PolicyMaskingInterceptor` (HTTP, and the WS / GraphQL / MCP walkers that
  re-use `mask_value`), the type name was `Box::leak`ed into a
  `Subject::Type(&'static str)` — a slow, unbounded memory leak proportional
  to response traffic, with attacker-influenceable content (any handler that
  echoes request data into a `"type"` field).
- **Root cause fixed: `Subject::Type` owns its name.** The variant now holds
  a `String` instead of `&'static str`, so runtime-built subject names
  resolve without leaking. This also removed the guard-side leak machinery:
  `PoliciesGuard` previously pushed every route-metadata subject name through
  a dedup set behind a **global mutex on the authz hot path**; it now clones
  the per-request name, and that whole `leak_static` block is gone.
  - **Migration (pre-1.0):** `Subject::Type("Post")` becomes
    `Subject::Type("Post".into())` (or any runtime `String`). The
    `AbilityBuilder::can*` methods still take `&'static str` literals for
    rule types — only the *query-side* `Subject` enum changed.
- **Tests:** unit test pinning that runtime-built (non-`'static`) names work
  through `can()`/`type_name()` (does not compile against the old enum);
  masking integration test masking a runtime-built `"type"` value. Public-API
  snapshot unchanged (path-level entries only).

### Fixed — production/security audit: request-scope panic across `tokio::spawn`

- **`Request`-scoped providers panicked when resolved off the request task.**
  A handler that spawned background work with bare `tokio::spawn` ran the
  child with no request scope (task-locals do not cross `spawn`), and
  resolving a `Request`-scoped provider there panicked — through
  `registry.try_get` too, which violated its documented "returns `None`
  instead of panicking" contract. Resolution off-scope is now graceful:
  - `registry.try_get::<T>()` returns `None` (absence, same as a missing
    provider — the doc now spells out both cases).
  - `registry.get::<T>()` still panics (`get` is the panic-on-unresolvable
    API, same as "not registered"), but with a message that names the fixes
    instead of a dead-end.
  - **New: `spawn_with_request_scope(future)`** — the supported way to run
    background work that resolves `Request`-scoped providers. Spawned inside
    a request, the child receives a **snapshot** of the request scope: it
    resolves the same request-scoped instances the request had at spawn
    time (including any in-flight `TransactionSlot`), while values
    constructed or inserted after the spawn stay private to whichever side
    created them. Spawned outside any scope, the child gets a fresh empty
    scope — request-scoped providers construct per spawned task and are
    isolated from every other task. The ability / principal slots are
    deliberately **not** carried: row-level authz stays deny-closed in the
    spawned task unless the caller explicitly wraps the future with the
    ability helpers.
- **Tests:** five new `nestrs-core` unit tests — off-scope `try_get` returns
  `None` (not a panic); off-scope `get` panics with the fix in the message;
  the child resolves the parent's instance (no re-construction, `Arc::ptr_eq`);
  child-scope writes stay in the child; off-scope spawns get isolated fresh
  scopes (one construction per spawn). API snapshot: +1 path
  (`nestrs_core::spawn_with_request_scope`).

### Fixed — production/security audit: unbounded in-memory cache, silent "unknown" rate-limit bucket, and trusted client-supplied request ids

Three hardening fixes from one audit finding cluster:

- **In-memory `CacheService` had no entry cap.** The in-memory backend was a
  plain unbounded `HashMap` — any flood of distinct keys (e.g.
  attacker-influenced cache keys) grew the map without limit, and TTL'd
  entries were only removed when *their own key* was read again (one-shot
  entries leaked their memory forever). The store is now bounded:
  **FIFO eviction of the oldest-inserted entry when full**, with a default
  cap of **10 000 entries**. Updates to existing keys never evict (and
  refresh their FIFO position).
  - **Migration (pre-1.0):** `CacheOptions::InMemory` changed from a unit
    variant to `InMemory { max_entries: usize }`. Use the constructors —
    `CacheOptions::in_memory()` (10 000 cap, unchanged semantics for
    working sets under that size) or
    `CacheOptions::in_memory_with_max_entries(n)` for a larger working
    set (`0` disables caching — writes are dropped). No in-repo caller
    pattern-matched the variant.
- **Rate limiting silently bucketed unresolvable client IPs into one
  shared `"unknown"` key.** When no IP could be resolved (malformed
  `x-forwarded-for` under a trusted topology, or a transport without
  socket metadata), the rate limiter, throttler, and throttler guard all
  keyed the request under the same `unknown` bucket — coupling the limits
  of unrelated clients. The shared fallback is kept (fail-closed:
  unkeyable traffic is still limited; per-request unique keys would let a
  garbage-XFF attacker bypass limiting entirely), but it now **warns**
  (`target: nestrs::client_ip`) so operators can see when traffic is being
  keyed this way and fix the proxy topology. All three call sites share
  one helper (`rate_limit_key_ip`).
- **`use_request_id` trusted the client-supplied `x-request-id`.**
  tower-http's `SetRequestIdLayer` only fills in a *missing* header —
  whatever the client sent was honored verbatim and echoed into logs,
  tracing, `RequestContext`, and the response header (forged correlation
  ids, log-forging payloads, unbounded value length). A sanitize layer now
  runs before the tower-http layers: a client-supplied id is honored only
  when it is plain identifier-ish ASCII (`[A-Za-z0-9._-]`, non-empty,
  ≤ 128 bytes — UUID/ULID/hex style); anything else is stripped and a
  fresh UUID is assigned (with a `nestrs::request_id` warn). Legitimate
  upstream propagation still works.
  - **No migration:** valid ids (the only kind well-behaved clients send)
    pass through unchanged; invalid ones previously round-tripped
    attacker-chosen bytes.

### Fixed — production/security audit: admin sidecar accepted its bearer token via query string and compared it non-constant-time

The `nestrs::admin` sidecar (the `admin` feature) accepted the configured
bearer token via `?token=<token>` in the URL, and compared both the
header and query presentations with plain `==`.

- **Behavior:** the query-string path is REMOVED — credentials in URLs
  leak into access logs, proxy logs, browser history, and `Referer`
  headers. The token is now accepted only via
  `Authorization: Bearer <token>`, and the comparison is constant-time
  (`subtle::ConstantTimeEq`), so a `==` no longer short-circuits at the
  first differing byte (a byte-by-byte timing oracle). Token length is
  not treated as a secret (single early length check — standard for
  bearer credentials). The `admin` feature now pulls `subtle`.
- **Migration:** pass the token as a header (`Authorization: Bearer
  <token>`) — the in-repo `nestrs-mcp` runtime client already does;
  nothing in this workspace used the query param. Requests presenting
  the token in the query string now get 401.
- **Tests:** unit tests cover the constant-time helper and the
  header/scheme/wrong-token/missing-token matrix; the live smoke test
  pins that the correct token in the query string is rejected (401), as
  are missing tokens and tokens without the `Bearer` scheme.

### Fixed — production/security audit: override_provider silently coerced overridden providers to Singleton scope

`ProviderRegistry::override_provider` (and the
`DynamicModuleBuilder`/`TestingModule`/`ConfigurableModuleBuilder` paths that
delegate to it) rebuilt the provider entry with a hardcoded
`ProviderScope::Singleton` and an `unreachable!()` placeholder factory —
wholesale replacing the provider's declared scope. Overriding a
`#[injectable(scope = "request")]` or `scope = "transient"` provider made it
process-global: per-request state bled across requests, scope introspection
(`provider_summaries`, the admin snapshot) reported Singleton, and the
placeholder factory meant a scope-preserving fix could never have resolved
anything at all.

- **Behavior:** the override now preserves `T::scope()`. The placeholder
  factory is replaced by one handing out the given instance, so
  request-scoped overrides resolve per request (each request receives the
  SAME instance — an override explicitly targets one concrete object) and
  transient overrides resolve per injection site. Singleton behavior is
  unchanged (preset instance cell, lifecycle hooks still wired).
- **Migration:** none for singleton providers — every override in this
  workspace and in typical `TestingModule` usage targets a singleton. Code
  that overrode a request/transient provider and relied on the accidental
  Singleton coercion gets the provider's declared scope back.
- **Tests:** nestrs-core unit tests pin ptr-equality of the override
  instance plus the preserved scope for all three scopes — Singleton
  (instance served), Request (per-request resolution inside
  `with_request_scope`, instance shared across sequential requests), and
  Transient (per-resolution instance).

### Fixed — production/security audit: dependency-ordered lifecycle hooks ran DEPENDENTS before their dependencies

`ordered_singletons` — the topological sort behind all five lifecycle
hook runners — built its adjacency directly from the recorded
`constructor -> dependency` edges, treating the CONSTRUCTOR (the
dependent) as the node that initializes first. That inverted the
documented "dependencies initialize before dependents" contract: in
practice hook order collapsed to registration order for
dependency-connected providers, so a service's `on_module_init` could
run before the provider it depends on had initialized (e.g. a DB pool,
a cache), and the (already reversed) destroy hooks tore dependencies
down before the dependents that still held references to them.

- **Discovered while fixing the DI write-lock contention** (an empirical
  probe: a provider registered FIRST with a construct-time dependency
  still ran its init hook first, matching registration order instead of
  dependency order).
- **Behavior:** the sort now inverts the recorded edge direction when
  building its adjacency, so dependencies initialize first and the
  reversed destroy/shutdown hooks tear dependents down before their
  dependencies. Ties keep registration order; the hook-cycle fallback
  (append unvisited in registration order) is unchanged.
- **Migration:** code that relied on the old behavior was relying on a
  documented-contract violation (registration order even when a
  dependency edge existed). The doc comment and mdBook always stated
  dependencies first.
- **Tests:** a 3-level chain (C depends on B depends on A, registered
  top-first) pins init `[A, B, C]` and destroy `[C, B, A]` — both
  the direction and the registration-order override.

### Fixed — production/security audit: DI dependency recording took the global write lock on every resolution

Every `registry.get` executed inside a provider construction (inside
`construct` or a `useFactory` closure) recorded the
`constructor -> dependency` edge into the **process-global** dependency
graph under a `RwLock` **write** lock — even when the edge had long
since been recorded. Request-scoped and transient providers are
constructed per request, so every warm request re-took the same global
write lock for each DI dependency, serializing resolution across all
threads and tasks on one graph lock.

- **Behavior:** `record_provider_dependency` now double-checks under the
  read lock first — an already-recorded edge (the steady-state case on a
  warm process) returns without ever taking the write lock. The write
  lock is taken only the first time an edge is seen, and its existing
  re-check-before-push makes racing first recordings safe (no duplicate
  edges).
- **No migration.** Lock-contention fix only; the recorded graph contents
  are byte-identical and no public API changed (snapshot unchanged).
- **Tests:** nestrs-core unit tests pin edge idempotency across repeated
  recordings (the read-locked fast path) and dedup under 16 concurrent
  first-time recorders (the double-checked write arm).

### Fixed — production/security audit: useValue/useFactory providers could not run lifecycle hooks

`register_use_value` / `register_use_factory` accepted any
`T: Send + Sync + 'static`, so their five lifecycle hook slots were wired to
a no-op — `onModuleInit`, `onModuleDestroy`, `onApplicationBootstrap`,
`onBeforeApplicationShutdown`, and `onApplicationShutdown` never fired for
value/factory providers, silently. A pre-built connection pool, cache, or
background worker registered via `useValue`/`useFactory` had no way to warm
up or flush/close during boot or graceful shutdown (the workarounds —
eager construction inside the factory, or "fail on first use" — are exactly
the patterns this forces).

- **Behavior:** new opt-in trait `ProviderLifecycle` (five default-no-op
  async hooks, mirroring the NestJS `OnModuleInit` & friends on a
  `useValue`/`useFactory` provider) plus two registration variants,
  `ProviderRegistry::register_use_value_with_lifecycle` and
  `register_use_factory_with_lifecycle`. The framework then drives the
  hooks for singleton providers exactly like `Injectable` hooks:
  dependency/registration order for init/bootstrap, reverse for
  before-shutdown/shutdown/destroy. A factory singleton that nothing has
  resolved yet is constructed by its first hook (the same lazy contract as
  `Injectable` providers whose first resolution happens in a hook).
- **Why not fix the plain methods:** Rust has no specialization, so
  `register_use_value`/`register_use_factory` (which take any `T` so plain
  values like `i32`/`String`/config structs stay registerable) cannot
  detect a hook impl. Their documented hook-less behavior is unchanged.
- **Migration:** none — both plain methods keep byte-identical signatures.
  To get hooks on a value/factory provider, implement `ProviderLifecycle`
  for its type (re-exported in `nestrs::prelude`) and switch the
  registration call to the `_with_lifecycle` variant. Request/transient
  factory providers keep their existing behavior (hooks run only for
  singletons, matching `Injectable`).
- **Tests:** nestrs-core unit tests pin the hook sequence per provider
  (init → bootstrap → before-shutdown → shutdown → destroy), init-order /
  reverse-destroy ordering across providers, the factory-singleton lazy
  construction (hook is the first `get`, later `get`s reuse it), and the
  plain variants staying hook-less; a nestrs integration test drives the
  exact `listen()` boot + graceful-shutdown sequences over a value and a
  factory provider together.

### Fixed — production/security audit: nested scope installers REPLACED the request scope instead of layering into it

`with_request_scope` unconditionally opened a fresh request-scoped provider
cache, even when one was already active. Every installer that runs inside an
active request scope therefore forked it — most visibly the `#[transactional]`
middleware (`install_transactional_middleware` /
`TransactionalInterceptor`) under `use_request_scope()`: the nested
middleware replaced the request-scoped cache the outer middleware had
opened, instead of layering into it. The same shadowing applied to the
GraphQL, WebSocket, and MCP scope wrappers whenever they ran inside an
in-request transport.

- **Behavior:** `with_request_scope` now JOINS an active scope — values
  inserted by outer middleware stay visible inside nested installers, and
  anything a nested installer inserts (e.g. the `TransactionSlot`) lands in
  the same scope. Only when no scope is active does it open a fresh one
  (the per-request middleware case, and per-message/per-operation scopes
  on transports that drive handlers outside an HTTP request — those keep
  their isolation, since the upgraded-connection / tool-call futures run
  outside the original request's scope).
- **The user-visible defect:** a `ProviderScope::Request` provider resolved
  before a nested installer (e.g. by a guard) was invisible inside it —
  re-resolution constructed a SECOND instance of a provider the request
  had already built, breaking the one-instance-per-request DI contract,
  and values stashed by outer middleware disappeared for the rest of the
  request.
- **No migration.** No signature changes (`with_request_scope`,
  `request_scope_insert`, `request_scope_get` unchanged); public API
  snapshot verified unchanged. Code that (incorrectly) relied on a nested
  `with_request_scope` hiding outer values would be affected — there were
  no such call sites in the workspace.
- Tests: nestrs-core unit tests pin nesting (outer value visible inside,
  nested insert visible outside, one construction of a request-scoped
  provider across a nested boundary — `Arc::ptr_eq`); an end-to-end
  transactional test drives a route through an outer scope middleware +
  the transactional middleware, asserting the tx slot AND the outer
  middleware's stashed value are both visible in the handler and the
  transaction still commits; all multi-transport scope suites
  (request_scope, testing_module, ws/graphql multi-transport, MCP) pass
  unchanged.

### Fixed — production/security audit: `#[crud]` list endpoint materialized the entire table per request

`GET /` on a `#[crud]` controller fetched **every row** from the database
(`SELECT * FROM {table}`, no LIMIT), then sorted/filtered/searched and
paginated in memory. On a large table this is a memory-exhaustion and DoS
vector on every unauthenticated list request — one query bounds none of the
work it triggers.

- The list endpoint now runs in two tiers. Queries without
  `sort`/`filter`/`search` (the default `GET /` and plain `?page`/`?per_page`
  paging) push the window down to SQL — `LIMIT {per_page}
  OFFSET {(page-1)*per_page}` — so the database materializes one page, not
  the table. Results are byte-identical to the previous in-memory slice
  (same `ORDER BY id ASC` ordering, same 422 validation on `page`/`per_page`).
- Queries **with** `sort`/`filter`/`search` keep the full-fetch + in-memory
  evaluation: the `@nestjsx/crud`-compatible contract (case-insensitive
  substring filter, recursive search, `sort` over the full set before the
  window) requires evaluating the whole result set before slicing.
  Pushing those into SQL (dialect-aware `json_extract`) is future work;
  until then a `?sort=id:ASC` query on a huge table still fetches wide —
  documented, not silently truncated.
- New API: `Repository::find_all_paged(limit, offset)` (the bounded
  companion of `find_all`, negative arguments rejected with a `Protocol`
  error before any SQL runs) and `CrudService::list_page(limit, offset)`.
  Under `authz-row-level`, `find_all_authorized_paged` pushes LIMIT/OFFSET
  into the same WHERE-compiling fast path as `find_many_authorized` — but
  only when the matched read rule's predicate can compile to SQL. A
  predicate that can't (e.g. a plain closure) would make SQL pages come
  back short, so the method transparently falls back to the authz
  fetch-all path and slices the window in Rust: pagination is always
  exact, page windows never leak rows another principal should not see.
- **No migration needed.** `page`/`per_page` defaults and bounds (1..=1000)
  are unchanged; `list()` semantics are unchanged (it is now only the
  tier-2 wide-fetch primitive plus the direct "give me everything" API).
  Hand-written services calling `CrudService::list()` keep their current
  behavior and can opt into the bounded window with `list_page`.
- Tests: HTTP-level exact-window suite on a 7-row fixture (default query,
  full middle page, partial last page, empty page past the end); direct
  `find_all_paged` window/negative-validation tests; under
  `authz-row-level`, interleaved-row tests proving SQL-pushdown predicates
  paginate the *filtered* set (Alice's page 2 = her posts 4–6, not raw rows
  4–6) and closure predicates fall back without leaking other principals'
  rows into short pages.

### Fixed — production/security audit: WS/micro guards, pipes, and interceptors bypassed DI (silently Default-constructed)

`#[use_ws_guards]` / `#[use_micro_guards]` (and the sibling pipes /
interceptors) were activated by generated code as `<G as Default>::default()`
per message. A guard that follows the documented HTTP pattern — dependencies
injected via `CanActivate::resolve(&registry)` — silently received an empty
`Default` instance instead, so DI-backed guards on WebSocket gateways and
microservice handlers either always allowed (state tracking never
initialized) or always denied, with no compile-time signal.

- `WsCanActivate`, `WsPipeTransform`, `WsIncomingInterceptor` (nestrs-ws)
  and `MicroCanActivate`, `MicroPipeTransform`, `MicroIncomingInterceptor`
  (nestrs-microservices) each gained
  `fn resolve(&ProviderRegistry) -> Self { Self::default() }` — the exact
  shape of HTTP `CanActivate::resolve`. Override it to pull dependencies
  from the registry; stateless types that only implement `Default` are
  unaffected.
- `#[micro_routes]` now generates a second, registry-aware dispatch impl
  (`RegistryAwareMicroserviceHandler`) whose guard/pipe/interceptor arms
  instantiate via `resolve(registry)` per message (request-scoped
  semantics). `handler_factory` — the entry `#[module(microservices =
  [...])]` installs — wraps every handler in a registry-carrying
  `MicroserviceHandler`, so **all seven transport servers dispatch
  registry-aware with no server-side signature changes**. The plain
  `MicroserviceHandler` impl is still generated (registry-less dispatch of
  a bare `Arc<T>` keeps `Default` construction).
- `#[ws_routes]` likewise generates `RegistryAwareWsGateway`, and
  `#[ws_gateway]` mounts gateways through the new
  `nestrs::ws::ws_route_with_registry(gateway, registry)` so
  `serve_socket`'s dispatch resolves guards through DI per message. The
  plain `WsGateway::on_message` path and the manual `ws_route` /
  `ws_route_with_guards` / `ws_route_with_security` entry points are
  unchanged.
- **Migration (pre-1.0):** a hand-written `MicroserviceHandler` listed in
  `#[module(microservices = [...])]` must now also implement
  `RegistryAwareMicroserviceHandler` (a two-method delegation to the
  existing `handle_message` / `handle_event`, ignoring the registry); a
  hand-written `WsGateway` mounted via the `#[ws_gateway]` attribute must
  likewise implement `RegistryAwareWsGateway` (a one-method delegation to
  `on_message`). Types generated by `#[micro_routes]` / `#[ws_routes]`
  satisfy the new traits automatically.
- Tests: TCP round-trip proving a guard that admits only when
  `resolve(registry)` actually pulled a registered provider (Default denies),
  with the pipe stamping and interceptor observing its resolved instance;
  WS unit test driving `on_message_with_registry` through a real registry
  (pong frame + `pipe_via: "registry"` markers) and pinning that the plain
  dispatch still Default-constructs (guard-denied error frame).

### Fixed — production/security audit: NATS listener subscribed a subject it could never receive; wildcard handler patterns never matched

The NATS listener's wildcard subscription was built as `{prefix}>` — a
single literal token — instead of `{prefix}.>`. NATS wildcards must be their
own dot-delimited token, so the listener subscribed an ordinary subject name
that no transport ever publishes to: every request and event published to
`{prefix}.<pattern>` silently vanished. (No live-broker test exists to catch
it.) Alongside it, `#[message_pattern]` / `#[event_pattern]` handlers were
matched by literal string equality, so a declared wildcard pattern like
`user.*` or `audit.>` could never fire — the incoming pattern is always
concrete. And the listener subscribed without a queue group, so running
multiple instances of the same service meant every instance processed every
message: duplicated event side effects and racing RPC replies.

- The listener now subscribes `{prefix}.>` (pinned by a unit test asserting
  the subject shape, plus the subject/strip round-trip).
- New public helper `nestrs::microservices::pattern_matches(declared,
  incoming)` implements NATS subject-wildcard semantics: `*` matches
  exactly one dot-delimited token, a trailing `>` matches one or more
  trailing tokens, a non-final `>` compares literally. `#[micro_routes]`
  now emits guarded match arms for declared patterns containing `*` or `>`
  while plain patterns keep the literal match fast path. Literal arms are
  emitted ahead of wildcard arms, so exact patterns win regardless of
  declaration order. Wildcard patterns behave the same on every transport
  (matching runs on the concrete pattern after the namespace prefix is
  stripped); manual `MicroserviceHandler` impls can call `pattern_matches`
  directly.
- New opt-in `NatsMicroserviceOptions::with_queue_group("...")` subscribes
  the listener in a NATS queue group so horizontally scaled instances
  load-share messages (each message delivered to exactly one instance)
  instead of each processing every one.
- Unparseable payloads on the listener are now dropped with a `warn!`
  (subject, error, length — never the bytes) instead of silently, and
  failed reply publishes warn (the RPC caller's only signal would
  otherwise be its timeout).
- Tests: matcher table tests (literals, single-token `*`, trailing `>`,
  non-final `>` literal, token-count mismatches, mixed wildcards); a TCP
  transport round-trip test proving `*`/`>` patterns fire end-to-end with
  literal priority and that non-matching patterns return the standard
  "no microservice handler" error; NATS subject-shape and queue-group
  builder tests. No live NATS broker was available locally (4222 closed),
  so broker wiring is compile- and unit-verified only.

No migration needed: the previous behavior was silently broken, not relied
upon, and `with_queue_group` is additive.

### Fixed — production/security audit: Kafka rebuilt partition clients per call; RabbitMQ redelivered panicking poison messages

**Kafka.** Every operation rebuilt an rskafka `PartitionClient`, and each
construction forces a full leader discovery (multiple broker metadata
round-trips) before it serves a byte. The client built two per RPC
(requests + replies topics) and one per emit; the server listener rebuilt
one every 25 ms poll tick — even idle — and one per reply. Separately,
rskafka retries broker connections internally with **no deadline**, so
`request_timeout` was not honored mid-operation: a broker dying under an
in-flight `send_json` hung the call indefinitely, and an unreachable broker
parked reply tasks and the `kafka_cluster_reachable` liveness probe
forever, silently.

- Both the client and the server now keep one partition client per topic
  for the connection's lifetime (rskafka migrates them transparently across
  leader changes and broken connections, so the cache is safe). Cache hits
  skip leader discovery entirely.
- Every rskafka call is now bounded: client bootstrap, `get_offset`,
  produce, and each reply fetch by `request_timeout`; each server poll
  cycle by a 30 s stall cap; reply produces by the same cap; the liveness
  probe by 10 s. A failed bootstrap no longer poisons the transport — the
  next call retries.
- The server poll loop no longer ticks a fixed 25 ms sleep + partition
  rebuild. The 900 ms broker-side long-poll paces idle fetches (with a
  25 ms floor guarding brokers that return empty instantly); *errors*
  now warn and back off exponentially (250 ms → 10 s) instead of
  silently hot-retrying a dead broker ~40×/s; a poll stalled > 30 s
  (broker unreachable mid-poll) logs a warning and restarts the cycle.
  Shutdown stays responsive throughout.
- RPC semantics unchanged: correlation-id matching, at-most-once dispatch
  (offset advanced before handler spawn, `KafkaConsumerStart`), topic
  layout, wire shape.

**RabbitMQ.** A panicking handler killed the per-delivery task before its
`basic_ack`, so the message stayed unacked; the broker redelivered it on
reconnect (or after its consumer ack-timeout force-closed the channel), it
panicked again, and so on — and each poison message permanently wedged one
slot of the prefetch window. Handler dispatch (RPC and event paths) is now
wrapped in `catch_unwind`:

- a panic logs a `warn!` with the pattern and panic message, publishes an
  error reply ("microservice handler panicked") so the caller fails fast
  instead of timing out, and nacks **without requeue** — the poison message
  is dropped, not redelivered forever;
- unparseable wire payloads (already non-requeueing) now log a warning
  (length only, never the bytes);
- reply publishes ride one shared channel created at listen (was: a fresh
  channel per reply), and publish failures are logged instead of silently
  leaving the caller to time out.

Tests: Kafka — new always-on unit tests (unreachable broker fails
`send_json`/`emit_json` within `request_timeout`, both first and second
call); RabbitMQ — panic-payload extraction. The server-loop structural
change (single long-lived partition client) is pinned by construction; the
live-churn assertion pattern from the Redis suite applies when a broker is
available.

### Fixed — production/security audit: Redis transport dialed fresh connections for every RPC

Every Redis microservice operation opened new TCP connections: a client
`send_json` dialed a dedicated pubsub connection for its reply subscription
*and* a command connection for the PUBLISH; `emit_json` dialed one per emit;
and the server dialed a fresh command connection inside every reply task.
Under load that is 2N+ connections of connect/handshake/teardown per second
against the Redis server — connection storms, fd exhaustion risk, and added
per-RPC latency. The redis dependency now enables its `connection-manager`
feature.

- **Client `send_json`** now runs over one fixed pair of long-lived
  connections shared by all calls (and all clones of the transport): a
  reconnecting `ConnectionManager` for PUBLISHes, and a single dedicated
  pubsub connection owned by a pump task that routes each reply message to
  its waiting RPC by exact channel. Per-RPC reply channels are still unique
  unguessable UUIDs, still subscribed *before* the request is published (the
  subscription ack is awaited), and still released immediately after the
  reply (or timeout) — no per-RPC subscription or connection buildup. The
  correlation-id check, wire shape, and error mapping are unchanged.
- **`emit_json`** publishes over the shared manager (with a
  `request_timeout`-bounded publish).
- **Server** creates one `ConnectionManager` at `listen` and reuses it for
  every reply; if Redis restarts, the manager reconnects transparently. The
  existing long-lived wildcard pubsub subscription is unchanged.
- **Failure semantics**: a dead pubsub connection (Redis restart) restarts
  the pump on the next call; connect attempts are bounded by
  `request_timeout` so a black-holed address fails the RPC instead of
  hanging callers queued on the shared-connection lock.
- **Tests**: 2 new always-on unit tests (unreachable Redis fails fast
  without hanging; failed connects don't poison the transport) and a new
  opt-in live suite `nestrs-microservices/tests/redis_transport_live.rs`
  (set `NESTRS_TEST_REDIS_URL`) asserting sequential + concurrent RPCs,
  error replies, and emits all run on a fixed connection set
  (`INFO clients`'s `connected_clients` is stable across the whole workload).

### Fixed — production/security audit: TCP microservice transport had unbounded frames and no timeouts

The TCP transport (`nestrs::microservices` TCP server + `TcpTransport`) read
newline-delimited frames with unbounded line readers on both ends, spawned a
task per accepted connection with no cap, and never timed anything out. A
peer that streamed bytes without ever sending a newline grew server (and
client) memory without bound; silent connections and unanswered RPCs hung
forever.

- **Bounded frames (1 MiB default)** — frames are now read with
  `MAX_FRAME_BYTES` caps on both ends. A frame past the cap gets a generic
  `frame rejected` error frame (never echoing attacker bytes) and the
  connection is dropped. The reader accumulates via `fill_buf`/`consume`, so
  pipelined frames queued behind a rejected one are not lost on healthy
  connections.
- **Server idle timeout (30 s)** — a silent or half-open connection is
  dropped, releasing its task and buffered bytes.
- **Client whole-RPC timeout (30 s)** — `TcpTransport::send_json` /
  `emit_json` bound connect + write + read: a server that accepts and never
  responds fails the call instead of hanging the caller (the client's own
  response read is capped too).
- **Connection cap (1024 concurrent)** — past the cap new connections are
  closed immediately with a warning (fail-fast) instead of spawning unbounded
  tasks.
- `MAX_FRAME_BYTES` is re-exported from `nestrs::microservices` so tests and
  deployments can assert against the exact wire budget.
- **4 new tests** (`nestrs/tests/tcp_frame_hardening.rs`, feature
  `microservices`): oversized-frame rejection + connection drop, pipelined
  frames surviving the capped reader, idle-connection drop, and client
  timeout against a silent server (the two timeout tests run under tokio's
  paused clock, so they exercise the real 30 s paths in milliseconds).

### Fixed — production/security audit: `files` doc example taught an arbitrary-file-read footgun

The `stream_file_or_response` doc example streamed `upload_dir().join(&p.name)`
straight from a path parameter. Path extractors percent-decode before the
handler sees the value, so `/download/..%2F..%2Fetc%2Fpasswd` served any file
the process can read — and the pattern was the documented one.

- **New `nestrs::files::stream_file_from_dir(base, name, content_type)`** —
  the safe shape for `/download/:name` handlers: `name` must be a single path
  component (separators, `.`/`..`, NUL rejected — Windows also rejects `:`),
  and the resolved path is canonicalized and required to stay inside `base`,
  so a symlink planted in the directory pointing outside is a 404, not a
  served file. Invalid names → 400; missing → 404; other I/O errors → 500.
- The `stream_file_or_response` / `stream_file_with_content_type` docs now
  show the safe helper and explicitly direct request-derived paths to it.
- **5 new tests** (`nestrs/tests/files_traversal.rs`, feature `files`):
  valid stream, rejection of traversal/absolute/degenerate names, missing
  file 404, symlink-plant containment, and an end-to-end route proving
  percent-decoded `..%2F..%2F` is rejected after extraction.
- `examples/lab lab3_files` now uses the library helper instead of its
  private sanitizer (same behavior, one implementation).

### Fixed — production/security audit: unauthenticated health-probe endpoints were unbounded and leaked error detail

The fixed `/__nestrs/health/{live,ready,startup}` endpoints mount outside
every middleware layer (they must stay reachable under load shedding), which
made them an unauthenticated surface with no execution bound: every hit ran
the `DatabaseIndicator` / `HttpIndicator` readiness work (DB pings, outbound
dependency GETs) — a probe storm (or an attacker) amplified directly into
the dependencies — and Down responses carried raw error text (internal route
paths, DB endpoints, dependency URLs, panic payloads) to any unauthenticated
caller. A panicking indicator unwound the connection task entirely.

- **Panic guard** — probe execution now runs on its own task: a panicking
  indicator or stamped handler becomes a 503 with a generic message instead
  of a dropped connection.
- **Generic Down messages** — responses say *that* a check failed (plus
  failing indicator names), never *why*: the mirrored handler's
  method/path/status, indicator error text, and panic payloads go to
  `tracing` only.
- **Short-TTL cache with in-flight coalescing** — liveness and readiness
  outcomes are cached for 5s (k8s default `periodSeconds` is 10), and
  concurrent probes share one execution, capping indicator execution at one
  run per window no matter how fast the endpoint is hammered. Deliberately
  *not* a 429-style rate cap: k8s treats any non-2xx probe as a failure and
  restarts the pod, so rate-limiting a probe would fail the probe.
- The `enable_readiness_check` endpoint (`/ready`-style, user-configured
  path) keeps its NestJS-terminus response shape — it sits inside the normal
  middleware stack (rate limit, CatchPanic when enabled) and its
  per-indicator detail is the documented terminus contract.
- **5 new tests** — panicking indicator → 503 (not a dropped connection),
  stamped panicking handler → 503 (own binary: stamps are process-global),
  aggregation redaction (indicator names in, `postgres://secret-host:5432`
  out), and 8-request concurrent-burst coalescing to exactly one indicator
  execution + TTL cache follow-ups.

### Fixed — production/security audit: rate limiter / throttler ignored the declared proxy topology

An app behind a reverse proxy calling `use_trusted_proxy_headers(n)` but not
re-stating the hop count on the limiter options got every proxied client keyed
on the proxy's own address — one shared budget, collective 429s for unrelated
users, and a trivially-exhausted limiter by a single abuser.

- **The rate limiter and throttler now inherit the app-level hop count by
  default.** `use_trusted_proxy_headers(1)` is the single source of truth:
  `RateLimitOptions` (private field, builder unchanged) and
  `ThrottlerOptions::trusted_proxy_hops` both resolve client identity exactly
  like the `ClientIp` extractor. Set the limiter-level value only to override;
  a value that diverges from the app topology logs a `tracing::warn!` (one
  side keys on the proxy address while the other trusts forwarded headers —
  almost always a misconfiguration).
- **`ThrottlerOptions::trusted_proxy_hops` changed type `u16` → `Option<u16>`**
  (pre-1.0): `None` (new default) = inherit, `Some(hops)` = explicit override,
  `Some(0)` = deliberately distrust forwarded headers. Migration: add `Some(`
  around any literal. The `RateLimitOptions::trusted_proxy_hops` builder keeps
  its `u16` signature.
- **`ThrottlerGuard` no longer hardcodes 0 hops** — the guard runs at route
  level, inside the trusted-proxy middleware, so it now reads the per-request
  hop count installed by `use_trusted_proxy_headers` (falling back to 0 when
  no topology was declared). Previously every request hit the guard as one
  client regardless of the declared topology.
- **5 new tests** — limiter inheritance + exhausted-bucket pin, explicit
  `Some(0)` override (shared bucket), throttler inheritance, throttler
  explicit override, and guard-per-request topology resolution.

### Fixed — production/security audit: `#[dto]` validation markers were silent no-ops

- **`#[IsUUID]` now validates** — the marker was previously stripped without
  emitting anything, so NestJS migrants writing `#[IsUUID] id: String` got no
  UUID check and garbage IDs flowed into downstream services. The `#[dto]`
  macro now rewrites it to `#[validate(custom(function = "nestrs::is_uuid"))]`
  (new public helper `nestrs::is_uuid`): a runtime canonical 8-4-4-4-12
  hexadecimal check (any version/variant, nil UUID included — the
  `@IsUUID("all")` equivalent) producing a 422 with an `isUuid` constraint
  through `ValidatedBody` / `ValidatedQuery` / `ValidatedPath` /
  `ValidationPipe`. `Option<String>` fields skip validation on `None`.
  Fields typed `uuid::Uuid` keep the marker as a satisfied no-op — serde
  already rejects malformed UUIDs at the JSON boundary.
- **Marker/type contradictions are now compile errors** — `#[IsString]`,
  `#[IsBoolean]`, `#[IsInt]`, `#[IsNumber]`, and `#[IsUUID]` on a
  non-matching field type (e.g. `#[IsString]` on `i64`) used to be silently
  swallowed, validating nothing. The `#[dto]` macro now fails the build with
  an actionable message pointing at the field type. Type-matching uses
  (e.g. `#[IsString]` on `String`, `#[IsInt]` on `i32`, all existing
  workspace/docs examples) compile unchanged.
- **7 new tests** in `nestrs/tests/dto_validation_markers.rs` (canonical v4,
  nil UUID, garbage rejection, wrong group lengths, `Option` skip/validate,
  helper parity with class-validator's `isUuid("all")` semantics).

### Security — CSWSH defence (WebSocket Origin allowlist)

- **CSWSH (Cross-Site WebSocket Hijacking) defence for `nestrs-ws`** —
  `ws_route` and `ws_route_with_guards` previously accepted WebSocket
  upgrades from any `Origin`. New `ws_route_with_security` /
  `ws_route_with_guards_and_security` entry points take an explicit
  `WsSecurityConfig` allowlist and reject mismatched origins (with
  `null` always rejected when an allowlist is configured). Origin
  rejection on the new entry points is a hard HTTP 403; on the
  guards-and-security entry point it short-circuits before the
  guard chain. Legacy `ws_route` / `ws_route_with_guards` are
  retained for callers behind a trusted reverse proxy that enforces
  its own allowlist; they call through with `WsSecurityConfig::allow_off()`.
- **8 new tests** in `nestrs-ws/tests/cswsh.rs` cover allow_off,
  allowlist acceptance, disallowed-origin 403, `null` rejection,
  `require_origin`, and security-before-guard ordering. All 18
  `nestrs-ws` tests pass.

### Added — Wave 4.5: public-API snapshot test (CI gate for breaking changes)

- **Public-API snapshot test** for the five published crates
  (`nestrs`, `nestrs-core`, `nestrs-graphql`, `nestrs-ws`,
  `nestrs-mcp`). `STABILITY.md` documented the policy; this wave
  enforces it.
- **`scripts/ci/public_api_snapshot.py`** — generates one snapshot
  per crate by reading `rustdoc --output-format json` (nightly-only)
  and emitting a sorted list of fully-qualified public item paths to
  `tests/api-snapshots/<crate>.txt`. Filters out `#[doc(hidden)]`
  items and `__nestrs_*` macro-internal helpers (per
  `STABILITY.md`'s "not stable" policy).
- **`--check` mode** — regenerates and diffs against committed
  snapshots, prints added/removed lines on drift, exits 1.
- **CI gate** — new `public-api-snapshot` job in
  `.github/workflows/ci.yml` running on the nightly toolchain
  (rustdoc JSON output is nightly-only; the existing
  `1.88.0`/`stable`/`beta` rows continue to enforce the
  build/test matrix).
- **Why hand-rolled vs `cargo-public-api`** — `cargo-public-api`
  reads the same rustdoc JSON, but pulling it from crates.io
  required an install the local auto-mode classifier couldn't
  grant. Reading the JSON directly with stdlib `json` keeps the
  gate running without external tooling. Same fields, same output
  format.

### Added — Wave 4.3: DB migrations + seeding CLI (`nestrs-cli db` subcommands)

- **`nestrs-cli db` subcommand family** behind a default-OFF `db`
  Cargo feature on `nestrs-cli` (crate `nestrs-scaffold`). The `db`
  arm of the dispatcher errors with a clear message when the feature
  is off, so the binary stays minimal for users who don't need it.
- **Migrations** (`nestrs-cli db [--backend sqlx|prisma] migrate ...`)
  via `sqlx::migrate::Migrator` (pinned to `=0.8.6` to match the
  workspace):
  - `add <name> [--reversible]` — writes
    `<NNN>_<name>.sql` (or `.up.sql` + `.down.sql` when
    `--reversible`). Sequence is walked from the existing directory,
    not timestamped (deterministic, greppable, sortable; sqlx-cli's
    timestamped convention is replaced by a simpler counter).
  - `run [--path <dir>] [--target-version V] [--database-url URL]` —
    applies pending migrations; idempotent on already-applied sets.
  - `revert [--target-version V]` — sqlx 0.8 requires an explicit
    target; defaults to "max applied − 1" so a no-flag revert undoes
    just the most recent.
  - `info [--path <dir>]` — prints applied (from `_sqlx_migrations`)
    and on-disk files with `applied` / `pending` / `[missing on
    disk]` markers, plus a count line.
  - `--backend prisma` pass-throughs to `npx prisma migrate dev
    --create-only` (add), `npx prisma migrate deploy` (run), and
    `npx prisma migrate status` (info). `revert` is not supported
    on the Prisma backend (Prisma has no first-class revert; we
    surface that as a clear error pointing at `npx prisma migrate
    resolve --rolled-back`).
- **Seeding** (`nestrs-cli db seed ...`):
  - `--bin <name> [--manifest-path <path>]` — pass-through to
    `cargo run --bin <name>`, forwards `DATABASE_URL` and
    `NESTRS_DB__URL` env, propagates exit code. User owns the seed
    binary; no compile-on-the-fly magic.
  - `--seed-file <path>` — execute a SQL file via `sqlx::raw_sql`
    inside an explicit transaction; rollback on error. TypeORM-style
    escape hatch for `.sql` fixture dumps.
- **URL resolution** — `--database-url` > `DATABASE_URL` >
  `NESTRS_DB__URL`. Same precedence in every subcommand; documented
  in `--help`.
- **12 tests** in `nestrs-cli/tests/db_cli.rs`: 4 on `migrate add`
  (filename shape, `--reversible`, sequence increment, invalid name
  rejection), 4 on `migrate run/revert/info` against a SQLite
  fixture, 2 on `seed --bin` (exit code propagation + env
  forwarding), 2 on `seed --seed-file` (apply + transactional
  rollback on bad SQL). All pass on MSRV 1.88.
- **Postgres parity is documented as a follow-up.** SQLite-only CI
  fixture; sqlx's `Migrator` uses the same `Any`-driver code path on
  Postgres, but driver-specific quirks (e.g. `_sqlx_migrations`
  quote escaping) aren't exercised here.
- **Out of scope** (documented in module-level docs of
  `nestrs-cli/src/db.rs`): typed `DatabaseConfig` on `nestrs`
  (separate wave's work; CLI reads env directly), `synchronize`
  full DDL diff (multi-month port), `nestrs-cli generate seed`
  scaffolding, public-API snapshot test, crate-split study.

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
- **`#[dto]` derives `schemars::JsonSchema`** (upstream `#[input]` parity):
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
  surfaces upstream ships as separate crates, as one feature-gated crate:
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
