# Backend stack recipes (procedures)

This chapter gives **many copy-paste-friendly examples** per stack: **`Cargo.toml`**, **Prisma / env**, **module graph**, **`main` bootstrap**, **services**, **controllers / GraphQL / micro handlers**, **CRUD variations**, **message brokers**, and **event-driven patterns**.

It complements [Microservices](microservices.md), [`MICROSERVICES.md`](../../MICROSERVICES.md), [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md), and [`nestrs-prisma` README](../../nestrs-prisma/README.md)—not replace them.

---

## Naming note: “Mongoose” vs Rust

In **NestJS**, **`@nestjs/mongoose`** wraps **Mongoose**. In **nestrs**, use **`MongoModule` / `MongoService`** ([`mongodb`](https://docs.rs/mongodb))—there is **no** npm Mongoose runtime. Optional: **Prisma CLI** with a Mongo datasource for **schema tooling**; Rust app code typically uses **BSON** + **`mongodb`** collections.

---

## Shared prerequisites (relational Prisma)

1. Toolchain: workspace [`rust-version`](../../Cargo.toml); see [First steps](first-steps.md).
2. **`nestrs-prisma`**: enable **exactly one** SQLx backend: `sqlx-postgres` **or** `sqlx-mysql` **or** `sqlx-sqlite`.
3. **`async-trait`** as a **direct** dependency if you use **`prisma_model!`**.
4. **`prisma/schema.prisma`** + **`DATABASE_URL`**; run **`prisma migrate`**, **`db push`**, or your SQL.
5. Extra crates for generated scalars: **`chrono`**, **`uuid`**, **`serde_json`**, **`rust_decimal`**, … — see [**`nestrs-prisma` README** § Generated type dependencies](../../nestrs-prisma/README.md).

---

# Recipe A — REST + PostgreSQL + Prisma

**Goal:** **`#[routes]`** HTTP API + **[`PrismaService`](https://docs.rs/nestrs-prisma/latest/nestrs_prisma/struct.PrismaService.html)** (SQLx).

### A.1 Minimal `Cargo.toml` (PostgreSQL)

```toml
[package]
name = "my-api"
version = "0.1.0"
edition = "2021"

[dependencies]
nestrs = "0.3.8"
nestrs-prisma = { version = "0.3.8", features = ["sqlx", "sqlx-postgres"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
validator = { version = "0.20", features = ["derive"] }
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "macros", "postgres"] }
```

### A.2 Same stack + OpenAPI + JSON

```toml
nestrs = { version = "0.3.8", features = ["openapi"] }
nestrs-openapi = "0.3.8"
utoipa = { version = "5", features = ["axum_extras"] }
```

Wire **[OpenAPI](openapi-http.md)** on [`NestApplication`](appendix-api-cookbook.md) after you have working REST routes.

### A.3 MySQL variant (feature swap only)

```toml
nestrs-prisma = { version = "0.3.8", features = ["sqlx", "sqlx-mysql"] }
```

`DATABASE_URL` example: `mysql://user:pass@localhost:3306/mydb`

### A.4 SQLite local / CI (matches [`examples/hello-app`](../../examples/hello-app))

```toml
nestrs-prisma = { version = "0.3.8", features = ["sqlx", "sqlx-sqlite"] }
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "macros", "sqlite"] }
```

Use `sqlite:./dev.db` or `sqlite::memory:` for tests; prefer **`pool_max(1)`** for in-memory SQLite when doing DDL + queries (see **`nestrs-prisma` README**).

### A.5 `prisma/schema.prisma` — PostgreSQL

```prisma
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

model User {
  id    Int    @id @default(autoincrement())
  email String @unique
  name  String
}
```

### A.6 `prisma/schema.prisma` — SQLite (demo)

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

### A.7 `.env.example` (relational)

```bash
# PostgreSQL
DATABASE_URL="postgresql://USER:PASSWORD@127.0.0.1:5432/myapp"

# Or SQLite (hello-app style)
# DATABASE_URL="sqlite:./dev.db"
```

### A.8 Bootstrap `PrismaModule` + optional DDL (from [`hello-app`](../../examples/hello-app/src/main.rs))

```rust
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite:{}", base.join("dev.db").display()));
    let schema_path = base.join("prisma/schema.prisma");

    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url(db_url)
            .pool_min(1)
            .pool_max(10)
            .schema_path(schema_path.to_string_lossy().as_ref()),
    );

    // Demo-only: create tables when not using prisma migrate yet
    let prisma = PrismaService::default();
    for ddl in [
        r#"CREATE TABLE IF NOT EXISTS "User" (
            "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            "email" TEXT NOT NULL,
            "name" TEXT NOT NULL
        )"#,
        r#"CREATE UNIQUE INDEX IF NOT EXISTS "User_email_key" ON "User"("email")"#,
    ] {
        if prisma.execute(ddl).await.is_err() {
            break;
        }
    }

    NestFactory::create::<AppModule>()
        .set_global_prefix("api")
        .listen_graceful(3000)
        .await;
}
```

For **Postgres**, prefer **`prisma migrate`** instead of raw `execute` DDL in production.

### A.9 Row type + service (`query_all_as`, `query_scalar`, `execute`)

[`PrismaService::execute`](https://docs.rs/nestrs-prisma) takes a **single SQL string** (no bind args)—use it for DDL one-offs, migrations, or fully inlined literals. For **parameterized** inserts, prefer **`prisma_model!`** repositories (**§ A.10**) or raw **`sqlx`** against your own pool.

```rust
use nestrs::prelude::*;
use nestrs_prisma::PrismaService;
use std::sync::Arc;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub name: String,
}

#[injectable]
pub struct UserService {
    prisma: Arc<PrismaService>,
}

impl UserService {
    pub async fn health(&self) -> &'static str {
        match self.prisma.query_scalar("SELECT 1").await {
            Ok(_) => "up",
            Err(_) => "degraded",
        }
    }

    pub async fn list_users(&self) -> Result<Vec<UserRow>, String> {
        self.prisma
            .query_all_as(r#"SELECT "id", "email", "name" FROM "User" ORDER BY "id""#)
            .await
    }

    /// Example: idempotent DDL for local/dev only (production → use `prisma migrate`).
    pub async fn ensure_demo_schema(&self) -> Result<(), String> {
        self.prisma
            .execute(
                r#"CREATE TABLE IF NOT EXISTS "User" (
                    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                    "email" TEXT NOT NULL,
                    "name" TEXT NOT NULL
                )"#,
            )
            .await
            .map(|_| ())
    }
}
```

### A.10 Declarative `prisma_model!` repository (relational table)

```rust
nestrs_prisma::prisma_model!(User => "User", {
    id: i64,
    email: String,
    name: String,
});

async fn create_demo(prisma: std::sync::Arc<nestrs_prisma::PrismaService>) -> Result<(), nestrs_prisma::PrismaError> {
    prisma
        .user()
        .create(UserCreateInput {
            email: "ada@example.com".into(),
            name: "Ada".into(),
        })
        .await?;
    Ok(())
}
```

Macro expands **`PrismaUserRepository`**, **`find_many`**, **`create`**, etc.—see **[`nestrs-prisma` README § Declarative](../../nestrs-prisma/README.md)**.

### A.11 HTTP controllers on top

```rust
#[controller(prefix = "/users", version = "v1")]
pub struct UserController;

#[routes(state = UserService)]
impl UserController {
    #[get("/")]
    pub async fn list(State(s): State<Arc<UserService>>) -> Result<Json<Vec<UserRow>>, HttpException> {
        s.list_users().await.map(Json).map_err(InternalServerErrorException::new)
    }

    #[get("/health/db")]
    pub async fn db_health(State(s): State<Arc<UserService>>) -> &'static str {
        s.health().await
    }
}
```

### A.12 Module graph

```rust
#[module(
    imports = [PrismaModule],
    re_exports = [PrismaModule],
)]
pub struct DataModule;

#[module(
    imports = [DataModule],
    controllers = [UserController],
    providers = [UserService],
)]
pub struct AppModule;
```

### A.13 Curl checks

```bash
curl -s "http://127.0.0.1:3000/api/v1/users/health/db"
curl -s "http://127.0.0.1:3000/api/v1/users/"
```

*(Prefix **`/api`** matches **`set_global_prefix`** + controller **`version = "v1"`** path layout—adjust to your app.)*

### A.14 Error mapping

[`PrismaError`](https://docs.rs/nestrs-prisma) implements **`Into<HttpException>`** (404 / 409 / pool errors). Use **`.map_err(|e| HttpException::from(e))`** or **`?`** where return type is **`Result<_, HttpException>`**.

### A.15 Official `nestrs-prisma` quickstart (User + Post, relationships)

The repository ships a full **end-to-end** example at:

**[`nestrs-prisma/examples/quickstart.rs`](../../nestrs-prisma/examples/quickstart.rs)** (also on GitHub: `nestrs-prisma/examples/quickstart.rs` on tag **`v0.3.8`**).

It demonstrates:

- Two **`prisma_model!`** tables, **`users`** and **`posts`**, with a **foreign key** `author_id` → `users.id`.
- [`PrismaModule::for_root_with_options`](https://docs.rs/nestrs-prisma) + [`PrismaService`](https://docs.rs/nestrs-prisma).
- **Schema → Rust sync** with **relation** metadata via [`PrismaService::sync_from_prisma_schema`](https://docs.rs/nestrs-prisma) and [`SchemaSyncOptions`](https://docs.rs/nestrs-prisma) (writes a generated file, reports **`relation_count`**).
- **CRUD** with `prisma.user().create`, `prisma.post().create`, `find_unique`, `find_many` with a `PostWhere` on **`author_id`**.

**Run it from the nestrs workspace root:**

```bash
cargo run -p nestrs-prisma --example quickstart --features "sqlx,sqlx-sqlite"
```

Unset **`DATABASE_URL`** defaults to **`sqlite:quickstart.db`**. Non-SQLite URLs skip the bundled DDL + demo CRUD branch but still print commands and may run **`sync_from_prisma_schema`** when **`prisma/schema.prisma`** exists.

**Minimal Prisma schema (relational) aligned with the example:**

```prisma
datasource db {
  provider = "sqlite"
  url      = env("DATABASE_URL")
}

model User {
  id    Int    @id @default(autoincrement())
  email String @unique
  name  String
  posts Post[]
}

model Post {
  id       Int  @id @default(autoincrement())
  title    String
  authorId Int  @map("author_id")
  author   User @relation(fields: [authorId], references: [id], onDelete: Cascade)
}
```

Table names in SQL / `prisma_model!` map to **`users`** / **`posts`** (see the example’s macro and DDL).

### A.16 Commands: Prisma CLI + Rust client codegen + relation-aware Rust sync

Use these in order for a typical workflow.

**1. Install Node Prisma CLI** (migrate / db push):

```bash
npm install prisma @prisma/client --save-dev
# or: pnpm / yarn
```

**2. Apply schema to the database** (pick one):

```bash
npx prisma migrate dev
# or for quick local/prototype:
npx prisma db push
```

The crate also exposes command builders: **`prisma_migrate_deploy_command`** → **`npx prisma migrate deploy`**, **`prisma_db_push_command`** → **`npx prisma db push`** (Mongo-style deploys)—see [**docs.rs**](https://docs.rs/nestrs-prisma).

**3. Generate the Prisma Client Rust API** (optional separate stack per [`nestrs-prisma` README § 4](../../nestrs-prisma/README.md)):

[`prisma_generate_command`](https://docs.rs/nestrs-prisma) expands to:

```bash
cargo prisma generate --schema prisma/schema.prisma
```

You need the **[prisma-client-rust](https://github.com/Brendonovich/prisma-client-rust)** toolchain installed (`cargo install prisma-client-rust-cli` or project setup). Register that generated client as its own provider if you use it **alongside** [`PrismaService`](https://docs.rs/nestrs-prisma).

**4. Generate nestrs-prisma Rust bindings from the same `schema.prisma` (includes relation graph + FK plan)**

This is **`nestrs-prisma`’s** bridge—not the Node client. After [`PrismaModule::for_root_with_options`](https://docs.rs/nestrs-prisma), call **`sync_from_prisma_schema`** (same pattern as **quickstart**):

```rust
use nestrs_prisma::schema_bridge::SchemaSyncOptions;
use nestrs_prisma::PrismaService;

let prisma = PrismaService::default();

let report = prisma
    .sync_from_prisma_schema(
        "prisma/schema.prisma",
        SchemaSyncOptions {
            output_file: "src/models/prisma_generated.rs".to_string(),
            max_identifier_len: 63,
            apply_foreign_keys: false, // set true to execute generated FK DDL via SQLx
        },
    )
    .await?;

println!(
    "models={}, relations={}, written={}",
    report.model_count, report.relation_count, report.written_file
);
```

- **`relation_count`** reflects parsed **`@relation`** edges and inferred relation metadata (see **[`SchemaSyncReport`](https://docs.rs/nestrs-prisma)**).
- With **`apply_foreign_keys: true`**, the service may execute generated **`foreign_key_sql`** against your pool—use only when you intend to mutate live DDL.

At runtime you can still **`println!`** the documented generate line without executing it:

```rust
println!("{}", nestrs_prisma::prisma_generate_command("prisma/schema.prisma"));
```

### A.17 `prisma_model!` for related tables (matches quickstart)

Declare **each** table and include the **FK scalar** on the child model:

```rust
nestrs_prisma::prisma_model!(User => "users", {
    id: i64,
    email: String,
    name: String,
});

nestrs_prisma::prisma_model!(Post => "posts", {
    id: i64,
    title: String,
    author_id: i64,
});
```

Then use nested flows such as **`prisma.post().find_many(PostWhere::and(vec![post::author_id::equals(user.id)]))`** after **`create`**—see **quickstart.rs** lines showing **`create`** → **`find_many`** on posts.

### A.18 Reference project layout (REST + Prisma)

```text
my-api/
├── Cargo.toml
├── .env                      # DATABASE_URL (never commit secrets)
├── prisma/
│   └── schema.prisma
└── src/
    ├── main.rs               # PrismaModule::for_root*, NestFactory::create, listen*
    └── app/
        ├── mod.rs            # optional: re-export modules
        ├── data_module.rs    # imports PrismaModule
        ├── users.rs          # controller + service + DTOs
        └── posts.rs          # second controller using same PrismaService
```

Keep **`PrismaModule::for_root_with_options`** in **`main`** (or a tiny `bootstrap.rs`) **before** **`NestFactory::create`** so the pool exists before DI builds providers.

### A.19 More REST patterns: POST create, path param, `find_unique`, `count`, `update`

**POST JSON body + validation** (same shapes as [First steps](first-steps.md)):

```rust
#[dto]
pub struct CreateUserBody {
    #[IsEmail]
    pub email: String,
    #[Length(min = 1, max = 120)]
    pub name: String,
}

#[routes(state = UserService)]
impl UserController {
    #[post("/")]
    pub async fn create(
        State(s): State<Arc<UserService>>,
        ValidatedBody(body): ValidatedBody<CreateUserBody>,
    ) -> Result<Json<UserRow>, HttpException> {
        s.create_via_macro(&body.email, &body.name).await.map(Json)
    }

    #[get("/:id")]
    pub async fn get_one(
        State(s): State<Arc<UserService>>,
        nestrs::axum::extract::Path(id): nestrs::axum::extract::Path<i64>,
    ) -> Result<Json<UserRow>, HttpException> {
        s.find_by_id(id).await.map(Json)
    }
}
```

**Service methods using `prisma_model!` client** (aligned with **[`nestrs-prisma/tests/prisma_model_client.rs`](../../nestrs-prisma/tests/prisma_model_client.rs)** — adjust **`User`** table name / types to your **`prisma_model!`**):

```rust
impl UserService {
    pub async fn create_via_macro(&self, email: &str, name: &str) -> Result<UserRow, HttpException> {
        let row = self
            .prisma
            .user()
            .create(UserCreateInput {
                email: email.into(),
                name: name.into(),
            })
            .await
            .map_err(HttpException::from)?;
        Ok(UserRow {
            id: row.id,
            email: row.email,
            name: row.name,
        })
    }

    pub async fn find_by_id(&self, id: i64) -> Result<UserRow, HttpException> {
        let u = self
            .prisma
            .user()
            .find_unique(user::id::equals(id))
            .await
            .map_err(HttpException::from)?
            .ok_or_else(|| NotFoundException::new("user"))?;
        Ok(UserRow {
            id: u.id,
            email: u.email,
            name: u.name,
        })
    }

    pub async fn count_users(&self) -> Result<i64, HttpException> {
        self.prisma
            .user()
            .count(UserWhere::and(vec![]))
            .await
            .map_err(HttpException::from)
    }

    pub async fn rename(&self, id: i64, name: &str) -> Result<(), HttpException> {
        self.prisma
            .user()
            .update(user::id::equals(id), user::name::set(name.into()))
            .await
            .map_err(HttpException::from)?;
        Ok(())
    }
}
```

More APIs you can lift from tests: **`create_many`**, **`find_many`**, **`update_many`**, **`aggregate`**, **`group_by`**—same file as above.

### A.20 Run the repo’s HTTP + Prisma demo (`hello-app`)

From the **nestrs** workspace root:

```bash
cd examples/hello-app
export DATABASE_URL="sqlite:${PWD}/dev.db"
cargo run
```

Then try:

```bash
curl -s "http://127.0.0.1:3000/platform/v1/api/db-health"
curl -s "http://127.0.0.1:3000/platform/v1/api/users-db"
```

Paths use **`set_global_prefix("platform")`** + controller **`version = "v1"`** + **`prefix = "/api"`**—see **[`examples/hello-app/src/main.rs`](../../examples/hello-app/src/main.rs)**.

### A.21 PostgreSQL via Docker (quick env)

```bash
docker run --name nestrs-pg -e POSTGRES_PASSWORD=dev -e POSTGRES_DB=myapp -p 5432:5432 -d postgres:16
export DATABASE_URL="postgresql://postgres:dev@127.0.0.1:5432/myapp"
npx prisma migrate dev   # after placing prisma/schema.prisma with provider postgresql
```

Match **`nestrs-prisma`** features to **`sqlx-postgres`** (**§ A.1**).

### A.22 In-memory SQLite for tests (`pool_max(1)`)

```rust
let _ = PrismaModule::for_root_with_options(
    PrismaOptions::from_url("sqlite::memory:?cache=shared")
        .pool_min(1)
        .pool_max(1)
        .schema_path("prisma/schema.prisma"),
);
```

Then call **`execute(ddl)`** once per test or use **`tokio::test`** with **`serial_test`** if sharing registries—see [integration tests](../../nestrs/tests/) patterns.

### A.23 `NestFactory::into_router()` smoke test

```rust
#[tokio::test]
async fn router_returns_200() {
    let _ = PrismaModule::for_root_with_options(/* … */);
    let router = NestFactory::create::<AppModule>().into_router();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/health/db")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success());
}
```

Use **`tower::ServiceExt::oneshot`** — same idea as **`param_decorators_and_pipes`** tests in **`nestrs/tests/`**.

### A.24 REST + Prisma troubleshooting

| Symptom | Things to check |
|---------|------------------|
| `sqlx` pool / TLS errors | **`DATABASE_URL`** scheme matches **`sqlx-*`** feature; Postgres often needs `sslmode` in URL. |
| “feature not enabled” | Exactly **one** of **`sqlx-postgres` / `sqlx-mysql` / `sqlx-sqlite`** on **`nestrs-prisma`**. |
| In-memory SQLite flaky | **`pool_max(1)`** and single-threaded DDL (**`nestrs-prisma` README**). |
| Macro / trait errors on **`prisma_model!`** | Add **`async-trait`** to **`Cargo.toml`** directly. |

### A.25 REST CRUD cheat-sheet (`prisma_model!` **User**)

Wire these routes once **`User`** / **`users`** matches **§ A.17** style macros and DDL.

| Operation | HTTP | Handler calls (service) |
|-----------|------|-------------------------|
| **Create** | `POST /users/` | **`user().create(UserCreateInput { … })`** |
| **Read list** | `GET /users/` | **`find_many`** with **`UserWhere`** + optional **`take`/`skip`** via **`find_many_with_options`** |
| **Read one** | `GET /users/:id` | **`find_unique(user::id::equals(id))`** |
| **Update** | `PATCH /users/:id` | **`update(user::id::equals(id), …)`** or **`update_many`** |
| **Delete** | `DELETE /users/:id` | **`delete`** / **`delete_many`** if exposed for your macro version |

Prefer **`ValidatedBody`** + **`ValidatedPath`** ([First steps](first-steps.md)) for payloads; map **`PrismaError`** → **`HttpException`** (**§ A.14**).

### A.26 Pagination & counts (list endpoints)

Sketch for **offset paging** (names follow your **`prisma_model!`** expansion—see **[`prisma_model_client.rs`](../../nestrs-prisma/tests/prisma_model_client.rs)**):

```rust
use nestrs_prisma::SortOrder;

pub async fn page(&self, skip: i64, take: i64) -> Result<Vec<UserRow>, HttpException> {
    let rows = self
        .prisma
        .user()
        .find_many_with_options(UserFindManyOptions {
            r#where: UserWhere::and(vec![]),
            order_by: Some(vec![user::id::order(SortOrder::Asc)]),
            take: Some(take.clamp(1, 100)),
            skip: Some(skip.max(0)),
            distinct: None,
        })
        .await
        .map_err(HttpException::from)?;
    Ok(rows.into_iter().map(/* model → UserRow */).collect())
}
```

Expose **`GET /users?skip=&take=`** via **`ValidatedQuery`** and return **`Json<Vec<UserRow>>`** plus a **`X-Total-Count`** header from **`count`** if you need exact totals.

---

# Recipe B — REST + MongoDB (`MongoModule`)

**Goal:** REST + official **`mongodb`** driver (Nest **`MongooseModule`**-style **bootstrap only**).

### B.1 `Cargo.toml`

```toml
[dependencies]
nestrs = { version = "0.3.8", features = ["mongo"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
mongodb = "3"
bson = "3"
futures-util = { version = "0.3", features = ["sink"] }
serde = { version = "1", features = ["derive"] }
validator = { version = "0.20", features = ["derive"] }
```

### B.2 Bootstrap URI

```rust
let _ = nestrs::MongoModule::for_root(
    std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".into()),
);
```

### B.3 Service: database + collection

```rust
use bson::doc;
use futures_util::TryStreamExt;
use mongodb::options::FindOptions;
use nestrs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileDoc {
    pub email: String,
    pub display_name: String,
}

#[injectable]
pub struct ProfileStore {
    mongo: nestrs::MongoService,
}

impl ProfileStore {
    pub async fn count_profiles(&self) -> Result<u64, String> {
        let db = self.mongo.database("app").await?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.estimated_document_count()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<ProfileDoc>, String> {
        let db = self.mongo.database("app").await?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.find_one(doc! { "email": email })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn list_recent(&self, limit: i64) -> Result<Vec<ProfileDoc>, String> {
        let db = self.mongo.database("app").await?;
        let col = db.collection::<ProfileDoc>("profiles");
        let opts = FindOptions::builder().limit(limit).build();
        let cursor = col.find(doc! {}, opts).await.map_err(|e| e.to_string())?;
        cursor.try_collect().await.map_err(|e| e.to_string())
    }
}
```

### B.4 Controller excerpt

```rust
#[controller(prefix = "/profiles", version = "v1")]
pub struct ProfileController;

#[routes(state = ProfileStore)]
impl ProfileController {
    #[get("/count")]
    pub async fn count(State(s): State<std::sync::Arc<ProfileStore>>) -> Result<String, HttpException> {
        let n = s.count_profiles().await.map_err(InternalServerErrorException::new)?;
        Ok(n.to_string())
    }

    #[get("/by-email")]
    pub async fn by_email(
        State(s): State<std::sync::Arc<ProfileStore>>,
        axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let email = q.get("email").cloned().unwrap_or_default();
        let doc = s
            .find_by_email(&email)
            .await
            .map_err(InternalServerErrorException::new)?;
        Ok(Json(serde_json::to_value(doc).unwrap_or(serde_json::json!(null))))
    }
}
```

### B.5 Module wiring

```rust
#[module(imports = [MongoModule], controllers = [ProfileController], providers = [ProfileStore])]
pub struct AppModule;
```

### B.6 Indexes & ops

Create indexes with **`mongosh`**, Compass, or driver **`create_index`** in a one-off admin task—nestrs does not ship migrations for Mongo.

### B.7 Reference layout (REST + Mongo)

```text
mongo-api/
├── Cargo.toml
├── .env                      # MONGODB_URI (never commit secrets)
└── src/
    ├── main.rs               # MongoModule::for_root, NestFactory::create, listen*
    └── app/
        ├── mod.rs
        ├── profiles.rs       # controller + ProfileStore
        └── health.rs         # optional: GET /health/mongo
```

### B.8 MongoDB via Docker + `MONGODB_URI`

```bash
docker run --name nestrs-mongo -p 27017:27017 -d mongo:7
export MONGODB_URI="mongodb://127.0.0.1:27017"
```

Use **`MongoModule::for_root`** with that URI (or **`mongodb://user:pass@host:27017/db?authSource=admin`** for Atlas-style URLs).

### B.9 Writes: `insert_one`, `replace_one`, `update_one`, `delete_one`

```rust
use bson::{doc, oid::ObjectId};
use mongodb::options::{FindOneAndUpdateOptions, ReturnDocument};

impl ProfileStore {
    pub async fn upsert_by_email(&self, email: &str, display_name: &str) -> Result<(), String> {
        let db = self.mongo.database("app").await?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.replace_one(
            doc! { "email": email },
            ProfileDoc {
                email: email.into(),
                display_name: display_name.into(),
            },
            mongodb::options::ReplaceOptions::builder()
                .upsert(true)
                .build(),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn rename_first_match(&self, email: &str, new_name: &str) -> Result<Option<ProfileDoc>, String> {
        let db = self.mongo.database("app").await?;
        let col = db.collection::<ProfileDoc>("profiles");
        let opts = FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();
        col.find_one_and_update(
            doc! { "email": email },
            doc! { "$set": { "display_name": new_name } },
            opts,
        )
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn delete_by_id(&self, hex_id: &str) -> Result<u64, String> {
        let oid = ObjectId::parse_str(hex_id).map_err(|e| e.to_string())?;
        let db = self.mongo.database("app").await?;
        let col = db.collection::<mongodb::bson::Document>("profiles");
        let r = col.delete_one(doc! { "_id": oid }).await.map_err(|e| e.to_string())?;
        Ok(r.deleted_count)
    }
}
```

Typed **`ProfileDoc`** reads/writes **Serde** fields; **`_id`** often lives only in BSON unless you add **`#[serde(rename = "_id")] pub id: Option<ObjectId>`** to your struct.

### B.10 Unique index from Rust (startup hook)

```rust
use mongodb::{options::IndexOptions, IndexModel};

async fn ensure_email_index(store: &ProfileStore) -> Result<(), String> {
    let db = store.mongo.database("app").await?;
    let col = db.collection::<ProfileDoc>("profiles");
    let model = IndexModel::builder()
        .keys(doc! { "email": 1 })
        .options(IndexOptions::builder().unique(true).build())
        .build();
    col.create_index(model).await.map_err(|e| e.to_string())?;
    Ok(())
}
```

Call once from **`main`** after **`MongoModule::for_root`** or inside a lazily-initialized provider.

### B.11 Minimal `main.rs`

```rust
#[tokio::main]
async fn main() {
    let _ = nestrs::MongoModule::for_root(
        std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".into()),
    );

    NestFactory::create::<AppModule>()
        .listen(3000)
        .await
        .expect("server");
}
```

### B.12 Extra REST routes (POST upsert + `ObjectId` path)

```rust
#[derive(serde::Deserialize)]
pub struct UpsertBody {
    pub email: String,
    pub display_name: String,
}

#[routes(state = ProfileStore)]
impl ProfileController {
    #[post("/")]
    pub async fn upsert(
        State(s): State<std::sync::Arc<ProfileStore>>,
        nestrs::axum::extract::Json(body): nestrs::axum::extract::Json<UpsertBody>,
    ) -> Result<nestrs::axum::http::StatusCode, HttpException> {
        s.upsert_by_email(&body.email, &body.display_name)
            .await
            .map_err(InternalServerErrorException::new)?;
        Ok(nestrs::axum::http::StatusCode::NO_CONTENT)
    }

    #[delete("/:id")]
    pub async fn delete(
        State(s): State<std::sync::Arc<ProfileStore>>,
        nestrs::axum::extract::Path(id): nestrs::axum::extract::Path<String>,
    ) -> Result<String, HttpException> {
        let n = s.delete_by_id(&id).await.map_err(InternalServerErrorException::new)?;
        Ok(format!("deleted:{n}"))
    }
}
```

Wire handlers you actually implement (**`upsert_by_email`**, **`delete_by_id`**) from **§ B.9**.

### B.13 `MongoService::ping` in a health route

```rust
#[get("/health/mongo")]
pub async fn mongo_health(State(m): State<std::sync::Arc<nestrs::MongoService>>) -> &'static str {
    if m.ping().await.is_ok() {
        "ok"
    } else {
        "down" // tighten: map to 503 via Result
    }
}
```

### B.14 Try with `curl`

```bash
curl -s "http://127.0.0.1:3000/v1/profiles/count"
curl -s "http://127.0.0.1:3000/v1/profiles/by-email?email=a@example.com"
curl -s -X POST "http://127.0.0.1:3000/v1/profiles/" \
  -H 'content-type: application/json' \
  -d '{"email":"a@example.com","display_name":"Ada"}'
```

Paths depend on **`#[controller(prefix = …, version = …)]`** + any **`set_global_prefix`** on **`NestFactory`** (**Recipe A.20** pattern).

### B.15 Mongo troubleshooting

| Symptom | Things to check |
|---------|------------------|
| “MongoModule must be called…” | **`MongoModule::for_root`** runs **before** **`NestFactory::create`** (same process). |
| Connection refused | **`MONGODB_URI`** host/port; Docker **`-p 27017:27017`**. |
| Duplicate key on email | **`replace_one`** + unique index (**§ B.10**)—handle write errors in API. |
| `_id` missing on read | **`find_one`** returns **`ProfileDoc`** without **`_id`** unless you model it—use **`Document`** or add **`ObjectId`** field to struct. |

### B.16 REST CRUD routes (`ProfileStore`)

Typical surface on **`ProfileController`** (prefix **`/profiles`**, version **`v1`**):

| Method | Path | Behavior |
|--------|------|----------|
| `GET` | `/` | **`list_recent(limit)`** — add **`skip`** query for paging |
| `GET` | `/:id` | Resolve **`ObjectId`**, **`find_one`** by **`_id`** (collection as **`Document`** or typed struct with **`_id`**) |
| `POST` | `/` | **`insert_one(ProfileDoc)`** |
| `PUT` | `/` | **`upsert_by_email`** (**§ B.9**) — treat as replace-by-email |
| `PATCH` | `/:id` | **`find_one_and_update`** partial **`$set`** (**§ B.9**) |
| `DELETE` | `/:id` | **`delete_by_id`** (**§ B.9**) |

Return **`201`** + **`Location`** on create when you expose new **`ObjectId`**s.

### B.17 Typed document with **`_id`** for CRUD reads

```rust
use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileDoc {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub email: String,
    pub display_name: String,
}
```

Then **`GET /:id`** can **`find_one`** into **`ProfileDoc`** and serialize **`id`** as hex string in JSON (`serde_json` supports **`ObjectId`** with **`hex`** feature on **`bson`** crate as configured).

---

# Recipe C — GraphQL + PostgreSQL + Prisma

**Goal:** **`async-graphql`** on **`/graphql`** + **`PrismaService`** in resolvers.

### C.1 `Cargo.toml`

```toml
nestrs = { version = "0.3.8", features = ["graphql"] }
nestrs-prisma = { version = "0.3.8", features = ["sqlx", "sqlx-postgres"] }
async-graphql = "=7.0.17"
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
```

### C.2 Query root holding Prisma

```rust
use async_graphql::{Object, Schema, EmptyMutation, EmptySubscription};
use nestrs_graphql::with_default_limits;
use nestrs_prisma::PrismaService;
use std::sync::Arc;

pub struct QueryRoot {
    prisma: Arc<PrismaService>,
}

#[Object]
impl QueryRoot {
    async fn db_ping(&self) -> String {
        self.prisma
            .query_scalar("SELECT 1")
            .await
            .unwrap_or_else(|e| format!("err:{e}"))
    }

    async fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
```

### C.3 Build schema with limits

```rust
let query = QueryRoot { prisma: prisma.clone() };
let schema = with_default_limits(Schema::build(query, EmptyMutation, EmptySubscription)).finish();
```

### C.4 Attach to HTTP app

```rust
NestFactory::create::<AppModule>()
    .enable_graphql(schema)
    .listen(3000)
    .await;
```

### C.5 Custom path + disable Playground (staging)

```rust
use nestrs_graphql::GraphQlHttpOptions;

NestFactory::create::<AppModule>()
    .enable_graphql_with_options(
        schema,
        "/query",
        GraphQlHttpOptions {
            enable_playground: false,
            ..Default::default()
        },
    )
    .listen(3000)
    .await;
```

*(Exact **`GraphQlHttpOptions`** fields: follow **[`nestrs-graphql`](https://docs.rs/nestrs-graphql)** current API.)*

### C.6 REST + GraphQL together

Keep **`#[routes]`** controllers **and** **`enable_graphql`**—one **`NestFactory::create::<AppModule>()`**, two surfaces.

### C.7 Curl / browser

```bash
curl -s -X POST http://127.0.0.1:3000/graphql \
  -H 'content-type: application/json' \
  -d '{"query":"{ dbPing version }"}'
```

### C.8 `SimpleObject` + mutation root (create/read)

```rust
use async_graphql::{Object, Schema, SimpleObject};
use nestrs_graphql::with_default_limits;

#[derive(SimpleObject, Clone)]
pub struct GqlUser {
    pub id: String,
    pub email: String,
    pub name: String,
}

pub struct MutationRoot {
    prisma: std::sync::Arc<nestrs_prisma::PrismaService>,
}

#[Object]
impl MutationRoot {
    async fn register_demo_user(&self, email: String) -> Result<GqlUser, async_graphql::Error> {
        let _ = self
            .prisma
            .query_scalar("SELECT 1")
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(GqlUser {
            id: "demo".into(),
            email,
            name: "demo".into(),
        })
    }
}

// Build:
// let schema = with_default_limits(
//     Schema::build(QueryRoot { prisma }, MutationRoot { prisma }, EmptySubscription),
// )
// .finish();
```

Swap **`register_demo_user`** body for **`prisma_model!`** **`create`** / **`find_unique`** (**Recipe A.19**) once your **`User`** model matches the DB.

### C.9 Resolver returning `Result` / errors

Prefer **`async_graphql::Error::new`** / **`new_with_source`** so clients get structured GraphQL errors; **`HttpException`** maps through whichever bridge you choose—many apps use **`Result<T, async_graphql::Error>`** at the resolver boundary:

```rust
#[Object]
impl QueryRoot {
    async fn strict_ping(&self) -> Result<String, async_graphql::Error> {
        self.prisma
            .query_scalar("SELECT 1")
            .await
            .map(|_| "ok".into())
            .map_err(async_graphql::Error::new_with_source)
    }
}
```

### C.10 Paths with `set_global_prefix`

[`enable_graphql`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_graphql) merges **`GET/POST /graphql`** onto the **same Axum router** as REST—**global prefix applies**:

```rust
NestFactory::create::<AppModule>()
    .set_global_prefix("platform")
    .enable_graphql(schema)
    .listen(3000)
    .await;
```

Then POST to **`http://127.0.0.1:3000/platform/graphql`** (not bare **`/graphql`**) unless you mount at **`/`**.

### C.11 Batch GraphQL POST

```bash
curl -s -X POST http://127.0.0.1:3000/graphql \
  -H 'content-type: application/json' \
  -d '[{"query":"{ version }"},{"query":"{ dbPing }"}]'
```

Handler supports batch arrays when enabled in **`nestrs-graphql`** router (**same path** as single POST).

### C.12 Export SDL (contracts / federation hint)

```rust
#[cfg(test)]
fn snapshot_schema(schema: &async_graphql::Schema<QueryRoot, async_graphql::EmptyMutation, async_graphql::EmptySubscription>) {
    let sdl = nestrs_graphql::export_schema_sdl(schema);
    assert!(sdl.contains("type Query"));
}
```

See **`nestrs-graphql` README** for **`export_schema_sdl_with_options`** / federation-shaped SDL.

### C.13 REST + GraphQL + OpenAPI in one app

You can chain **`.enable_graphql(schema)`** with **`.enable_openapi()`** (features **`graphql` + `openapi`**) on one **`NestFactory::create`**—controllers stay on REST paths; GraphQL stays on **`/graphql`** (plus prefix).

### C.14 GraphQL + Prisma troubleshooting

| Symptom | Things to check |
|---------|------------------|
| 404 on `/graphql` | **`set_global_prefix`** / **`enable_graphql_with_path`**—confirm full URL. |
| Playground 405 / disabled | **`GraphQlHttpOptions::enable_playground`** (**Recipe C.5**). |
| Deep resolver stacks | Re-use **one** shared **`Arc<PrismaService>`**; avoid per-request pool creation. |
| N+1 on relations | Add **`DataLoader`** / batching in **`async-graphql`** (outside nestrs core). |

### C.15 GraphQL CRUD with **`prisma_model!`** (sketch)

Assume **`nestrs_prisma::prisma_model!(User => "users", { id: i64, email: String, name: String })`** and a shared **`Arc<PrismaService>`** on **`QueryRoot`** / **`MutationRoot`**.

**Query — list + one:**

```rust
#[Object]
impl QueryRoot {
    async fn users(&self, take: Option<i64>, skip: Option<i64>) -> Result<Vec<GqlUser>, async_graphql::Error> {
        let rows = self
            .prisma
            .user()
            .find_many_with_options(UserFindManyOptions {
                r#where: UserWhere::and(vec![]),
                order_by: Some(vec![user::id::order(nestrs_prisma::SortOrder::Asc)]),
                take: Some(take.unwrap_or(20).clamp(1, 100)),
                skip: Some(skip.unwrap_or(0).max(0)),
                distinct: None,
            })
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(rows
            .into_iter()
            .map(|u| GqlUser {
                id: u.id.to_string(),
                email: u.email,
                name: u.name,
            })
            .collect())
    }

    async fn user(&self, id: i64) -> Result<Option<GqlUser>, async_graphql::Error> {
        let u = self
            .prisma
            .user()
            .find_unique(user::id::equals(id))
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(u.map(|u| GqlUser {
            id: u.id.to_string(),
            email: u.email,
            name: u.name,
        }))
    }
}
```

**Mutation — create / update / delete:**

```rust
#[Object]
impl MutationRoot {
    async fn create_user(&self, email: String, name: String) -> Result<GqlUser, async_graphql::Error> {
        let u = self
            .prisma
            .user()
            .create(UserCreateInput { email, name })
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(GqlUser {
            id: u.id.to_string(),
            email: u.email,
            name: u.name,
        })
    }

    async fn delete_user(&self, id: i64) -> Result<bool, async_graphql::Error> {
        let n = self
            .prisma
            .user()
            .delete_many(UserWhere::and(vec![user::id::equals(id)]))
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(n > 0)
    }
}
```

Adjust **`delete`** vs **`delete_many`** to the APIs your macro expands (**[`prisma_model_client.rs`](../../nestrs-prisma/tests/prisma_model_client.rs)** lists **`update`**, **`delete_many`**, etc.).

### C.16 Batching writes (`create_many`)

Reuse **`create_many`** / **`create_many_with_options`** from tests when importing CSV or sync jobs—same **`PrismaService`** pool as HTTP.

### C.17 Splitting GraphQL layers

Keep **thin resolvers**; put transaction boundaries in a **`UserRepository`** `#[injectable]` if CRUD grows—inject **`Arc<PrismaService>`** once and share across Query/Mutation roots.

---

# Recipe D — GraphQL + MongoDB

### D.1 `Cargo.toml`

```toml
nestrs = { version = "0.3.8", features = ["graphql", "mongo"] }
mongodb = "3"
bson = "3"
async-graphql = "=7.0.17"
futures-util = { version = "0.3", features = ["sink"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### D.2 Query root with `MongoService`

```rust
pub struct QueryRoot {
    mongo: std::sync::Arc<nestrs::MongoService>,
}

#[Object]
impl QueryRoot {
    async fn mongo_ping(&self) -> bool {
        self.mongo.ping().await.is_ok()
    }
}
```

### D.3 Build + listen

```rust
let schema = Schema::build(
    QueryRoot { mongo: mongo.clone() },
    async_graphql::EmptyMutation,
    async_graphql::EmptySubscription,
)
.finish();

NestFactory::create::<AppModule>()
    .enable_graphql(schema)
    .listen(3000)
    .await;
```

**Prisma CLI + Mongo:** okay for **`db push`** workflow; **Rust query path** for documents is still **`mongodb`** / **BSON** unless you maintain a separate layer.

### D.4 Typed rows on the wire (`SimpleObject`)

Reuse **`ProfileDoc`** from **Recipe B** and add a GraphQL-facing type:

```rust
use async_graphql::SimpleObject;

#[derive(SimpleObject, Clone)]
pub struct GqlProfile {
    pub email: String,
    pub display_name: String,
}
```

Map **`ProfileDoc` → `GqlProfile`** in resolvers so you control which BSON fields cross the API.

### D.5 `QueryRoot`: list + collection count

```rust
use async_graphql::Object;
use bson::doc;
use futures_util::TryStreamExt;
use mongodb::options::FindOptions;

#[Object]
impl QueryRoot {
    async fn profile_count(&self) -> Result<u64, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.estimated_document_count()
            .await
            .map_err(async_graphql::Error::new)
    }

    async fn profiles(&self, take: Option<i64>) -> Result<Vec<GqlProfile>, async_graphql::Error> {
        let lim = take.unwrap_or(20).clamp(1, 100);
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        let opts = FindOptions::builder().limit(Some(lim)).build();
        let cursor = col.find(doc! {}).await.map_err(async_graphql::Error::new)?;
        let rows: Vec<ProfileDoc> = cursor.try_collect().await.map_err(async_graphql::Error::new)?;
        Ok(rows
            .into_iter()
            .map(|p| GqlProfile {
                email: p.email,
                display_name: p.display_name,
            })
            .collect())
    }
}
```

Keep **`mongo_ping`** from **§ D.2** on the same **`impl`**.

### D.6 `MutationRoot`: insert a document

```rust
use async_graphql::Object;

pub struct MutationRoot {
    mongo: std::sync::Arc<nestrs::MongoService>,
}

#[Object]
impl MutationRoot {
    async fn seed_profile(
        &self,
        email: String,
        display_name: String,
    ) -> Result<bool, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.insert_one(ProfileDoc { email, display_name })
            .await
            .map_err(async_graphql::Error::new)?;
        Ok(true)
    }
}
```

Build: **`Schema::build(QueryRoot { mongo: m.clone() }, MutationRoot { mongo: m }, EmptySubscription).finish()`**, then **`with_default_limits`** (**Recipe C**).

### D.7 `MongoModule` + module graph (same process as REST)

```rust
#[module(imports = [MongoModule], /* controllers, providers */ )]
pub struct AppModule;
```

For a **complete** bootstrap (no hand-waving about where **`mongo`** comes from), use **`§ D.11`**: call **`MongoModule::for_root`**, then **`Arc::new(MongoService)`** — **`MongoService`** is a unit handle; connection state lives in **`MongoModule`**’s static init (**[`mongo.rs`](../../nestrs/src/mongo.rs)**).

### D.8 `_id`, `ObjectId`, and loose `Document`

- For **`_id` in GraphQL**, common pattern is **`String`** (hex) + parse in mutations (**Recipe B.9**).
- For truly schemaless payloads, resolve **`mongodb::bson::Document`** and map selected keys into **`serde_json::Value`** or custom **`SimpleObject`** types.

### D.9 Curl (POST to `/graphql` after any global prefix)

```bash
curl -s -X POST http://127.0.0.1:3000/graphql \
  -H 'content-type: application/json' \
  -d '{"query":"mutation { seedProfile(email: \"a@b.com\", displayName: \"Ada\") }"}'

curl -s -X POST http://127.0.0.1:3000/graphql \
  -H 'content-type: application/json' \
  -d '{"query":"{ mongoPing profileCount profiles(take: 5) { email } }"}'
```

GraphQL field names default to **camelCase** (**`display_name` → `displayName`** in queries).

### D.10 GraphQL + Mongo troubleshooting

| Symptom | Things to check |
|---------|------------------|
| Empty **`profiles`** | Seed via mutation REST (**Recipe B**) or **§ D.6**; confirm DB name **`app`**. |
| Resolver errors surface as GraphQL errors | **`async_graphql::Error::new`** vs **`map_err`**—keep messages safe for clients. |
| Slow scans | Add indexes (**Recipe B.10**) before exposing wide **`find`** resolvers. |

### D.11 Single-file Mongo GraphQL app (`src/main.rs`)

Copy-paste **one** crate root file (matches **§ D.1** deps). Split into **`lib.rs`** / modules when you outgrow it.

Prerequisites: **MongoDB** reachable at **`MONGODB_URI`** (defaults to **`mongodb://127.0.0.1:27017`**).

```rust
use async_graphql::{EmptySubscription, Object, Schema, SimpleObject};
use bson::doc;
use futures_util::TryStreamExt;
use mongodb::options::FindOptions;
use nestrs::graphql::with_default_limits;
use nestrs::{module, MongoModule, MongoService, NestFactory};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileDoc {
    pub email: String,
    pub display_name: String,
}

#[derive(SimpleObject, Clone)]
pub struct GqlProfile {
    pub email: String,
    pub display_name: String,
}

pub struct QueryRoot {
    mongo: Arc<MongoService>,
}

#[Object]
impl QueryRoot {
    async fn mongo_ping(&self) -> bool {
        self.mongo.ping().await.is_ok()
    }

    async fn profile_count(&self) -> Result<u64, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.estimated_document_count()
            .await
            .map_err(async_graphql::Error::new)
    }

    async fn profiles(&self, take: Option<i64>) -> Result<Vec<GqlProfile>, async_graphql::Error> {
        let lim = take.unwrap_or(20).clamp(1, 100);
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        let opts = FindOptions::builder().limit(Some(lim)).build();
        let cursor = col.find(doc! {}).await.map_err(async_graphql::Error::new)?;
        let rows: Vec<ProfileDoc> = cursor.try_collect().await.map_err(async_graphql::Error::new)?;
        Ok(rows
            .into_iter()
            .map(|p| GqlProfile {
                email: p.email,
                display_name: p.display_name,
            })
            .collect())
    }
}

pub struct MutationRoot {
    mongo: Arc<MongoService>,
}

#[Object]
impl MutationRoot {
    async fn seed_profile(
        &self,
        email: String,
        display_name: String,
    ) -> Result<bool, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        col.insert_one(ProfileDoc { email, display_name })
            .await
            .map_err(async_graphql::Error::new)?;
        Ok(true)
    }
}

#[module(imports = [MongoModule])]
pub struct AppModule;

#[tokio::main]
async fn main() {
    let _ = MongoModule::for_root(
        std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".into()),
    );

    let mongo = Arc::new(MongoService);
    let schema = with_default_limits(Schema::build(
        QueryRoot {
            mongo: mongo.clone(),
        },
        MutationRoot { mongo },
        EmptySubscription,
    ))
    .finish();

    NestFactory::create::<AppModule>()
        .enable_graphql(schema)
        .listen(3000)
        .await
        .expect("listen");
}
```

Run: **`cargo run`**, then **§ D.9** curls against **`http://127.0.0.1:3000/graphql`**.

### D.12 GraphQL CRUD mutations (update + delete by email or id)

Extend **§ D.11** **`MutationRoot`** with targeted writes (same **`ProfileDoc`** / **`app`** DB):

```rust
#[Object]
impl MutationRoot {
    async fn update_profile_by_email(
        &self,
        email: String,
        display_name: String,
    ) -> Result<bool, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        let r = col
            .update_one(
                doc! { "email": &email },
                doc! { "$set": { "display_name": &display_name } },
                None,
            )
            .await
            .map_err(async_graphql::Error::new)?;
        Ok(r.modified_count > 0 || r.matched_count > 0)
    }

    async fn delete_profile_by_email(&self, email: String) -> Result<u64, async_graphql::Error> {
        let db = self.mongo.database("app").await.map_err(async_graphql::Error::new)?;
        let col = db.collection::<ProfileDoc>("profiles");
        let r = col
            .delete_one(doc! { "email": &email })
            .await
            .map_err(async_graphql::Error::new)?;
        Ok(r.deleted_count)
    }
}
```

Add **`#[derive(Debug, serde::Deserialize, serde::Serialize)]`** on **`ProfileDoc`** when you enable **`_id`** round-trips (**§ B.17**).

---

# Recipe E — gRPC microservice + PostgreSQL + Prisma

**Goal:** **`#[micro_routes]`** over **gRPC** JSON wire + **`PrismaService`** in handlers.

### E.1 `Cargo.toml`

```toml
nestrs = { version = "0.3.8", features = ["microservices", "microservices-grpc"] }
nestrs-prisma = { version = "0.3.8", features = ["sqlx", "sqlx-postgres"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### E.2 DTOs + micro handler using Prisma

```rust
use nestrs::prelude::*;
use nestrs_prisma::PrismaService;
use std::sync::Arc;

#[dto]
struct SqlPingReq {
    #[validate(range(min = 1, max = 1))]
    check: i32,
}

#[dto]
struct SqlPingRes {
    #[IsString]
    sample: String,
}

#[injectable]
struct SqlMicroHandler {
    prisma: Arc<PrismaService>,
}

#[micro_routes]
impl SqlMicroHandler {
    #[message_pattern("sql.ping")]
    async fn ping(&self, _req: SqlPingReq) -> Result<SqlPingRes, HttpException> {
        let sample = self
            .prisma
            .query_scalar("SELECT 1")
            .await
            .map_err(|e| InternalServerErrorException::new(e))?;
        Ok(SqlPingRes { sample })
    }
}
```

### E.3 Module with Prisma + microservices list

```rust
#[module(
    imports = [PrismaModule],
    providers = [SqlMicroHandler],
    microservices = [SqlMicroHandler],
)]
pub struct AppModule;
```

### E.4 gRPC bind + listen

```rust
use std::net::{Ipv4Addr, SocketAddr};

let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 50051));
let app = NestFactory::create_microservice_grpc::<AppModule>(
    nestrs::microservices::GrpcMicroserviceOptions::bind(addr),
);
app.listen().await;
```

For **`PrismaModule::for_root_with_options`** + **`#[tokio::main]`** + **`AppModule`** in **one file**, see **`§ E.14`**.

### E.5 Hybrid: gRPC + HTTP (REST/GraphQL) same process

Build your **`async_graphql::Schema`** first (Recipe **C**), then merge HTTP options on the nested [`NestApplication`](appendix-api-cookbook.md):

```rust
use async_graphql::Schema;

let schema: Schema<QueryRoot, async_graphql::EmptyMutation, async_graphql::EmptySubscription> =
    /* … build … */;

let grpc_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 50051));
let app = NestFactory::create_microservice_grpc::<AppModule>(
        nestrs::microservices::GrpcMicroserviceOptions::bind(grpc_addr),
    )
    .also_listen_http(3000)
    .configure_http(move |nest| nest.enable_graphql(schema).set_global_prefix("api"));

app.listen().await;
```

**`Schema`** must be **`Clone`** if the closure can run more than once—prefer building inside **`configure_http`** or wrapping in **`Arc`** per your layout.

Wire format: **[`nestrs_microservices::wire`](https://docs.rs/nestrs-microservices)** JSON inside protobuf—see [GraphQL, WebSockets & microservices DX § JSON wire](graphql-ws-micro-dx.md).

### E.6 TCP microservice reference (same patterns, different transport)

Integration-style layout matches **[`nestrs/tests/microservices_tcp_integration.rs`](../../nestrs/tests/microservices_tcp_integration.rs)** (`#[module(..., microservices = [...])]` + **`create_microservice`** + **`also_listen_http`**).

### E.7 Second RPC: keyed lookup (`#[message_pattern]`)

```rust
#[dto]
struct SqlScalarReq {
    #[validate(range(min = 1, max = 256))]
    sql_len_hint: i32,
}

#[dto]
struct EchoSqlRow {
    #[IsString]
    one: String,
}

#[micro_routes]
impl SqlMicroHandler {
    #[message_pattern("sql.scalar.one")]
    async fn scalar_one(&self, _req: SqlScalarReq) -> Result<EchoSqlRow, HttpException> {
        let one = self
            .prisma
            .query_scalar("SELECT 1")
            .await
            .map_err(|e| InternalServerErrorException::new(e))?;
        Ok(EchoSqlRow { one })
    }
}
```

Prefer **one** `#[micro_routes] impl SqlMicroHandler { … }` that also contains **`ping`** from **§ E.2**—easier to read than several `impl` blocks (multiple blocks can work, but consolidate when possible).

Message pattern strings are your **gRPC / wire** contract—treat them like topic or method names; version them if the payload changes.

### E.8 Event-style handler (`#[event_pattern]`)

Fire-and-forget events (no return value) — see **[`nestrs/tests/microservices_tcp_integration.rs`](../../nestrs/tests/microservices_tcp_integration.rs)**:

```rust
#[dto]
struct UserCreatedEvent {
    id: i64,
}

#[micro_routes]
impl SqlMicroHandler {
    #[event_pattern("user.created.sql_audit")]
    async fn audit_created(&self, evt: UserCreatedEvent) {
        tracing::info!(id = evt.id, "audit stub");
    }
}
```

Register **`SqlMicroHandler`** once in **`microservices = [...]`** — multiple **`#[micro_routes]`** **`impl`** blocks can live on the same handler **`struct`** when you consolidate patterns.

### E.9 Guards on RPC (`#[use_micro_guards]`)

From the same TCP integration test:

```rust
#[derive(Default)]
struct BlockNegativeGuard;

#[nestrs::async_trait]
impl nestrs::microservices::MicroCanActivate for BlockNegativeGuard {
    async fn can_activate_micro(
        &self,
        _pattern: &str,
        payload: &serde_json::Value,
    ) -> Result<(), nestrs::microservices::TransportError> {
        if payload.get("check").and_then(|v| v.as_i64()) == Some(-1) {
            return Err(nestrs::microservices::TransportError::new("blocked"));
        }
        Ok(())
    }
}

#[micro_routes]
impl SqlMicroHandler {
    #[message_pattern("sql.ping.guarded")]
    #[use_micro_guards(BlockNegativeGuard)]
    async fn ping_guarded(&self, req: SqlPingReq) -> Result<SqlPingRes, HttpException> {
        self.ping(req).await
    }
}
```

*(gRPC transport uses the same **`MicroCanActivate`** hooks—payload is JSON before dispatch.)*

### E.10 Row-shaped RPC with `prisma_model!`

After **`nestrs_prisma::prisma_model!(User => "users", { … })`**, expose thin RPC DTOs:

```rust
#[dto]
struct UserByIdReq {
    #[validate(range(min = 1))]
    id: i64,
}

#[dto]
struct UserRpcRow {
    #[validate(range(min = 1))]
    id: i64,
    #[IsEmail]
    email: String,
    #[IsString]
    name: String,
}

#[micro_routes]
impl SqlMicroHandler {
    #[message_pattern("user.by_id")]
    async fn user_by_id(&self, req: UserByIdReq) -> Result<UserRpcRow, HttpException> {
        let u = self
            .prisma
            .user()
            .find_unique(user::id::equals(req.id))
            .await
            .map_err(|e| InternalServerErrorException::new(e))?
            .ok_or_else(|| NotFoundException::new("user"))?;
        Ok(UserRpcRow {
            id: u.id,
            email: u.email,
            name: u.name,
        })
    }
}
```

Exact **`user::`** helpers follow your **`prisma_model!`** expansion (**Recipe A**).

### E.11 TCP vs gRPC selection

| Transport | Bootstrap |
|-----------|-----------|
| **gRPC + JSON wire** | **`NestFactory::create_microservice_grpc`** (**§ E.4**) |
| **TCP JSON** | **`NestFactory::create_microservice`** + **`TcpMicroserviceOptions`** (**§ E.6**) |

Business code (**DTOs**, **`#[message_pattern]`**, Prisma calls) stays the same—only transport options change.

### E.12 Hybrid HTTP + micro smoke layout

Mirroring the integration test:

```rust
NestFactory::create_microservice::<AppModule>(TcpMicroserviceOptions::new(ms_addr))
    .also_listen_http(http_port);

NestFactory::create_microservice_grpc::<AppModule>(GrpcMicroserviceOptions::bind(grpc_addr))
    .also_listen_http(3000)
    .configure_http(|nest| nest.set_global_prefix("api"));
```

Pick **one** bootstrap path per process; duplicate **`listen`** attempts usually mean separate binaries or profiles.

### E.13 Microservice troubleshooting

| Symptom | Things to check |
|---------|------------------|
| Handler never runs | **`microservices = [SqlMicroHandler]`** on **`#[module]`** and provider registered. |
| **`TransportError`** / guard failures | **`MicroCanActivate`** rejection payload vs client JSON shape. |
| **`HttpException`** over wire | Serialized like HTTP errors—see **`microservices_tcp_integration`** assertions. |
| Pool errors under load | **`PrismaModule`** pool sizing vs concurrent RPC workers. |

### E.14 Single-file gRPC microservice (`src/main.rs`)

Copy-paste **one** crate root file for the **`sql.ping`** RPC from **§ E.2–E.4**—no **`prisma_model!`** here (that lives in **§ E.10** once you add tables).

**Needs:**

- **`DATABASE_URL`** pointing at a reachable **PostgreSQL** instance (matches **`sqlx-postgres`** on **`nestrs-prisma`**).
- A **`prisma/schema.prisma`** on disk at the path passed to **`schema_path`** (same convention as **[`examples/hello-app`](../../examples/hello-app)** / **Recipe A**—adjust the path if your layout differs).

```rust
use nestrs::prelude::*;
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

#[dto]
struct SqlPingReq {
    #[validate(range(min = 1, max = 1))]
    check: i32,
}

#[dto]
struct SqlPingRes {
    #[IsString]
    sample: String,
}

#[injectable]
struct SqlMicroHandler {
    prisma: Arc<PrismaService>,
}

#[micro_routes]
impl SqlMicroHandler {
    #[message_pattern("sql.ping")]
    async fn ping(&self, _req: SqlPingReq) -> Result<SqlPingRes, HttpException> {
        let sample = self
            .prisma
            .query_scalar("SELECT 1")
            .await
            .map_err(|e| InternalServerErrorException::new(e))?;
        Ok(SqlPingRes { sample })
    }
}

#[module(
    imports = [PrismaModule],
    providers = [SqlMicroHandler],
    microservices = [SqlMicroHandler],
)]
pub struct AppModule;

#[tokio::main]
async fn main() {
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url(
            std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://postgres:postgres@127.0.0.1:5432/myapp".into()
            }),
        )
        .pool_min(2)
        .pool_max(10)
        .schema_path("prisma/schema.prisma"),
    );

    NestFactory::create_microservice_grpc::<AppModule>(
        nestrs::microservices::GrpcMicroserviceOptions::bind(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            50051,
        ))),
    )
    .listen()
    .await
    .expect("microservice");
}
```

Invoke **`sql.ping`** from your gRPC client using the **JSON-wire** payloads described in **[GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md)** / **`nestrs_microservices`**. For a **TCP** listener instead of gRPC, swap bootstrap to **`NestFactory::create_microservice`** (**§ E.6** / **E.11**)—handlers stay identical.

### E.15 Microservice CRUD patterns (`#[message_pattern]`)

Same **`prisma_model!(User => …)`** as **§ E.10**, extended:

| Pattern | Request DTO (sketch) | Response / error |
|---------|----------------------|-------------------|
| **`user.create`** | `{ "email", "name" }` | New row or **409** from Prisma |
| **`user.get`** | `{ "id" }` | **§ E.10** |
| **`user.list`** | `{ "skip", "take" }` | **`Vec<UserRpcRow>`** via **`find_many_with_options`** |
| **`user.update`** | `{ "id", …fields }` | **`update`** / **`update_many`** |
| **`user.delete`** | `{ "id" }` | **`delete_many`** → affected count |

Always version payloads (**`schema_version`** in JSON) when multiple services evolve independently.

---

# Recipe F — Microservices + message brokers

**Goal:** run **`#[micro_routes]`** handlers behind **brokers** (not only TCP/gRPC) and call peers with **`ClientProxy`** / **`ClientsModule`**.

Deep reference: **[`MICROSERVICES.md`](../../MICROSERVICES.md)**, **[`nestrs-microservices` README](../../nestrs-microservices/README.md)**, [Microservices](microservices.md).

### F.1 Which transports have `NestFactory` helpers?

| Transport | Umbrella features | Bootstrap |
|-----------|-------------------|-----------|
| **TCP** | **`microservices`** | **`NestFactory::create_microservice`** |
| **NATS** | **`microservices-nats`** | **`NestFactory::create_microservice_nats`** |
| **Redis** | **`microservices-redis`** | **`NestFactory::create_microservice_redis`** |
| **gRPC + JSON wire** | **`microservices-grpc`** | **`NestFactory::create_microservice_grpc`** (**Recipe E**) |
| **RabbitMQ** | **`microservices-rabbitmq`** | **`NestFactory::create_microservice_rabbitmq`** |
| **Kafka listener** | **`microservices-kafka`** | Compose **`KafkaMicroserviceServer`** (see **§ F.10**) |
| **MQTT listener** | **`microservices-mqtt`** | Compose **`MqttMicroserviceServer`** — same idea |

All transports deserialize the same **`WireRequest`** JSON (**[`wire`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/index.html)**).

### F.2 `Cargo.toml` feature matrix (examples)

```toml
# NATS micro listener + NATS client in another crate
nestrs = { version = "0.3.8", features = ["microservices", "microservices-nats"] }

# Redis micro listener
nestrs = { version = "0.3.8", features = ["microservices", "microservices-redis"] }

# RabbitMQ work-queue listener
nestrs = { version = "0.3.8", features = ["microservices", "microservices-rabbitmq"] }

# Kafka transport (client) — enable kafka on nestrs-microservices transitively
nestrs = { version = "0.3.8", features = ["microservices", "microservices-kafka"] }
```

Match **exactly one** broker feature set per binary unless you know how layers compose.

### F.3 NATS micro listener + Docker

```bash
docker run --name nestrs-nats -p 4222:4222 -d nats:2-alpine
```

Server bootstrap:

```rust
NestFactory::create_microservice_nats::<AppModule>(
    nestrs::microservices::NatsMicroserviceOptions::new(
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into()),
    ),
)
.listen()
.await;
```

Remote caller (**CLI / other service**) — **`ClientProxy`** over **`NatsTransport`**:

```rust
use nestrs::microservices::{ClientProxy, NatsTransport, NatsTransportOptions};
use std::sync::Arc;

let proxy = ClientProxy::new(Arc::new(NatsTransport::new(NatsTransportOptions::new(
    "nats://127.0.0.1:4222",
))));
let pong: serde_json::Value = proxy.send("sql.ping", &serde_json::json!({"check": 1})).await?;
```

Typed **`send<TReq,TRes>`** works when your DTOs derive **`serde`** (**[`ClientProxy`](https://docs.rs/nestrs-microservices)**).

### F.4 Redis micro listener + Docker

```bash
docker run --name nestrs-redis -p 6379:6379 -d redis:7-alpine
```

```rust
NestFactory::create_microservice_redis::<AppModule>(
    nestrs::microservices::RedisMicroserviceOptions::new("redis://127.0.0.1:6379")
        .with_prefix("myapp"),
)
.listen()
.await;
```

Client:

```rust
use nestrs::microservices::{ClientProxy, RedisTransport, RedisTransportOptions};
let proxy = ClientProxy::new(Arc::new(RedisTransport::new(RedisTransportOptions::new(
    "redis://127.0.0.1:6379",
))));
proxy.emit("order.created", &serde_json::json!({"id": 1})).await?; // fire-and-forget
```

### F.5 RabbitMQ micro listener

```bash
docker run --name nestrs-rmq -p 5672:5672 -p 15672:15672 -e RABBITMQ_DEFAULT_USER=guest -e RABBITMQ_DEFAULT_PASS=guest -d rabbitmq:3-management-alpine
```

```rust
NestFactory::create_microservice_rabbitmq::<AppModule>(
    nestrs::microservices::RabbitMqMicroserviceOptions::new("amqp://guest:guest@127.0.0.1:5672/")
        .with_work_queue("users.rpc"),
)
.listen()
.await;
```

JSON **`WireRequest`** bodies land on the **work queue**; replies use private reply queues per the adapter (**[`README`](../../nestrs-microservices/README.md)** § RabbitMQ).

### F.6 `ClientsModule`: multiple named brokers in one HTTP app

Register brokers as a **dynamic module**, then merge it into your root module (**[`dynamic_modules.rs`](../../nestrs/tests/dynamic_modules.rs)**):

```rust
use nestrs::microservices::{
    ClientConfig, ClientsModule, ClientsService, NatsTransportOptions, RedisTransportOptions,
};
use nestrs::prelude::*;
use std::sync::Arc;

let dm: DynamicModule = ClientsModule::register(&[
    ClientConfig::nats(
        "USERS_MS",
        NatsTransportOptions::new(std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into())),
    ),
    ClientConfig::redis(
        "AUDIT_BUS",
        RedisTransportOptions::new("redis://127.0.0.1:6379"),
    ),
]);

let app = NestFactory::create_with_modules::<AppModule, _>([dm]);
```

Resolve proxies from **`ClientsService`** inside any **`#[injectable]`**:

```rust
let clients: Arc<ClientsService> = registry.get(); // or inject via ctor field
clients.expect("USERS_MS").send("user.get", &req).await?;
```

`ClientsModule` also registers an **`EventBus`** (see **Recipe G**). When **exactly one** client is registered, a default **`ClientProxy`** exists.

### F.7 Broker selection (operational view)

| Need | Prefer |
|------|--------|
| Lowest ops, quick dev | **TCP** or **Redis** (single container) |
| Cloud-native streaming, replay | **Kafka** clients + listener process |
| Managed queues / DLQ | **RabbitMQ** |
| Millions of tiny subjects | **NATS** |
| Embedded / IoT bridges | **MQTT** |

### F.8 TLS / SASL (Kafka)

Kafka supports **`KafkaConnectionOptions`** / **`KafkaSaslOptions`** on both **`KafkaTransportOptions`** (client) and **`KafkaMicroserviceOptions`** (listener). Mirror cluster settings from your operator (MSK, Strimzi, Confluent).

### F.9 Broker health probes

nestrs exposes broker-specific **`HealthIndicator`** stubs (**`NatsBrokerHealth`**, **`RedisBrokerHealth`**, **`kafka_cluster_reachable_with`**) — combine with **`enable_readiness_check`** on HTTP sides (**[API cookbook](appendix-api-cookbook.md)**).

### F.10 Kafka listener process (advanced)

There is **no** `NestFactory::create_microservice_kafka` today. Run **`KafkaMicroserviceServer::new(KafkaMicroserviceOptions::new(vec!["127.0.0.1:9092".into()]), handlers)`** with **`handlers`** collected the same way **`NestFactory::create_microservice`** builds **`TcpMicroserviceServer`** (**[`nestrs/src/lib.rs`](../../nestrs/src/lib.rs)** search **`microservice_handlers`**). Prefer **`ClientConfig::kafka`** for callers until a first-party helper lands.

---

# Recipe G — Event-driven architecture (EDA)

**Patterns:** integration events cross service boundaries; domain events stay in-process; both map cleanly to **`emit`** / **`EventBus`**.

See **[`MICROSERVICES.md`](../../MICROSERVICES.md)** § Integration events, CQRS/outbox, reliability.

### G.1 Fire-and-forget over the broker (`emit`)

- Use **`Transport::emit_json`** / **`ClientProxy::emit`** for **`order.created`**, **`user.deleted`**, audit fan-out.
- Use **`send`** when the caller needs a typed reply (RPC), not pure EDA.

### G.2 `#[event_pattern]` on micro handlers (consumer)

Already in **§ E.8** — handlers take an event DTO and return **`()`**. These are **inbound integration events** on the micro transport.

### G.3 In-process `EventBus` + `#[on_event]` (same binary)

For **domain** reactions without a broker (or before you add one), use **`nestrs_events::EventBus`** with **`#[event_routes]`** + **`#[on_event("topic")]`**:

```rust
use nestrs::prelude::*;
use serde_json::json;

#[injectable]
struct OrderProjector;

#[event_routes]
impl OrderProjector {
    #[on_event("order.created")]
    async fn apply(&self, payload: serde_json::Value) {
        tracing::info!(?payload, "projection");
    }
}

// Register `OrderProjector` in providers; on HTTP/micro bootstrap, `wire_on_event_handlers`
// subscribes these methods to the shared `EventBus`.
```

Emit from a service:

```rust
async fn place_order(bus: std::sync::Arc<nestrs::microservices::EventBus>) {
    bus.emit("order.created", &json!({ "id": 99, "event_version": 1 })).await;
}
```

Wire **`EventBus`** by importing **`ClientsModule::register`** (even with a dummy transport in tests) or construct **`EventBus::new()`** and register manually in custom modules—**[`microservices_events.rs`](../../nestrs-microservices/tests/microservices_events.rs)** shows **`ClientsModule`** exporting **`EventBus`**.

### G.4 HTTP writes DB then **`emit`** (integration style)

Typical **`OrderController`** flow:

1. **`ValidatedBody`** → **`OrderService::create`** → single DB transaction.
2. After commit, **`clients.expect("AUDIT").emit("order.created", &dto)`**.
3. On failure after commit, rely on **outbox** (**§ G.6**) instead of losing the event.

### G.5 Idempotency & versioning

- Consumers must tolerate **at-least-once** delivery — guard with **`event_id`** / **`idempotency_key`**.
- Include **`event_version`** in payloads when evolving schemas (**[`MICROSERVICES.md`](../../MICROSERVICES.md)**).

### G.6 Outbox pattern (Postgres / Prisma)

1. In the same transaction as business rows, insert an **outbox** row (`payload`, **`topic`**, **`created_at`**).
2. Background worker reads outbox, calls **`emit`**, marks sent.
3. Retries + dead-letter table per organizational policy.

**nestrs** does not ship an outbox crate — implement with **`prisma_model!`** or raw SQL.

### G.7 CQRS read models

Subscribe to **`order.created`** (NATS/Kafka/`on_event`) and update a read-optimized store (Redis, Mongo, Elasticsearch). Keeps HTTP write path thin.

### G.8 When to pick which recipe

| Scenario | Recipe |
|----------|--------|
| CRUD REST + SQL | **A** |
| CRUD document API | **B** |
| Public graph API + SQL | **C** |
| Graph + documents | **D** |
| Service-to-service RPC + Prisma | **E** |
| Broker-backed RPC/events | **F** |
| Domain + integration events, outbox | **G** |

---

## More combinations (quick reference)

| Need | nestrs / crates |
|------|------------------|
| REST + MySQL + Prisma | `sqlx-mysql` on **`nestrs-prisma`** |
| REST + SQLite + Prisma | `sqlx-sqlite`; see **Recipe A.4**, **hello-app** |
| REST + SQLx only | **`database-sqlx`** on **`nestrs`** |
| WebSockets + DB | **`ws`** + **`nestrs-ws`** + same providers |
| Kafka / NATS / Redis micros | **`microservices-*`** features |
| OpenAPI + any DB | **`openapi`** + services calling Prisma/Mongo/SQLx |
| CLI scaffolds | **`nestrs generate resource --transport …`** — [CLI](cli.md) |

---

## CLI acceleration (many transports)

```bash
nestrs generate resource users --transport rest
nestrs generate resource users --transport graphql
nestrs generate resource users --transport grpc
nestrs generate resource users --transport ws
nestrs generate resource audit --transport microservice
```

Replace generated stub providers with **Recipes A–G** (CRUD stacks **A–D**, RPC **E**, brokers **F**, events **G**).

---

## Where to deepen

| Topic | Doc |
|------|-----|
| HTTP middleware order | [HTTP pipeline order](http-pipeline-order.md) |
| Swagger / security | [OpenAPI & HTTP](openapi-http.md) |
| Production | [Production](production.md), [Secure defaults](secure-defaults.md) |
| Metrics / tracing | [Observability](observability.md) |
| Full `NestApplication` API | [API cookbook](appendix-api-cookbook.md) |

---

**Summary:** Recipes **A–G** pair **stack-specific CRUD** with **broker-backed microservices** (**F**) and **EDA** primitives (**G** — **`emit`**, **`EventBus`**, **`#[on_event]`**, outbox guidance). **Recipes D.11** and **E.14** stay the minimal **single-file** GraphQL‑Mongo and **gRPC** starters. Compose **REST + GraphQL + hybrid HTTP/micro** with **one module graph**, adding **`ClientsModule`** / **`also_listen_http`** where needed.
