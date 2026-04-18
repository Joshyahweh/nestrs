# Custom decorators (Nest → Rust)

In **NestJS**, *custom decorators* usually mean:

1. **Parameter decorators** (`@User()`, `@Req()`, …) built with `createParamDecorator` and `ExecutionContext`.
2. **Class/method metadata** (`SetMetadata`, reflectable keys) read later by guards, interceptors, or pipes.

**nestrs** is **Rust**: there is **no** TypeScript/JavaScript runtime or `reflect-metadata`. “Decorators” are **proc macros** and **explicit types**. This page maps Nest patterns to what **nestrs** supports today — **Partial** parity: you can attach **string metadata** to handlers and read it from guards; you cannot arbitrarily replicate every TS decorator without writing macros or extractors yourself.

## 1) Route and handler metadata (closest to `SetMetadata`)

### Declarative attributes (generated into registration)

On route methods inside `#[routes]`:

- **`#[set_metadata("key", "value")]`** — attaches metadata to the **handler key** (module path + function name) when the route is registered.
- **`#[roles("admin", "editor")]`** — shorthand that sets the **`roles`** metadata key to a comma-separated string (used with role-aware guards).

Under the hood, `impl_routes!` calls **`MetadataRegistry::set`** for each pair, using the same handler key that is stored on the request as **`HandlerKey`** during dispatch.

### Imperative API

Any code (e.g. tests or dynamic registration) can call:

```rust
use nestrs::core::MetadataRegistry;

MetadataRegistry::set("my_app::controllers::UserController::list", "roles", "admin");
let value = MetadataRegistry::get("my_app::controllers::UserController::list", "roles");
```

Keys are plain **strings**; values are **strings**. There is no nested JSON object store in the registry — serialize JSON yourself if you need structured data.

### Reading metadata in a guard

Guards run with access to **`axum::http::request::Parts`**. nestrs inserts **`HandlerKey(&'static str)`** into request extensions before your route handler runs. Built-in helpers such as **`route_roles_csv`** read the **`roles`** metadata for the current handler.

See `nestrs::security` (e.g. `AuthStrategyGuard`, `XRoleMetadataGuard`) and `nestrs/tests/cross_cutting_extended.rs` for examples.

### Worked example: `#[roles]` + `XRoleMetadataGuard`

[`XRoleMetadataGuard`](https://docs.rs/nestrs/latest/nestrs/security/struct.XRoleMetadataGuard.html) is a small reference guard: it reads **`HandlerKey`** from the request, loads the **`roles`** string from [`MetadataRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.MetadataRegistry.html) (populated by `#[roles(...)]`), and compares it to the **`x-role`** header (comma-split allow list on the handler side, single role from the client in this demo).

**Application code** (no test-only reset guards—use real auth in production):

```rust
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api", version = "v1")]
struct DocsController;

#[routes(state = AppState)]
impl DocsController {
    /// `#[roles("admin")]` registers metadata; the guard enforces it.
    #[get("/admin-only")]
    #[roles("admin")]
    #[use_guards(XRoleMetadataGuard)]
    async fn admin_only() -> &'static str {
        "ok"
    }
}

#[module(controllers = [DocsController], providers = [AppState])]
struct AppModule;
```

Call the route with `x-role: admin` to receive **200**; `x-role: user` yields **403**. In real services, replace the header check with a JWT claim, session, or [`AuthStrategy`](https://docs.rs/nestrs/latest/nestrs/security/trait.AuthStrategy.html) implementation—keep **`#[roles]`** + OpenAPI inference ([OpenAPI & HTTP](openapi-http.md)) as **documentation** of intent even when enforcement lives in a different guard.

### Worked example: validation + `ValidationPipe` (closest to `@Body()` + `ValidationPipe`)

On **`#[routes]`**, add **`#[use_pipes(ValidationPipe)]`** on the handler (or controller) and use **`#[param::body]`**, **`#[param::query]`**, or **`#[param::param]`** with a **`#[dto]`** type. Field constraints use the nestrs DTO attributes (for example **`#[IsEmail]`**, **`#[Length(...)]`**, **`#[validate(...)]`**)—see **`nestrs/tests/param_decorators_and_pipes.rs`** for query, path, and body coverage.

```rust
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[dto]
struct SignupDto {
    #[IsEmail]
    email: String,
}

#[controller(prefix = "/api", version = "v1")]
struct ExampleController;

#[routes(state = AppState)]
impl ExampleController {
    #[post("/signup")]
    #[use_pipes(ValidationPipe)]
    async fn signup(#[param::body] dto: SignupDto) -> &'static str {
        let _ = dto;
        "ok"
    }
}

#[module(controllers = [ExampleController], providers = [AppState])]
struct AppModule;
```

Some code paths also accept **`ValidatedBody<SignupDto>`** tuple extractors (see `nestrs/tests/bootstrap_composition.rs` **`POST /validate`**). Invalid payloads return **422** with a structured error body.

## 2) Parameter “decorators” (closest to `createParamDecorator`)

Nest parameter decorators hide extraction from `ExecutionContext`. In nestrs, extraction is **type-driven**:

| Nest-style idea | nestrs approach |
|-----------------|-----------------|
| Body DTO | **`#[param::body]`** + **`#[dto]`** + **`#[use_pipes(ValidationPipe)]`**, or **`ValidatedBody<T>`** tuple style |
| Query DTO | **`#[param::query]`** with **`#[dto]`** + **`ValidationPipe`**, or **`ValidatedQuery<T>`** |
| Path params | **`#[param::param]`** with **`#[dto]`**, or **`ValidatedPath<T>`** |
| Raw request | **`#[param::req]`** → `Request` |
| Headers | **`#[param::headers]`** → `HeaderMap` |
| Client IP | **`#[param::ip]`** or **`ClientIp`** extractor (see `param_decorators_and_pipes` tests) |

These use **attributes on parameters** (`#[param::body]`, etc.) expanded by **`#[routes]`**, not runtime reflection. Adding a **new** extractor shape means **new types and/or proc macros**, not a single `createParamDecorator` API.

## 3) Built-in method/controller attributes (decorator-like)

Familiar Nest-adjacent attributes include:

- HTTP mapping: **`#[get]`, `#[post]`, …**
- **`#[controller(...)]`**, **`#[module(...)]`**, **`#[injectable]`**
- Cross-cutting: **`#[use_guards(...)]`**, **`#[use_pipes(...)]`**, **`#[use_interceptors(...)]`**, **`#[use_filters(...)]`**
- Response shaping: **`#[http_code(201)]`**, **`#[response_header(...)]`**, **`#[redirect(...)]`**
- Versioning: **`#[ver("v2")]`**, controller **`version = "v1"`**
- OpenAPI (feature **`openapi`**): **`#[openapi(summary = "…", tag = "…", responses = ((200, "…"), …))]`**
- WebSocket / microservice handlers have their own attribute sets (`#[subscribe_message]`, `#[message_pattern]`, …).

Each is implemented as a **proc macro** in **`nestrs-macros`**; see [docs.rs/nestrs-macros](https://docs.rs/nestrs-macros) and the crate source for the exact token streams.

## 4) Rolling your own “custom decorator”

Practical options:

1. **Metadata only** — use **`#[set_metadata]`** + read **`MetadataRegistry`** from a **`CanActivate`** implementation or middleware (with **`HandlerKey`**).
2. **New route attributes** — fork or extend **`nestrs-macros`** (or a workspace proc-macro crate) to emit the same `impl_routes!` / registration patterns.
3. **New extractors** — Axum **`FromRequestParts`** / **`FromRequest`** types; optionally wrap with validation types similar to **`ValidatedBody`**.

What you **do not** get out of the box:

- A **single** Nest-style API to attach arbitrary reflectable metadata to arbitrary parameters at runtime.
- **Automatic** discovery of ad-hoc attributes on random functions without macro expansion.

## Summary

| Nest feature | nestrs status |
|--------------|----------------|
| `SetMetadata` / metadata on handlers | **Yes** — `#[set_metadata]`, `#[roles]`, `MetadataRegistry`, `HandlerKey` |
| Guards reading metadata | **Yes** — pattern established; built-ins for roles/strategies |
| Custom parameter decorators | **Partial** — use Axum extractors + `#[param::…]` / macros; no TS-style `createParamDecorator` |
| Reflect / generic decorator runtime | **No** — compile-time Rust only |

For parity expectations across the framework, see [Roadmap parity](roadmap-parity.md). For **`NestApplication`** methods that pair with guards and filters (CORS, global exception filter, etc.), see the [API cookbook](appendix-api-cookbook.md).
