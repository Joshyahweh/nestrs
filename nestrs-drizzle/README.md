# nestrs-drizzle

Drizzle ORM adapter for the [nestrs](https://crates.io/crates/nestrs) framework — the Rust equivalent of [`@nestjs/typeorm`](https://docs.nestjs.com/techniques/database) when Drizzle is the chosen query builder.

```rust,ignore
use nestrs_drizzle::{DrizzleModule, DrizzleService, schema};
use drizzle_orm::postgres::Pg;

// define a table
schema::table! {
    users (id) {
        id -> Int4,
        email -> Text,
        name -> Text,
    }
}

#[tokio::main]
async fn main() {
    DrizzleModule::for_root("postgres://user:pass@localhost/app");
    let svc = DrizzleService::new();
    let db = svc.pool::<Pg>();
    // drizzle_orm::postgres::PgQueryBuilder usage:
    // let q = drizzle_orm::QueryBuilder::new()
    //     .select(columns)
    //     .from(users::table)
    //     .to_owned();
    // db.execute(q).await.unwrap();
}
```

## What you get

- `DrizzleModule::for_root(url)` — global connection-string setter.
- `DrizzleModule::for_root_with_options(opts)` — fine-grained driver tuning.
- `DrizzleOptions` builder (URL parsing, statement cache, slow-query log, max-pool-size).
- `DrizzleService` — injectable handle. `pool::<Backend>()` resolves a typed connection pool for the chosen SQL backend.
- `drizzle_orm` re-exported at the crate root so callers don't add it as a direct dep.

## Feature flags

- `default = []` — base crate, no SQL backend enabled.
- `postgres` — `drizzle-orm/postgres` (sqlx postgres driver).
- `mysql` — `drizzle-orm/mysql` (sqlx mysql driver).
- `sqlite` — `drizzle-orm/sqlite` (sqlx sqlite driver).
- `all` — convenience for all three backends.

## License

MIT OR Apache-2.0.