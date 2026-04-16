# nestrs-prisma

Nest-style **`PrismaModule`** / **`PrismaService`** for [nestrs](https://crates.io/crates/nestrs): configuration, optional **SQLx** pooling (`DATABASE_URL`), and helpers that stay **ORM-agnostic** so you can choose how models are represented in Rust.

**Docs:** [docs.rs/nestrs-prisma](https://docs.rs/nestrs-prisma) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

```toml
[dependencies]
async-trait = "0.1"
nestrs-prisma = { version = "0.3.6", features = ["sqlx", "sqlx-sqlite"] }
nestrs = "0.3.6"
```

When enabling `sqlx`, choose exactly one backend feature in your app: `sqlx-sqlite`, `sqlx-postgres`, or `sqlx-mysql`. If more than one is enabled (for example through workspace `--all-features`), the concrete driver is selected in priority order: Postgres, then MySQL, then SQLite.

The **`async-trait`** crate must be a direct dependency of any crate that invokes **`prisma_model!`**, because the generated repository trait uses `#[async_trait::async_trait]`.

## Generated type dependencies

Generated bindings can include native Rust types based on Prisma scalar/native DB type mapping.
Add the crates your schema requires:

```toml
[dependencies]
chrono = { version = "0.4", features = ["clock"] } # DateTime/Timestamp/Date/Time mappings
uuid = { version = "1", features = ["v4"] }        # @db.Uuid
serde_json = "1"                                    # Json/JsonB
rust_decimal = "1"                                  # Decimal/Numeric
ipnetwork = "0.21"                                  # @db.Cidr
bit-vec = "0.6"                                     # @db.Bit/@db.VarBit
```

If your schema does not use a given type family, you can omit that dependency.

## Run the quickstart example

There are two ways to run quickstart:

### A) From this repository (maintainers/contributors)

From the `nestrs` workspace root:

```bash
cargo run -p nestrs-prisma --example quickstart --features "sqlx,sqlx-sqlite"
```

### B) From your own app (crate consumers on `nestrs-prisma = "0.3.6"`)

`cargo run -p nestrs-prisma ...` will not work in your app, because `-p` targets a package in your current workspace.
Instead:

1. Add dependency:

```toml
nestrs-prisma = { version = "0.3.6", features = ["sqlx", "sqlx-postgres"] }
nestrs = "0.3.6"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

2. Fetch quickstart source into your app as `examples/quickstart.rs` (no manual copying):

```bash
mkdir -p examples
curl -fsSL "https://raw.githubusercontent.com/Joshyahweh/nestrs/v0.3.6/nestrs-prisma/examples/quickstart.rs" -o examples/quickstart.rs
```

Alternative (fetch from crates.io source):

```bash
cargo install cargo-download
cargo download nestrs-prisma==0.3.6 --extract
mkdir -p examples
cp nestrs-prisma-0.3.6/examples/quickstart.rs examples/quickstart.rs
```

3. Run from your app root:

```bash
cargo run --example quickstart
```

If `DATABASE_URL` is not set, the example defaults to `sqlite:quickstart.db`.
With `prisma/schema.prisma` present, it also generates bindings and writes `src/models/prisma_generated.rs`.

## 1. Write models in Prisma

Add a schema next to your app (convention: `prisma/schema.prisma`):

```prisma
datasource db {
  provider = "sqlite"
  url      = env("DATABASE_URL")
}

model User {
  id    Int    @id @default(autoincrement())
  email String @unique
  name  String
}
```

Use `prisma migrate`, `prisma db push`, or your own SQL migrations to apply it to the database.

## 2. Map rows in Rust (SQLx path, no codegen)

Enable **`sqlx`** on `nestrs-prisma`. Mirror columns with `sqlx::FromRow` and load via [`PrismaService::query_all_as`](https://docs.rs/nestrs-prisma):

```rust
#[derive(Debug, sqlx::FromRow, serde::Serialize)]
struct UserRow {
    id: i64,
    email: String,
    name: String,
}

let users: Vec<UserRow> = prisma
    .query_all_as(r#"SELECT "id", "email", "name" FROM "User""#)
    .await?;
```

Use [`PrismaService::execute`](https://docs.rs/nestrs-prisma) for DDL/DML without a row mapper (e.g. migration scripts).

## 3. Declarative `prisma_model!` (NestJS / Prisma-shaped repository)

With **`sqlx`** enabled, you can declare a table-bound model once. The macro expands to:

- A `#[derive(sqlx::FromRow)]` struct for the row shape.
- `UserWhere` (per model) with `And` / `Or` / `Not`, per-field **`equals`** / **`not`**, and helpers under a lowercase module (e.g. `user::id::equals(1)`).
- `UserUpdate` with `Option` fields and `user::email::set("…")`-style builders.
- `UserCreateInput`: if the first field is `id: i64`, it is omitted from create input (auto-increment inserts).
- `UserOrderBy` and `user::id::order(SortOrder::Desc)`.
- **`PrismaUserRepository`** (async trait) implemented for [`ModelRepository<User>`](https://docs.rs/nestrs-prisma) with:
  `find_unique`, `find_first`, `find_many`, `find_many_with_options`, `count`, `create`, `create_many`, `update`, `update_many`, `upsert`, `delete`, `delete_many`.
- **`PrismaUserClientExt`** on [`Arc<PrismaService>`](https://docs.rs/nestrs-prisma) so you can write `prisma.user().find_unique(…)`.

**Errors:** [`PrismaError`](https://docs.rs/nestrs-prisma) maps to [`nestrs::HttpException`](https://docs.rs/nestrs) via `impl From<PrismaError> for HttpException` (404 for missing rows, 409 for unique violations, 503-style for pool issues).

**Example:**

```rust
nestrs_prisma::prisma_model!(User => "users", {
    id: i64,
    email: String,
    name: String,
});

// `PrismaUserRepository` + `PrismaUserClientExt` live in this module after the macro expands.
async fn demo(prisma: std::sync::Arc<nestrs_prisma::PrismaService>) -> Result<(), nestrs_prisma::PrismaError> {
    let _ = prisma
        .user()
        .create(UserCreateInput {
            email: "a@b.c".into(),
            name: "Ada".into(),
        })
        .await?;
    Ok(())
}
```

**Supported field types today:** integer scalars (`i8`..`i64`, `u8`..`u64`), `String`, `bool`, `chrono` date/time types, `uuid::Uuid`, `serde_json::Value`, `Vec<u8>`, `rust_decimal::Decimal`, and provider-native mappings like `std::net::IpAddr` / `ipnetwork::IpNetwork` / `bit_vec::BitVec` when your schema uses corresponding native DB types.

**Note:** In-memory SQLite with SQLx `Any` works most reliably with a **single-connection** pool (`pool_max(1)`) so DDL and queries share one database. File-backed URLs avoid that limitation.

## 4. Optional: Prisma Client Rust (`cargo prisma generate`)

For generated model APIs, add [prisma-client-rust](https://github.com/Brendonovich/prisma-client-rust) to your app, run:

```bash
cargo prisma generate --schema prisma/schema.prisma
```

Register the generated client as an extra **`#[injectable]`** / module provider alongside `PrismaService`. This crate does not embed the codegen CLI; it focuses on connectivity and a **`PrismaService`** shape familiar to Nest users.

## 5. Generate command hint

After `PrismaModule::for_root`, you can surface the documented generate line with:

```rust
let hint = PrismaModule::generate_command_hint();
```
