# nestrs-sea-orm

SeaORM adapter for nestrs — NestJS TypeORM / Sequelize analogue with a typed
[`Repo`](https://docs.rs/nestrs-sea-orm), ambient request transactions, and
pluggable [`RowAuthz`](https://docs.rs/nestrs-sea-orm).

```toml
nestrs-sea-orm = "1.4.0"
# or via the umbrella:
nestrs = { version = "1.4.0", features = ["sea-orm", "authz"] }
```

## Connect

```rust,ignore
let orm = nestrs_sea_orm::SeaOrmModule::for_root_async("sqlite::memory:").await?;
// NestFactory::create_with_modules::<AppModule, _>([orm])
```

`from_connection` is also available for tests / custom pools. Construct
`Repo::<Entity>::new(db)` in your services after injecting
`Arc<DatabaseConnection>`.

## Repo + ambient transactions

```rust,ignore
use nestrs_sea_orm::{
    install_sea_orm_transactional_middleware, Repo,
};
use axum::middleware::from_fn_with_state;

let repo = Repo::<post::Entity>::new(db.clone());
let router = router.layer(from_fn_with_state(
    db.clone(),
    install_sea_orm_transactional_middleware,
));
// Inside a request: Repo methods prefer the ambient tx (commit on 2xx/3xx/4xx).
```

## Row-level authz

Implement [`RowAuthz`](https://docs.rs/nestrs-sea-orm/latest/nestrs_sea_orm/trait.RowAuthz.html)
or use `nestrs::AbilityAuthz` (features `sea-orm` + `authz`):

```rust,ignore
use nestrs::{current_ability_authz, AbilityAuthz};

let authz = current_ability_authz().expect("policies middleware");
let row = repo.find_by_id_authorized(&authz, "Post", id).await?;
```

## Route posture (umbrella)

```rust,ignore
NestFactory::create::<AppModule>()
    .require_route_posture() // panics at listen if any route lacks #[public]/guards
    .listen_graceful(3000)
    .await;
```
