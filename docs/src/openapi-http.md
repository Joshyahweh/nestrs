# OpenAPI & HTTP DX

This chapter closes the main gap versus Nest **`@nestjs/swagger`**: **request/response schemas** are not derived from Rust types in the core generator, but you can **compose** a full document with **`OpenApiOptions.components`**, **`utoipa`**, **`okapi`**, or hand-written JSON. **Security** in Swagger is supported via **`components.securitySchemes`**, global **`security`**, and an optional **heuristic** that maps **`#[roles]`** metadata to per-operation **`security`**.

## What nestrs generates today

- **`paths`**: every HTTP route registered through `impl_routes!` / `#[routes]` (via [`RouteRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.RouteRegistry.html)).
- **Operation fields**: `operationId`, `summary` (from handler name or `#[openapi(summary = "...")]`), `tags` (from path or `#[openapi(tag = "...")]`), `responses` (default **200** or `#[openapi(responses = ((code, "description"), ...))]`).
- **Not generated**: request bodies, query/path/header **schemas**, and links from DTOs — same limitation as noted in [`nestrs-openapi`](../../nestrs-openapi/README.md).

Use [`NestApplication::enable_openapi`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_openapi) or [`enable_openapi_with_options`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_openapi_with_options) with [`OpenApiOptions`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html).

**Minimal wiring** (requires `features = ["openapi"]` on `nestrs`):

```toml
[dependencies]
nestrs = { version = "0.3.8", features = ["openapi"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .enable_openapi() // GET /openapi.json, GET /docs (Swagger UI)
        .listen(3000)
        .await;
}
```

For **`components`**, **`security`**, and **`infer_route_security_from_roles`**, replace `enable_openapi()` with `enable_openapi_with_options(OpenApiOptions { ... })` as in the sections below.

---

## Schema story: manual `components`

Put JSON Schema objects under **`components.schemas`** and refer to them from your own tooling, codegen, or a later merge step. Nest’s `@ApiProperty` maps to **you** maintaining schema JSON (or generating it elsewhere).

```rust
use nestrs_openapi::OpenApiOptions;
use serde_json::json;

OpenApiOptions {
    components: Some(json!({
        "schemas": {
            "UserDto": {
                "type": "object",
                "required": ["id", "email"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "email": { "type": "string", "format": "email" }
                }
            }
        }
    })),
    ..Default::default()
}
```

**Per-route response `$ref`:** the built-in `#[openapi(responses = …)]` attribute today only sets **status + description** (no `content` / `$ref`). To attach `content.application/json.schema.$ref` to specific operations, either:

- extend the document **after** serializing `openapi.json` (middleware, build script), or  
- contribute an enhancement to `OpenApiRouteSpec` / `#[openapi]` if you need first-class support.

---

## Optional: **utoipa** (Axum-friendly)

[`utoipa`](https://crates.io/crates/utoipa) can derive **`OpenApi`** metadata and **schemas** from Rust types. Typical pattern with nestrs:

1. Define DTOs / path types with `utoipa`’s `ToSchema`, `IntoParams`, etc.
2. Build a small `utoipa` **`OpenApi`** (often only `components.schemas` / `securitySchemes`).
3. Serialize that fragment to [`serde_json::Value`](https://docs.rs/serde_json) and **merge** into [`OpenApiOptions.components`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html#structfield.components) (deep-merge `schemas` / `securitySchemes` maps so nestrs and utoipa can coexist).

Exact merge code depends on your `utoipa` version (e.g. `OpenApi` may implement `Serialize`, or you merge via `utoipa::openapi::RefOr` helpers). The important part: **one** OpenAPI document is still served from nestrs; utoipa supplies **fragments**, not a second router.

---

## Optional: **okapi** (Rocket)

[`okapi`](https://crates.io/crates/okapi) targets **Rocket**. nestrs is **Axum**-first, so okapi is not wired in-tree. The same **recipe** applies: generate OpenAPI JSON (or components) from okapi in a Rocket-specific crate or subproject, then **merge** `components` into `OpenApiOptions` if you share types across stacks, or maintain parallel schema definitions.

---

## Security: `securitySchemes` and `security`

### Global security (all operations)

Declare schemes under **`components.securitySchemes`** and optional root **`security`** on [`OpenApiOptions`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html):

```rust
use serde_json::json;
use nestrs_openapi::OpenApiOptions;

OpenApiOptions {
    components: Some(json!({
        "securitySchemes": {
            "bearerAuth": {
                "type": "http",
                "scheme": "bearer",
                "bearerFormat": "JWT"
            }
        }
    })),
    security: Some(vec![json!({ "bearerAuth": [] })]),
    ..Default::default()
}
```

### Per-operation security (heuristic from `#[roles]`)

If a handler uses **`#[roles("admin")]`** (or other roles), the macros register **`roles`** metadata on the handler key in [`MetadataRegistry`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.MetadataRegistry.html). When you set:

- [`OpenApiOptions::infer_route_security_from_roles`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html#structfield.infer_route_security_from_roles) = `true`
- [`OpenApiOptions::roles_security_scheme`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html#structfield.roles_security_scheme) = `"bearerAuth"` (or any key matching `components.securitySchemes`)

then **only those operations** get an OpenAPI **`security`** array with that scheme. Routes **without** `#[roles]` are unchanged.

This is **heuristic**: it keys off **metadata**, not runtime guard types or `CanActivate` implementations. Custom guards that do not set `roles` metadata will not trigger the hint unless you also add `#[roles(...)]` or `#[set_metadata("roles", "...")]` for documentation purposes.

---

## Merging **utoipa** `OpenApi` fragments

A typical workflow:

1. Derive **`utoipa::ToSchema`** (and **`IntoParams`**, etc.) on your request/response types.  
2. Build a small **`utoipa::OpenApi`** instance that only lists **`components.schemas`** (and optionally **`securitySchemes`**).  
3. Serialize that fragment to [`serde_json::Value`](https://docs.rs/serde_json) and assign it to [`OpenApiOptions.components`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html#structfield.components), **deep-merging** with any manual JSON you already pass (so nestrs-generated **`paths`** stay authoritative).  
4. Serve with **`enable_openapi_with_options`** so **`GET /openapi.json`** reflects both route discovery and your schemas.

Exact merge code depends on your **utoipa** major version; keep merge logic in one module so upgrades stay localized.

## Troubleshooting

| Issue | What to check |
|-------|----------------|
| **`/docs` or `/openapi.json` 404** | `features = ["openapi"]` on **`nestrs`**; call **`enable_openapi()`** before **`listen`**. |
| Routes missing from document | Handlers must use **`#[routes]`** / `impl_routes!` so they register in **`RouteRegistry`**. |
| Schemas empty | Core does not infer DTOs—populate **`OpenApiOptions.components`** or merge **utoipa** (above). |
| **`security` not per route** | Set **`infer_route_security_from_roles`** and **`roles_security_scheme`**; add **`#[roles]`** or metadata for those routes. |
| **`422` validation not in OpenAPI** | Document request bodies manually under **`components`** until first-class `content` support lands for every route. |

---

## See also

- [API cookbook](appendix-api-cookbook.md) — `enable_openapi` / `enable_openapi_with_options` snippets  
- Crate README: [`nestrs-openapi/README.md`](../../nestrs-openapi/README.md)
- Security patterns: [Security](security.md)
- Roadmap row: [Roadmap parity](roadmap-parity.md) → OpenAPI / Swagger
