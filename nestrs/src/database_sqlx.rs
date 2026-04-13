//! Generic **SQL** connectivity via [SQLx](https://github.com/launchbadge/sqlx) (`AnyPool`), NestJS “Database” chapter analogue alongside [`nestrs_prisma`](https://crates.io/crates/nestrs-prisma).

use crate::core::DatabasePing;
use crate::{injectable, module};
use async_trait::async_trait;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

static SQLX_URL: OnceLock<String> = OnceLock::new();
static SQLX_POOL: OnceCell<sqlx::AnyPool> = OnceCell::const_new();

async fn ensure_pool() -> Result<&'static sqlx::AnyPool, String> {
    SQLX_POOL
        .get_or_try_init(|| async {
            let url = SQLX_URL.get().cloned().ok_or_else(|| {
                "SqlxDatabaseModule::for_root must be called before using SqlxDatabaseService"
                    .to_string()
            })?;
            sqlx::any::AnyPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await
                .map_err(|e| format!("sqlx connect: {e}"))
        })
        .await
}

/// Injectable handle to the shared [`sqlx::AnyPool`].
#[injectable]
pub struct SqlxDatabaseService;

impl SqlxDatabaseService {
    pub async fn pool(&self) -> Result<&'static sqlx::AnyPool, String> {
        ensure_pool().await
    }

    pub async fn ping(&self) -> Result<(), String> {
        let pool = ensure_pool().await?;
        sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .map_err(|e| format!("sqlx ping: {e}"))?;
        Ok(())
    }
}

#[async_trait]
impl DatabasePing for SqlxDatabaseService {
    async fn ping_database(&self) -> Result<(), String> {
        self.ping().await
    }
}

/// Call [`SqlxDatabaseModule::for_root`] before `NestFactory::create`, then `imports = [SqlxDatabaseModule]`.
#[module(
    providers = [SqlxDatabaseService],
    exports = [SqlxDatabaseService],
)]
pub struct SqlxDatabaseModule;

impl SqlxDatabaseModule {
    pub fn for_root(database_url: impl Into<String>) -> Self {
        let _ = SQLX_URL.set(database_url.into());
        Self
    }
}
