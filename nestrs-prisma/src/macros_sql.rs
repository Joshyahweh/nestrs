//! Bound-parameter SQL macros — the safe alternative to string-interpolated SQL.
//!
//! Every placeholder argument is passed through [`crate::sqlx`] QueryBuilder
//! `.bind(...)`, so values are sent out-of-band and can never alter query
//! structure. Placeholders follow the backend dialect (`?` for SQLite/MySQL,
//! `$1..$N` for Postgres).

/// Run parameterized SQL and map rows to `$T: FromRow`.
///
/// ```ignore
/// let admins: Vec<User> = nestrs_prisma::prisma_query_rows!(
///     prisma,
///     User,
///     r#"SELECT * FROM "users" WHERE "role" = ? AND "active" = ?"#,
///     role,          // bound — never interpolated
///     true,
/// )?;
/// ```
#[macro_export]
macro_rules! prisma_query_rows {
    ($svc:expr, $T:ty, $sql:expr $(, $arg:expr)* $(,)?) => {{
        // Evaluate once, borrow — never moves the caller's service handle.
        let __svc = &$svc;
        let _ = __svc;
        match $crate::sqlx_pool().await {
            ::std::result::Result::Ok(pool) => {
                let mut __q = $crate::sqlx::query_as::<_, $T>($sql);
                $(
                    __q = __q.bind($arg);
                )*
                __q.fetch_all(pool)
                    .await
                    .map_err($crate::PrismaError::from_sqlx)
            }
            ::std::result::Result::Err(e) => ::std::result::Result::Err(e),
        }
    }};
}

/// Run parameterized SQL returning a single `i64` scalar (counts, sums, …).
///
/// ```ignore
/// let n = nestrs_prisma::prisma_query_scalar!(
///     prisma,
///     r#"SELECT COUNT(*) FROM "users" WHERE "role" = ?"#,
///     role,
/// )?;
/// ```
#[macro_export]
macro_rules! prisma_query_scalar {
    ($svc:expr, $sql:expr $(, $arg:expr)* $(,)?) => {{
        // Evaluate once, borrow — never moves the caller's service handle.
        let __svc = &$svc;
        let _ = __svc;
        match $crate::sqlx_pool().await {
            ::std::result::Result::Ok(pool) => {
                let mut __q = $crate::sqlx::query_scalar::<_, i64>($sql);
                $(
                    __q = __q.bind($arg);
                )*
                __q.fetch_one(pool)
                    .await
                    .map_err($crate::PrismaError::from_sqlx)
            }
            ::std::result::Result::Err(e) => ::std::result::Result::Err(e),
        }
    }};
}

/// Execute parameterized DDL/DML, returning affected row count.
///
/// ```ignore
/// nestrs_prisma::prisma_execute!(
///     prisma,
///     r#"UPDATE "accounts" SET "balance" = "balance" - ? WHERE "email" = ?"#,
///     amount,
///     email,
/// )?;
/// ```
#[macro_export]
macro_rules! prisma_execute {
    ($svc:expr, $sql:expr $(, $arg:expr)* $(,)?) => {{
        // Evaluate once, borrow — never moves the caller's service handle.
        let __svc = &$svc;
        let _ = __svc;
        match $crate::sqlx_pool().await {
            ::std::result::Result::Ok(pool) => {
                let mut __q = $crate::sqlx::query($sql);
                $(
                    __q = __q.bind($arg);
                )*
                __q.execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err($crate::PrismaError::from_sqlx)
            }
            ::std::result::Result::Err(e) => ::std::result::Result::Err(e),
        }
    }};
}
