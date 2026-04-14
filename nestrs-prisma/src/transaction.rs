//! Prisma-style transaction and batch helpers on top of SQLx `Any`.
//!
//! Supports:
//! - Sequential transactional batches (`$transaction([])`-like)
//! - Interactive transactions (`$transaction(async tx => ...)`-like)
//! - Transaction options (`isolation_level`, `max_wait_ms`, `timeout_ms`)
//! - Best-effort retry helper for serialization/deadlock conflicts

/// Transaction isolation levels aligned with Prisma naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionIsolationLevel {
    ReadUncommitted,
    ReadCommitted,
    RepeatableRead,
    Snapshot,
    Serializable,
}

impl TransactionIsolationLevel {
    fn as_sql(self) -> &'static str {
        match self {
            TransactionIsolationLevel::ReadUncommitted => "READ UNCOMMITTED",
            TransactionIsolationLevel::ReadCommitted => "READ COMMITTED",
            TransactionIsolationLevel::RepeatableRead => "REPEATABLE READ",
            TransactionIsolationLevel::Snapshot => "SNAPSHOT",
            TransactionIsolationLevel::Serializable => "SERIALIZABLE",
        }
    }
}

/// Transaction execution options mirroring Prisma concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionOptions {
    /// Isolation level override. If `None`, DB default is used.
    pub isolation_level: Option<TransactionIsolationLevel>,
    /// Max wait time to acquire a transaction connection (ms). Prisma default: 2000.
    pub max_wait_ms: u64,
    /// Max transaction runtime (ms). Prisma default: 5000.
    pub timeout_ms: u64,
}

impl Default for TransactionOptions {
    fn default() -> Self {
        Self {
            isolation_level: None,
            max_wait_ms: 2000,
            timeout_ms: 5000,
        }
    }
}

#[cfg(feature = "sqlx")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DbProvider {
    PostgreSql,
    MySql,
    SqlServer,
    Sqlite,
    CockroachDb,
    Unknown,
}

#[cfg(feature = "sqlx")]
fn detect_provider(database_url: &str) -> DbProvider {
    let lower = database_url.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        if lower.contains("cockroach") || lower.contains("cockroachdb") {
            DbProvider::CockroachDb
        } else {
            DbProvider::PostgreSql
        }
    } else if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
        DbProvider::MySql
    } else if lower.starts_with("sqlserver://") || lower.starts_with("mssql://") {
        DbProvider::SqlServer
    } else if lower.starts_with("sqlite:") || lower.starts_with("file:") {
        DbProvider::Sqlite
    } else {
        DbProvider::Unknown
    }
}

#[cfg(feature = "sqlx")]
fn validate_isolation(provider: DbProvider, iso: TransactionIsolationLevel) -> Result<(), String> {
    use TransactionIsolationLevel::*;
    match provider {
        DbProvider::Sqlite | DbProvider::CockroachDb => {
            if iso != Serializable {
                return Err(format!(
                    "isolation level {iso:?} is not supported by this provider; use Serializable"
                ));
            }
        }
        DbProvider::PostgreSql | DbProvider::MySql => {
            if iso == Snapshot {
                return Err("Snapshot isolation is not supported by this provider".to_string());
            }
        }
        DbProvider::Unknown | DbProvider::SqlServer => {}
    }
    Ok(())
}

#[cfg(feature = "sqlx")]
fn retryable_conflict_error(msg: &str) -> bool {
    // Best-effort matching for serialization/deadlock conflicts across DBs.
    let m = msg.to_ascii_lowercase();
    m.contains("serialization")
        || m.contains("deadlock")
        || m.contains("40p01") // PG deadlock
        || m.contains("40001") // PG/MySQL serialization
        || m.contains("1213") // MySQL deadlock
        || m.contains("1205") // SQL Server deadlock victim
}

#[cfg(feature = "sqlx")]
async fn apply_isolation(
    tx: &mut sqlx::Transaction<'static, sqlx::Any>,
    provider: DbProvider,
    iso: TransactionIsolationLevel,
) -> Result<(), String> {
    validate_isolation(provider, iso)?;

    // SQLite/Cockroach are effectively serializable-only in Prisma docs context.
    if matches!(provider, DbProvider::Sqlite | DbProvider::CockroachDb)
        && iso == TransactionIsolationLevel::Serializable
    {
        return Ok(());
    }

    let stmt = format!("SET TRANSACTION ISOLATION LEVEL {}", iso.as_sql());
    sqlx::query(&stmt)
        .execute(tx.as_mut())
        .await
        .map_err(|e| format!("sqlx set isolation: {e}"))?;
    Ok(())
}

#[cfg(feature = "sqlx")]
pub struct PrismaTransaction {
    tx: sqlx::Transaction<'static, sqlx::Any>,
}

#[cfg(feature = "sqlx")]
impl PrismaTransaction {
    pub async fn execute(&mut self, sql: &str) -> Result<u64, String> {
        sqlx::query(sql)
            .execute(self.tx.as_mut())
            .await
            .map_err(|e| format!("sqlx execute: {e}"))
            .map(|r| r.rows_affected())
    }

    pub async fn query_scalar(&mut self, sql: &str) -> Result<String, String> {
        let v: i64 = sqlx::query_scalar(sql)
            .fetch_one(self.tx.as_mut())
            .await
            .map_err(|e| format!("sqlx query: {e}"))?;
        Ok(v.to_string())
    }

    pub async fn query_all_as<T>(&mut self, sql: &str) -> Result<Vec<T>, String>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        sqlx::query_as::<_, T>(sql)
            .fetch_all(self.tx.as_mut())
            .await
            .map_err(|e| format!("sqlx query: {e}"))
    }

    pub async fn commit(self) -> Result<(), String> {
        self.tx
            .commit()
            .await
            .map_err(|e| format!("sqlx commit: {e}"))
    }

    pub async fn rollback(self) -> Result<(), String> {
        self.tx
            .rollback()
            .await
            .map_err(|e| format!("sqlx rollback: {e}"))
    }
}

#[cfg(feature = "sqlx")]
impl crate::PrismaService {
    /// Starts an interactive transaction handle (`begin -> queries -> commit/rollback`).
    pub async fn begin_transaction(
        &self,
        opts: TransactionOptions,
    ) -> Result<PrismaTransaction, String> {
        use std::time::Duration;
        let pool = crate::sqlx_pool().await.map_err(|e| e.to_string())?;

        let begin_fut = pool.begin();
        let mut tx = tokio::time::timeout(Duration::from_millis(opts.max_wait_ms), begin_fut)
            .await
            .map_err(|_| format!("transaction begin timed out after {}ms", opts.max_wait_ms))?
            .map_err(|e| format!("sqlx begin transaction: {e}"))?;

        if let Some(iso) = opts.isolation_level {
            apply_isolation(&mut tx, detect_provider(&self.client().database_url), iso).await?;
        }

        Ok(PrismaTransaction { tx })
    }

    /// Sequential `$transaction([])`-style execution for independent writes.
    pub async fn transaction_execute_batch(
        &self,
        statements: &[&str],
        opts: TransactionOptions,
    ) -> Result<Vec<u64>, String> {
        use std::time::Duration;
        let mut tx = self.begin_transaction(opts).await?;

        let work = async {
            let mut out = Vec::with_capacity(statements.len());
            for stmt in statements {
                out.push(tx.execute(stmt).await?);
            }
            tx.commit().await?;
            Ok::<Vec<u64>, String>(out)
        };

        tokio::time::timeout(Duration::from_millis(opts.timeout_ms), work)
            .await
            .map_err(|_| format!("transaction timed out after {}ms", opts.timeout_ms))?
    }

    /// Interactive transaction for read-modify-write logic.
    pub async fn transaction_interactive<T, F>(
        &self,
        opts: TransactionOptions,
        f: F,
    ) -> Result<T, String>
    where
        F: for<'a> FnOnce(
            &'a mut PrismaTransaction,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T, String>> + Send + 'a>,
        >,
    {
        use std::time::Duration;
        let mut tx = self.begin_transaction(opts).await?;

        let run = tokio::time::timeout(Duration::from_millis(opts.timeout_ms), f(&mut tx)).await;
        let result = match run {
            Ok(r) => r,
            Err(_) => {
                let _ = tx.rollback().await;
                return Err(format!("transaction timed out after {}ms", opts.timeout_ms));
            }
        };

        match result {
            Ok(v) => {
                tx.commit().await?;
                Ok(v)
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    }

    /// Retry wrapper for `$transaction([])`-style batch writes when conflicts occur.
    pub async fn transaction_execute_batch_with_retry(
        &self,
        statements: &[&str],
        opts: TransactionOptions,
        max_retries: usize,
    ) -> Result<Vec<u64>, String> {
        let mut retries = 0usize;
        loop {
            match self.transaction_execute_batch(statements, opts).await {
                Ok(v) => return Ok(v),
                Err(e) if retries < max_retries && retryable_conflict_error(&e) => {
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
