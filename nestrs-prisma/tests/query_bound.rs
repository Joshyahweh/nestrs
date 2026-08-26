#![cfg(all(
    feature = "sqlx",
    not(feature = "sqlx-postgres"),
    not(feature = "sqlx-mysql"),
))]

use std::sync::Arc;

use nestrs_prisma::{prisma_execute, prisma_query_rows, prisma_query_scalar, PrismaModule, PrismaOptions, PrismaService};
use sqlx::FromRow;

#[derive(Debug, FromRow)]
struct Account {
    id: i64,
    email: String,
    balance: i64,
}

#[tokio::test]
async fn bound_macros_parameterize_against_injection() {
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite:file:query_bound?mode=memory&cache=shared")
            .pool_min(1)
            .pool_max(1),
    );
    let prisma = Arc::new(PrismaService::default());

    prisma
        .execute(
            r#"CREATE TABLE "accounts" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "email" TEXT NOT NULL UNIQUE,
                "balance" INTEGER NOT NULL
            )"#,
        )
        .await
        .expect("create accounts table");

    // 1. prisma_execute! with binds — including a value that *looks* like SQL.
    //    (Macros await internally and yield a plain Result — no extra `.await`.)
    let evil = "x@evil.io', 999); DROP TABLE accounts;--";
    let inserted = prisma_execute!(
        prisma,
        r#"INSERT INTO "accounts" ("email", "balance") VALUES (?, ?)"#,
        evil,
        42_i64,
    )
    .expect("insert with binds");
    assert_eq!(inserted, 1);

    // 2. The payload was stored as literal data and the table survived.
    let total = prisma_query_scalar!(prisma, r#"SELECT COUNT(*) FROM "accounts""#)
        .expect("table still exists");
    assert_eq!(total, 1);

    // 3. prisma_query_rows! maps rows through FromRow with bound filters.
    let found: Vec<Account> = prisma_query_rows!(
        prisma,
        Account,
        r#"SELECT "id", "email", "balance" FROM "accounts" WHERE "email" = ? AND "balance" = ?"#,
        evil,
        42_i64,
    )
    .expect("select with binds");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, 1);
    assert_eq!(found[0].email, evil);
    assert_eq!(found[0].balance, 42);

    // 4. Zero-bind form works too (macro arity without args).
    let scalar = prisma_query_scalar!(prisma, r#"SELECT COUNT(*) FROM "accounts""#);
    assert!(scalar.is_ok());

    // 5. Direct pool access supports hand-chained binds for advanced cases.
    let pool = prisma.pool().await.expect("pool");
    let n: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "accounts" WHERE "email" = ?"#)
        .bind(evil)
        .fetch_one(pool)
        .await
        .expect("pool-level bound query");
    assert_eq!(n, 1);
}
