use async_trait::async_trait;
use std::sync::Arc;
use std::sync::OnceLock;

use nestrs::core::DatabasePing;
use nestrs::prelude::*;

pub mod client;
pub mod deployment;
pub mod error;
pub mod index_ddl;
mod macros;
mod macros_enum;
mod macros_index;
mod macros_relation;
mod macros_where;
pub mod mapping;
pub mod query_optimization;
pub mod relation_queries;
pub mod relations;
pub mod schema_bridge;
pub mod transaction;

#[doc(hidden)]
pub use paste;

#[cfg(feature = "sqlx")]
#[doc(hidden)]
pub use sqlx;

#[cfg(feature = "sqlx")]
pub use client::ModelRepository;
pub use client::SortOrder;
pub use error::PrismaError;

#[cfg(feature = "sqlx")]
use tokio::sync::OnceCell;

/// Recommended default location for a Prisma schema in nestrs apps.
pub const DEFAULT_SCHEMA_PATH: &str = "prisma/schema.prisma";
/// Recommended default location for Prisma SQL migrations.
pub const DEFAULT_MIGRATIONS_PATH: &str = "prisma/migrations";

/// Builds the documented Rust Prisma client generation command.
///
/// This mirrors the expected `prisma-client-rust-cli` workflow while keeping
/// command construction explicit for docs/tools.
pub fn prisma_generate_command(schema_path: &str) -> String {
    format!("cargo prisma generate --schema {schema_path}")
}

/// Builds the documented Prisma migrate deploy command.
pub fn prisma_migrate_deploy_command() -> &'static str {
    "npx prisma migrate deploy"
}

/// Builds the documented Prisma db push command (MongoDB deployments).
pub fn prisma_db_push_command() -> &'static str {
    "npx prisma db push"
}

#[derive(Debug, Clone)]
pub struct PrismaOptions {
    pub database_url: String,
    pub pool_min: u32,
    pub pool_max: u32,
    pub schema_path: String,
}

impl PrismaOptions {
    pub fn from_url(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            pool_min: 2,
            pool_max: 20,
            schema_path: DEFAULT_SCHEMA_PATH.to_string(),
        }
    }

    pub fn pool_min(mut self, value: u32) -> Self {
        self.pool_min = value;
        self
    }

    pub fn pool_max(mut self, value: u32) -> Self {
        self.pool_max = value;
        self
    }

    pub fn schema_path(mut self, value: impl Into<String>) -> Self {
        self.schema_path = value.into();
        self
    }
}

static PRISMA_OPTIONS: OnceLock<PrismaOptions> = OnceLock::new();

#[cfg(feature = "sqlx")]
static SQLX_POOL: OnceCell<sqlx::AnyPool> = OnceCell::const_new();

#[cfg(feature = "sqlx")]
static SQLX_ANY_DRIVERS: OnceLock<()> = OnceLock::new();

/// Shared SQLx pool for [`PrismaService`] and generated [`ModelRepository`](client::ModelRepository) access.
#[cfg(feature = "sqlx")]
pub async fn sqlx_pool() -> Result<&'static sqlx::AnyPool, PrismaError> {
    ensure_sqlx_pool().await.map_err(PrismaError::PoolInit)
}

#[cfg(feature = "sqlx")]
async fn ensure_sqlx_pool() -> Result<&'static sqlx::AnyPool, String> {
    let _ = SQLX_ANY_DRIVERS.get_or_init(|| {
        sqlx::any::install_default_drivers();
    });
    SQLX_POOL
        .get_or_try_init(|| async {
            let opts = PRISMA_OPTIONS.get().cloned().ok_or_else(|| {
                "PrismaModule::for_root / for_root_with_options must be called before SQL connectivity"
                    .to_string()
            })?;
            sqlx::any::AnyPoolOptions::new()
                .max_connections(opts.pool_max)
                .min_connections(opts.pool_min)
                .connect(&opts.database_url)
                .await
                .map_err(|e| format!("sqlx connect: {e}"))
        })
        .await
}

#[derive(Debug, Clone)]
pub struct PrismaClientHandle {
    pub database_url: String,
    pub schema_path: String,
}

/// Injectable database service: configuration + optional **SQLx** pool when the `sqlx` feature is on.
///
/// For full Prisma Client Rust codegen, run `cargo prisma generate` and inject the generated client
/// as an additional provider; this crate stays ORM-agnostic while giving production-ready connectivity.
pub struct PrismaService {
    options: PrismaOptions,
    client: PrismaClientHandle,
}

impl PrismaService {
    pub fn client(&self) -> &PrismaClientHandle {
        &self.client
    }

    pub fn options(&self) -> &PrismaOptions {
        &self.options
    }

    /// Lightweight status without hitting the network (always `"ok"` if the service was constructed).
    pub fn health(&self) -> &'static str {
        "ok"
    }

    /// Run arbitrary SQL returning a single scalar (trusted SQL only — use parameters in app code).
    #[cfg(feature = "sqlx")]
    pub async fn query_scalar(&self, sql: &str) -> Result<String, String> {
        let pool = ensure_sqlx_pool().await?;
        let v: i64 = sqlx::query_scalar(sql)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("sqlx query: {e}"))?;
        Ok(v.to_string())
    }

    /// Run arbitrary SQL and map rows with [`sqlx::FromRow`] (trusted SQL only — use bound parameters in app code).
    #[cfg(feature = "sqlx")]
    pub async fn query_all_as<T>(&self, sql: &str) -> Result<Vec<T>, String>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        let pool = ensure_sqlx_pool().await?;
        sqlx::query_as::<_, T>(sql)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("sqlx query: {e}"))
    }

    /// Execute DDL/DML without returning rows (migrations, `CREATE TABLE`, etc.).
    #[cfg(feature = "sqlx")]
    pub async fn execute(&self, sql: &str) -> Result<u64, String> {
        let pool = ensure_sqlx_pool().await?;
        sqlx::query(sql)
            .execute(pool)
            .await
            .map_err(|e| format!("sqlx execute: {e}"))
            .map(|r| r.rows_affected())
    }

    /// `SELECT 1` / connectivity check against [`DATABASE_URL`](PrismaOptions::database_url).
    #[cfg(feature = "sqlx")]
    pub async fn ping(&self) -> Result<(), String> {
        let pool = ensure_sqlx_pool().await?;
        sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .map_err(|e| format!("sqlx ping: {e}"))?;
        Ok(())
    }

    /// Stub string when **`sqlx`** is disabled; enable **`sqlx`** for real execution.
    #[cfg(not(feature = "sqlx"))]
    pub fn query_raw(&self, sql: &str) -> String {
        format!("query accepted by prisma stub (enable nestrs-prisma/sqlx): {sql}")
    }

    pub fn mapping_guidance(&self) -> &'static str {
        "Prefer `From<ModelData>` / `TryFrom<ModelData>` impls for response DTOs; avoid returning generated Prisma model types directly from controllers."
    }
}

#[async_trait]
impl DatabasePing for PrismaService {
    async fn ping_database(&self) -> Result<(), String> {
        #[cfg(feature = "sqlx")]
        {
            self.ping().await
        }
        #[cfg(not(feature = "sqlx"))]
        {
            Ok(())
        }
    }
}

impl Default for PrismaService {
    fn default() -> Self {
        let options = PRISMA_OPTIONS
            .get()
            .cloned()
            .or_else(|| {
                std::env::var("DATABASE_URL")
                    .ok()
                    .map(PrismaOptions::from_url)
            })
            .unwrap_or_else(|| PrismaOptions::from_url("file:./dev.db"));

        let client = PrismaClientHandle {
            database_url: options.database_url.clone(),
            schema_path: options.schema_path.clone(),
        };

        Self { options, client }
    }
}

impl Injectable for PrismaService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[module(
    providers = [PrismaService],
    exports = [PrismaService],
)]
pub struct PrismaModule;

impl PrismaModule {
    pub fn for_root(database_url: impl Into<String>) -> Self {
        let _ = PRISMA_OPTIONS.set(PrismaOptions::from_url(database_url));
        Self
    }

    pub fn for_root_with_options(options: PrismaOptions) -> Self {
        let _ = PRISMA_OPTIONS.set(options);
        Self
    }

    pub fn generate_command_hint() -> String {
        let schema_path = PRISMA_OPTIONS
            .get()
            .map(|o| o.schema_path.as_str())
            .unwrap_or(DEFAULT_SCHEMA_PATH);
        prisma_generate_command(schema_path)
    }

    pub fn deploy_command_hint() -> &'static str {
        prisma_migrate_deploy_command()
    }
}
