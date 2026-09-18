//! SeaORM adapter — NestJS TypeORM / Sequelize analogue.
//!
//! TypeORM and Sequelize are Node ORMs. The Rust equivalent we bind into
//! the nestrs DI graph is [SeaORM](https://www.sea-ql.org/SeaORM/).

#![doc(html_root_url = "https://docs.rs/nestrs-sea-orm/1.3.0")]

use nestrs_core::{DynamicModule, ProviderRegistry};
use sea_orm::{Database, DatabaseConnection, DbErr};
use std::any::TypeId;
use std::sync::Arc;

/// `TypeOrmModule.forRoot` analogue.
pub struct SeaOrmModule;

impl SeaOrmModule {
    /// Connect and export [`DatabaseConnection`] for injection.
    ///
    /// Must complete **before** `NestFactory::create` (same rule as
    /// `MongoModule::for_root_async`).
    pub async fn for_root_async(database_url: impl AsRef<str>) -> Result<DynamicModule, DbErr> {
        let conn = Database::connect(database_url.as_ref()).await?;
        let mut registry = ProviderRegistry::new();
        registry.register_use_value::<DatabaseConnection>(Arc::new(conn));
        Ok(DynamicModule {
            registry,
            router: axum::Router::new(),
            exports: vec![TypeId::of::<DatabaseConnection>()],
        })
    }
}

/// SeaORM connection type (inject `Arc<DatabaseConnection>`).
pub type DbConn = DatabaseConnection;

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn sqlite_memory_connects() {
        let dm = super::SeaOrmModule::for_root_async("sqlite::memory:")
            .await
            .expect("sqlite memory should connect");
        assert!(!dm.exports.is_empty());
    }
}
