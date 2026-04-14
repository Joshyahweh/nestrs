#![cfg(feature = "sqlx")]

use std::sync::Arc;

use nestrs_prisma::transaction::{TransactionIsolationLevel, TransactionOptions};
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};

#[tokio::test]
async fn transaction_batch_and_interactive_sqlite_memory() {
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite:file:tx_batch?mode=memory&cache=shared")
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

    let batch = [
        r#"INSERT INTO "accounts" ("email", "balance") VALUES ('alice@prisma.io', 100)"#,
        r#"INSERT INTO "accounts" ("email", "balance") VALUES ('bob@prisma.io', 100)"#,
        r#"UPDATE "accounts" SET "balance" = "balance" + 10 WHERE "email" = 'alice@prisma.io'"#,
    ];
    let counts = prisma
        .transaction_execute_batch(&batch, TransactionOptions::default())
        .await
        .expect("transaction batch");
    assert_eq!(counts, vec![1, 1, 1]);

    let total = prisma
        .query_scalar(r#"SELECT COUNT(*) FROM "accounts""#)
        .await
        .expect("count");
    assert_eq!(total, "2");

    let res: Result<(), String> = prisma
        .transaction_interactive(TransactionOptions::default(), |tx| {
            Box::pin(async move {
                tx.execute(
                    r#"UPDATE "accounts" SET "balance" = "balance" - 20 WHERE "email" = 'alice@prisma.io'"#,
                )
                .await?;
                Err("force rollback".to_string())
            })
        })
        .await;
    assert!(res.is_err());

    let alice_after_rollback = prisma
        .query_scalar(r#"SELECT "balance" FROM "accounts" WHERE "email" = 'alice@prisma.io'"#)
        .await
        .expect("alice balance");
    assert_eq!(alice_after_rollback, "110");

    let transfer_result = prisma
        .transaction_interactive(
            TransactionOptions {
                isolation_level: Some(TransactionIsolationLevel::Serializable),
                ..TransactionOptions::default()
            },
            |tx| {
                Box::pin(async move {
                    tx.execute(
                        r#"UPDATE "accounts" SET "balance" = "balance" - 100 WHERE "email" = 'alice@prisma.io'"#,
                    )
                    .await?;
                    let sender_balance: i64 = tx
                        .query_scalar(r#"SELECT "balance" FROM "accounts" WHERE "email" = 'alice@prisma.io'"#)
                        .await?
                        .parse()
                        .map_err(|e| format!("parse sender balance: {e}"))?;
                    if sender_balance < 0 {
                        return Err("insufficient funds".to_string());
                    }
                    tx.execute(
                        r#"UPDATE "accounts" SET "balance" = "balance" + 100 WHERE "email" = 'bob@prisma.io'"#,
                    )
                    .await?;
                    Ok(())
                })
            },
        )
        .await;
    assert!(transfer_result.is_ok());

    let alice_final = prisma
        .query_scalar(r#"SELECT "balance" FROM "accounts" WHERE "email" = 'alice@prisma.io'"#)
        .await
        .expect("alice final");
    let bob_final = prisma
        .query_scalar(r#"SELECT "balance" FROM "accounts" WHERE "email" = 'bob@prisma.io'"#)
        .await
        .expect("bob final");
    assert_eq!(alice_final, "10");
    assert_eq!(bob_final, "200");

    let result = prisma
        .begin_transaction(TransactionOptions {
            isolation_level: Some(TransactionIsolationLevel::ReadCommitted),
            ..TransactionOptions::default()
        })
        .await;
    assert!(result.is_err());
}
