# SeaORM adapter (`Repo`, Bind, ambient tx, RowAuthz)

The **recommended** NestJS TypeORM / Sequelize analogue is [`nestrs-sea-orm`](https://docs.rs/nestrs-sea-orm).

## Features (1.5.0)

- `SeaOrmModule::for_root_async` / `from_connection` — DI export of `Arc<DatabaseConnection>`
- `Repo<E>` — typed repository; prefers an ambient request transaction when present
- `install_sea_orm_transactional_middleware` — commit on 2xx/3xx/4xx, rollback on 5xx
- `RowAuthz` / `AbilityAuthz` — deny-closed authorized CRUD helpers
- `bind_read` / `Bind` — NestRS-style authorized path to row (`BindError` as HTTP status)
- `attach_row_authz_middleware` — put `BoundAuthz` on request extensions
- `expose_schema` (feature `expose`) — one `JsonSchema` model to OpenAPI components
- Pair with `NestApplication::require_route_posture` + `#[public]` / `#[use_guards]`

## Quick example

```rust
use nestrs::current_ability_authz;
use nestrs_sea_orm::{bind_read, Repo};

let repo = Repo::<post::Entity>::new(db);
let authz = current_ability_authz().expect("policies");
let post = bind_read(&repo, &authz, "Post", id).await?;
```

See Mintlify adapters/sea-orm for Bind, expose, GraphQL DataLoader tips, and posture.
