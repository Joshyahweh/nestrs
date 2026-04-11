# nestrs-graphql

**GraphQL HTTP router** for [nestrs](https://crates.io/crates/nestrs) using **[async-graphql](https://github.com/async-graphql/async-graphql)** and **Axum**: single endpoint serves **Playground (GET)** and **batch queries (POST)**.

Includes **`limits`** helpers (`with_default_limits`, depth/complexity defaults) for production APIs.

**Docs:** [docs.rs/nestrs-graphql](https://docs.rs/nestrs-graphql) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-graphql = "0.1.2"
async-graphql = "7.0.17"
axum = "0.7"
```

Or:

```toml
nestrs = { version = "0.1.2", features = ["graphql"] }
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

## License

MIT OR Apache-2.0.
