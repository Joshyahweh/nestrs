# SeaORM adapter

`nestrs-sea-orm` is the **recommended ORM path** for nestrs (NestJS TypeORM /
Sequelize analogue).

## Install

```toml
nestrs = { version = "1.4.0", features = ["sea-orm", "authz"] }
# or
nestrs-sea-orm = "1.4.0"
```

## Features

- `SeaOrmModule::for_root_async` / `from_connection` — export `Arc<DatabaseConnection>`
- `Repo<E>` — typed repository; prefers an ambient request transaction when present
- `install_sea_orm_transactional_middleware` — commit on 2xx/3xx/4xx, rollback on 5xx
- `RowAuthz` + umbrella `AbilityAuthz` — deny-closed authorized helpers

See Mintlify [`adapters/sea-orm`](../../mintlify-docs/adapters/sea-orm.mdx) and
crate rustdoc for full examples. Pair with
`NestApplication::require_route_posture()` so unguarded routes fail at boot.
