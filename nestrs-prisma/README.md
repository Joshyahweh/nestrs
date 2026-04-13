# nestrs-prisma

Nest-style **`PrismaModule`** / **`PrismaService`** for [nestrs](https://crates.io/crates/nestrs): configuration, optional **SQLx** pooling (`DATABASE_URL`), and helpers that stay **ORM-agnostic** so you can choose how models are represented in Rust.

**Docs:** [docs.rs/nestrs-prisma](https://docs.rs/nestrs-prisma) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

```toml
[dependencies]
nestrs-prisma = { version = "0.2.0", features = ["sqlx"] }
nestrs = "0.2.0"
```

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

## 3. Optional: Prisma Client Rust (`cargo prisma generate`)

For generated model APIs, add [prisma-client-rust](https://github.com/Brendonovich/prisma-client-rust) to your app, run:

```bash
cargo prisma generate --schema prisma/schema.prisma
```

Register the generated client as an extra **`#[injectable]`** / module provider alongside `PrismaService`. This crate does not embed the codegen CLI; it focuses on connectivity and a **`PrismaService`** shape familiar to Nest users.

## Generate command hint

After `PrismaModule::for_root`, you can surface the documented generate line with:

```rust
let hint = PrismaModule::generate_command_hint();
```
