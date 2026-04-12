# nestrs-openapi

**OpenAPI 3.1 JSON + Swagger UI** for [nestrs](https://crates.io/crates/nestrs) HTTP routes registered through the framework’s **`RouteRegistry`** (populated by `#[routes]`). Serves **`/openapi.json`** (configurable) and a **`/docs`** Swagger page by default.

**Docs:** [docs.rs/nestrs-openapi](https://docs.rs/nestrs-openapi) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-openapi = "0.1.3"
axum = "0.7"
```

Or:

```toml
nestrs = { version = "0.1.3", features = ["openapi"] }
```

## Example

```rust
use axum::Router;
use nestrs_openapi::{openapi_router, OpenApiOptions};

fn docs_routes() -> Router {
    openapi_router(OpenApiOptions {
        title: "My API".into(),
        version: "0.1.0".into(),
        json_path: "/openapi.json".into(),
        docs_path: "/docs".into(),
        api_prefix: "/api/v1".into(),
    })
}
```

Merge the returned `Router` into your nestrs application router. Set **`api_prefix`** to match your global prefix + API version so documented paths align with real routes.

## License

MIT OR Apache-2.0.
