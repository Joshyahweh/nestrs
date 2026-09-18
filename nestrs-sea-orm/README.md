# nestrs-sea-orm

SeaORM adapter for nestrs. NestJS TypeORM / Sequelize have no Rust port;
SeaORM is the closest first-class ORM we can plug into the DI graph.

```toml
nestrs-sea-orm = "1.3.0"
```

```rust,ignore
let orm = nestrs_sea_orm::SeaOrmModule::for_root_async("sqlite::memory:").await?;
// pass `orm` into NestFactory::create_with_modules
```
