//! Query optimization helpers inspired by Prisma's optimization guidance.
//!
//! This module provides:
//! - Query attribution comments (model/action/query shape)
//! - Execution reports (duration, slow-query flag, row counts)
//! - Best-effort EXPLAIN helpers for debugging query plans
//! - `PrismaService` optimized wrappers for execute/query calls

use std::time::Instant;

/// Prisma Query Insights-like SQL attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryAttribution {
    pub model: String,
    pub action: String,
    /// Parameterized query shape (e.g. literals replaced with placeholders).
    pub query_shape: String,
}

impl QueryAttribution {
    pub fn new(
        model: impl Into<String>,
        action: impl Into<String>,
        query_shape: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            action: action.into(),
            query_shape: query_shape.into(),
        }
    }
}

/// Optimization behavior for a single query execution.
#[derive(Debug, Clone)]
pub struct QueryOptimizationOptions {
    pub attribution: Option<QueryAttribution>,
    /// Slow-query threshold in milliseconds.
    pub slow_query_threshold_ms: u128,
    /// Collect EXPLAIN plan automatically when query is slow.
    pub explain_on_slow: bool,
}

impl Default for QueryOptimizationOptions {
    fn default() -> Self {
        Self {
            attribution: None,
            slow_query_threshold_ms: 200,
            explain_on_slow: false,
        }
    }
}

/// Query execution metadata for diagnostics/observability.
#[derive(Debug, Clone, Default)]
pub struct QueryExecutionReport {
    pub original_sql: String,
    pub attributed_sql: String,
    pub elapsed_ms: u128,
    pub slow: bool,
    pub rows_affected: Option<u64>,
    pub row_count: Option<usize>,
    pub explain_plan: Option<Vec<String>>,
}

fn sanitize_for_sql_comment(value: &str) -> String {
    value.replace("/*", "/ *").replace("*/", "* /")
}

/// Adds Prisma-style attribution comment to the query text.
///
/// Output shape:
/// `/* prisma:model=User,action=findMany,shape=SELECT ... */ SELECT ...`
pub fn with_query_attribution(sql: &str, attribution: &QueryAttribution) -> String {
    let model = sanitize_for_sql_comment(&attribution.model);
    let action = sanitize_for_sql_comment(&attribution.action);
    let shape = sanitize_for_sql_comment(&attribution.query_shape);
    format!("/* prisma:model={model},action={action},shape={shape} */ {sql}")
}

/// Best-effort query shape normalization (replaces string and numeric literals with `?`).
pub fn infer_query_shape(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            out.push('?');
            i += 1;
            while i < chars.len() {
                let d = chars[i];
                if d == '\'' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        if c.is_ascii_digit() {
            out.push('?');
            i += 1;
            while i < chars.len() {
                let d = chars[i];
                if d.is_ascii_digit() || d == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }

        out.push(c);
        i += 1;
    }
    out
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
impl crate::PrismaService {
    /// Execute SQL and return diagnostics metadata suitable for query optimization tooling.
    pub async fn execute_optimized(
        &self,
        sql: &str,
        opts: QueryOptimizationOptions,
    ) -> Result<QueryExecutionReport, String> {
        let pool = crate::sqlx_pool().await.map_err(|e| e.to_string())?;
        let attributed_sql = if let Some(ref attr) = opts.attribution {
            with_query_attribution(sql, attr)
        } else {
            sql.to_string()
        };

        let started = Instant::now();
        let result = sqlx::query(&attributed_sql)
            .execute(pool)
            .await
            .map_err(|e| format!("sqlx execute: {e}"))?;
        let elapsed_ms = started.elapsed().as_millis();
        let slow = elapsed_ms >= opts.slow_query_threshold_ms;

        let explain_plan = if slow && opts.explain_on_slow {
            self.explain_query_plan(sql).await.ok()
        } else {
            None
        };

        Ok(QueryExecutionReport {
            original_sql: sql.to_string(),
            attributed_sql,
            elapsed_ms,
            slow,
            rows_affected: Some(result.rows_affected()),
            row_count: None,
            explain_plan,
        })
    }

    /// Query rows with diagnostics metadata (timing + optional slow-query EXPLAIN).
    pub async fn query_all_as_optimized<T>(
        &self,
        sql: &str,
        opts: QueryOptimizationOptions,
    ) -> Result<(Vec<T>, QueryExecutionReport), String>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        let pool = crate::sqlx_pool().await.map_err(|e| e.to_string())?;
        let attributed_sql = if let Some(ref attr) = opts.attribution {
            with_query_attribution(sql, attr)
        } else {
            sql.to_string()
        };

        let started = Instant::now();
        let rows = sqlx::query_as::<_, T>(&attributed_sql)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("sqlx query: {e}"))?;
        let elapsed_ms = started.elapsed().as_millis();
        let slow = elapsed_ms >= opts.slow_query_threshold_ms;

        let explain_plan = if slow && opts.explain_on_slow {
            self.explain_query_plan(sql).await.ok()
        } else {
            None
        };

        let report = QueryExecutionReport {
            original_sql: sql.to_string(),
            attributed_sql,
            elapsed_ms,
            slow,
            rows_affected: None,
            row_count: Some(rows.len()),
            explain_plan,
        };

        Ok((rows, report))
    }

    /// Returns database EXPLAIN output for a query.
    ///
    /// This is intentionally best-effort across providers:
    /// - PostgreSQL/CockroachDB: `EXPLAIN <sql>`
    /// - SQLite: `EXPLAIN QUERY PLAN <sql>`
    /// - MySQL: `EXPLAIN <sql>`
    /// - SQL Server: returns an error (not implemented in this helper)
    pub async fn explain_query_plan(&self, sql: &str) -> Result<Vec<String>, String> {
        use sqlx::Row;
        let provider = detect_provider(&self.client().database_url);
        let explain_sql = match provider {
            DbProvider::PostgreSql | DbProvider::CockroachDb => format!("EXPLAIN {sql}"),
            DbProvider::Sqlite => format!("EXPLAIN QUERY PLAN {sql}"),
            DbProvider::MySql => format!("EXPLAIN {sql}"),
            DbProvider::SqlServer => {
                return Err(
                    "SQL Server EXPLAIN/SHOWPLAN is not implemented in this helper".to_string(),
                )
            }
            DbProvider::Unknown => format!("EXPLAIN {sql}"),
        };

        let pool = crate::sqlx_pool().await.map_err(|e| e.to_string())?;
        let rows = sqlx::query(&explain_sql)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("sqlx explain: {e}"))?;

        let mut out = Vec::new();
        for row in rows {
            if let Ok(detail) = row.try_get::<String, _>("detail") {
                out.push(detail);
                continue;
            }
            let mut parts = Vec::new();
            for idx in 0..row.len() {
                if let Ok(v) = row.try_get::<String, _>(idx) {
                    parts.push(v);
                    continue;
                }
                if let Ok(v) = row.try_get::<i64, _>(idx) {
                    parts.push(v.to_string());
                    continue;
                }
                if let Ok(v) = row.try_get::<f64, _>(idx) {
                    parts.push(v.to_string());
                }
            }
            if !parts.is_empty() {
                out.push(parts.join(" | "));
            }
        }
        Ok(out)
    }
}
