# nestrs-graphql

**GraphQL HTTP router** for [nestrs](https://crates.io/crates/nestrs) using **[async-graphql](https://github.com/async-graphql/async-graphql)** and **Axum**: single endpoint serves **Playground (GET)** and **batch queries (POST)**.

Includes **`limits`** helpers (`with_default_limits`, depth/complexity defaults) and **`with_production_graphql_limits`** (adds the **`Analyzer`** extension) for production APIs.

**HTTP options:** `graphql_router_with_options` + **`GraphQlHttpOptions`** (e.g. toggle Playground).

**SDL:** `export_schema_sdl` / `export_schema_sdl_with_options` + **`SDLExportOptions`** for tooling and subgraph workflows.

**Nest parity vs ecosystem:** Federation, custom extensions (“plugins”), codegen, and field-level auth are covered by **async-graphql** and external tools (Apollo Router, `graphql-client`, etc.); see crate-level docs on `docs.rs`.

**Docs:** [docs.rs/nestrs-graphql](https://docs.rs/nestrs-graphql) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-graphql = "0.3.0"
async-graphql = "7.0.17"
axum = "0.7"
```

Or:

```toml
nestrs = { version = "0.3.0", features = ["graphql"] }
```

## Example

```rust
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
use nestrs_graphql::graphql_router;

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn hello(&self) -> &str {
        "world"
    }
}

fn main() {
    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish();
    let router = graphql_router(schema, "/graphql");
    // nest: merge `router` into your application Router (e.g. .merge(router))
    drop(router);
}
```

Apply **`with_default_limits`** on the **`SchemaBuilder`** before **`finish()`** if you want bounded depth/complexity:

```rust
use nestrs_graphql::with_default_limits;

let schema = with_default_limits(async_graphql::Schema::build(
    QueryRoot,
    EmptyMutation,
    EmptySubscription,
))
.finish();
```

Stricter production wiring (same limits + **`Analyzer`**):

```rust
use nestrs_graphql::with_production_graphql_limits;

let schema = with_production_graphql_limits(async_graphql::Schema::build(
    QueryRoot,
    EmptyMutation,
    EmptySubscription,
))
.finish();
```

Disable Playground but keep the batch POST handler:

```rust
use nestrs_graphql::{graphql_router_with_options, GraphQlHttpOptions};

let router = graphql_router_with_options(
    schema,
    "/graphql",
    GraphQlHttpOptions { enable_playground: false },
);
```

Export SDL for routers or codegen:

```rust
use nestrs_graphql::{export_schema_sdl, SDLExportOptions};

let sdl = export_schema_sdl(&schema);
// Federation-shaped SDL: export_schema_sdl_with_options(&schema, SDLExportOptions::default().federation())
```

## License

MIT OR Apache-2.0.
