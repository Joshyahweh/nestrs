# nestrs-openapi

**OpenAPI 3.1 JSON + Swagger UI** for [nestrs](https://crates.io/crates/nestrs) HTTP routes registered in [`nestrs_core::RouteRegistry`](https://docs.rs/nestrs-core) (via `impl_routes!` / `#[routes]`). Serves **`/openapi.json`** (configurable) and a **`/docs`** Swagger page by default.

**Docs:** [docs.rs/nestrs-openapi](https://docs.rs/nestrs-openapi) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## What you get (vs Nest `@nestjs/swagger`)

| Area | nestrs-openapi |
|------|-----------------|
| Paths & methods | **Yes** — from the route registry. |
| Swagger UI | **Yes** — static HTML + CDN `swagger-ui-dist`. |
| Operation summary | **Heuristic** by default; override with **`#[openapi(summary = "...")]`** on the handler. |
| Tags | **Heuristic** from the path; override with **`#[openapi(tag = "...")]`**. |
| Document `tags`, `servers`, `components`, `security` | **Optional** — set on [`OpenApiOptions`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html). |
| Per-route **security** (heuristic) | **Optional** — [`OpenApiOptions::infer_route_security_from_roles`](https://docs.rs/nestrs-openapi/latest/nestrs_openapi/struct.OpenApiOptions.html#structfield.infer_route_security_from_roles) + `roles_security_scheme` when handlers use **`#[roles]`** (metadata); still define `components.securitySchemes`. |
| Request/response **schemas** from DTOs | **No** in-core — hand-write **`components.schemas`** or merge fragments from **`utoipa`** / **`okapi`** / codegen; see repo mdBook [**OpenAPI & HTTP**](../docs/src/openapi-http.md). |
| Per-route `@ApiOperation` / `@ApiResponse` | **Partial** — `#[openapi(summary = \"...\", tag = \"...\", responses = ((404, \"...\"), ...))]`; otherwise default `200 OK`. |

## Install

```toml
[dependencies]
nestrs-openapi = "0.3.3"
axum = "0.7"
```

Or:

```toml
nestrs = { version = "0.3.3", features = ["openapi"] }
```

Use `NestFactory::create(..).enable_openapi()` or `enable_openapi_with_options(..)`; **`api_prefix`** is filled from your global prefix + URI version when applicable.

## Example (standalone router)

```rust
use axum::Router;
use nestrs_openapi::{openapi_router, OpenApiOptions};
use serde_json::json;

fn docs_routes() -> Router {
    openapi_router(OpenApiOptions {
        title: "My API".into(),
        version: "0.1.0".into(),
        json_path: "/openapi.json".into(),
        docs_path: "/docs".into(),
        api_prefix: "/api/v1".into(),
        servers: Some(vec![json!({ "url": "https://api.example.com", "description": "Production" })]),
        document_tags: Some(vec![json!({ "name": "users", "description": "User operations" })]),
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
        infer_route_security_from_roles: false,
        roles_security_scheme: "bearerAuth".into(),
    })
}
```

Merge the returned `Router` into your application. Set **`api_prefix`** so documented paths match real URLs.

## License

MIT OR Apache-2.0.
