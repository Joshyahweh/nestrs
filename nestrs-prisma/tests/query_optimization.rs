// Requires `SqlxDb = sqlx::Sqlite` — see `prisma_model_client.rs` / CI for `--all-features`.
#![cfg(all(
    feature = "sqlx",
    not(feature = "sqlx-postgres"),
    not(feature = "sqlx-mysql"),
))]

use std::sync::Arc;

use nestrs_prisma::query_optimization::{
    infer_query_shape, with_query_attribution, QueryAttribution, QueryOptimizationOptions,
};
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};

#[derive(Debug, sqlx::FromRow)]
struct ItemRow {
    id: i64,
    score: i64,
}

#[tokio::test]
async fn optimized_query_reports_and_explain_work() {
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite:file:query_opt?mode=memory&cache=shared")
            .pool_min(1)
            .pool_max(1),
    );
    let prisma = Arc::new(PrismaService::default());

    prisma
        .execute(
            r#"CREATE TABLE "items" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "score" INTEGER NOT NULL
            )"#,
        )
        .await
        .expect("create table");

    let shape = infer_query_shape(r#"INSERT INTO "items" ("score") VALUES (123)"#);
    assert!(shape.contains('?'));
    let comment_sql = with_query_attribution(
        r#"SELECT * FROM "items""#,
        &QueryAttribution::new("Item", "findMany", "SELECT * FROM items WHERE score = ?"),
    );
    assert!(comment_sql.contains("prisma:model=Item"));
    assert!(comment_sql.contains("action=findMany"));

    let insert_report = prisma
        .execute_optimized(
            r#"INSERT INTO "items" ("score") VALUES (10), (20), (30)"#,
            QueryOptimizationOptions {
                attribution: Some(QueryAttribution::new("Item", "createMany", shape)),
                slow_query_threshold_ms: 0,
                explain_on_slow: false,
            },
        )
        .await
        .expect("execute optimized");
    assert_eq!(insert_report.rows_affected, Some(3));
    assert!(insert_report.slow);
    assert!(insert_report.attributed_sql.contains("prisma:model=Item"));

    let (rows, select_report) = prisma
        .query_all_as_optimized::<ItemRow>(
            r#"SELECT "id", "score" FROM "items" ORDER BY "id" ASC"#,
            QueryOptimizationOptions {
                attribution: Some(QueryAttribution::new(
                    "Item",
                    "findMany",
                    "SELECT id, score FROM items ORDER BY id ASC",
                )),
                slow_query_threshold_ms: 0,
                explain_on_slow: true,
            },
        )
        .await
        .expect("query optimized");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].score, 10);
    assert!(rows[0].id >= 1);
    assert_eq!(select_report.row_count, Some(3));
    assert!(select_report.explain_plan.as_ref().is_some());
    assert!(!select_report
        .explain_plan
        .as_ref()
        .expect("plan")
        .is_empty());
}
