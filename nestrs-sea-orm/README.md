# nestrs-sea-orm

SeaORM adapter for [nestrs](https://github.com/Joshyahweh/nestrs) — NestJS TypeORM /
NestRS-style data layer: [`Repo`](https://docs.rs/nestrs-sea-orm), ambient
transactions, [`RowAuthz`](https://docs.rs/nestrs-sea-orm),
[`bind_read`](https://docs.rs/nestrs-sea-orm) / `Bind`, and optional
`expose_schema` for OpenAPI.

```toml
[dependencies]
nestrs = { version = "1.5.0", features = ["sea-orm-authz", "sea-orm-expose"] }
# or:
nestrs-sea-orm = { version = "1.5.0", features = ["expose"] }
```

```rust
use nestrs::current_ability_authz;
use nestrs_sea_orm::{
    bind_read, install_sea_orm_transactional_middleware, Repo, SeaOrmModule,
};

// Connect before NestFactory::create
let module = SeaOrmModule::for_root_async(&std::env::var("DATABASE_URL")?).await?;

// In a guarded handler:
let authz = current_ability_authz().expect("policies");
let post = bind_read(&Repo::<post::Entity>::new(db), &authz, "Post", id).await?;

NestFactory::create::<AppModule>()
    .require_route_posture() // panics at listen if any route lacks #[public]/guards
    .listen_graceful(3000)
    .await?;
```

Layer ambient transactions with
`axum::middleware::from_fn_with_state(db, install_sea_orm_transactional_middleware)`.

Implement [`RowAuthz`](https://docs.rs/nestrs-sea-orm/latest/nestrs_sea_orm/trait.RowAuthz.html)
yourself, or enable umbrella features `sea-orm` + `authz` and use
`AbilityAuthz` / `current_ability_authz()` / `attach_row_authz_middleware`.

Docs: [Mintlify adapters/sea-orm](https://docs.nestrs.dev/adapters/sea-orm) ·
[docs.rs/nestrs-sea-orm](https://docs.rs/nestrs-sea-orm)
