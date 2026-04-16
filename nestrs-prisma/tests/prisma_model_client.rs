// SQLite URLs require `SqlxDb = sqlx::Sqlite`. With `sqlx-postgres` / `sqlx-mysql` enabled
// (e.g. `cargo test --all-features`), `SqlxDb` is Postgres/MySQL instead, so this test is
// built only when those backend markers are off. CI runs it explicitly with
// `--features sqlx,sqlx-sqlite`.
#![cfg(all(
    feature = "sqlx",
    not(feature = "sqlx-postgres"),
    not(feature = "sqlx-mysql"),
))]

use std::sync::Arc;

use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};

nestrs_prisma::prisma_model!(User => "users", {
    id: i32,
    email: String,
    name: String,
});

#[tokio::test]
async fn prisma_model_crud_sqlite_memory() {
    // Single-connection in-memory pool so DDL + DML share one SQLite database file.
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite::memory:")
            .pool_min(1)
            .pool_max(1),
    );
    let prisma = Arc::new(PrismaService::default());

    prisma
        .execute(
            r#"CREATE TABLE "users" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "email" TEXT NOT NULL UNIQUE,
                "name" TEXT NOT NULL
            )"#,
        )
        .await
        .expect("create table");

    let created = prisma
        .user()
        .create(UserCreateInput {
            email: "a@example.com".into(),
            name: "Alice".into(),
        })
        .await
        .expect("create");

    assert_eq!(created.email, "a@example.com");
    assert_eq!(created.name, "Alice");
    assert!(created.id >= 1);

    let batch_n = prisma
        .user()
        .create_many(vec![
            UserCreateInput {
                email: "b1@example.com".into(),
                name: "Batch1".into(),
            },
            UserCreateInput {
                email: "b2@example.com".into(),
                name: "Batch2".into(),
            },
        ])
        .await
        .expect("create_many");
    assert_eq!(batch_n, 2);

    let skipped = prisma
        .user()
        .create_many_with_options(
            vec![
                UserCreateInput {
                    email: "b1@example.com".into(),
                    name: "DuplicateShouldSkip".into(),
                },
                UserCreateInput {
                    email: "c1@example.com".into(),
                    name: "NewRow".into(),
                },
            ],
            UserCreateManyOptions {
                skip_duplicates: true,
            },
        )
        .await
        .expect("create_many_with_options");
    assert_eq!(skipped, 1);

    let returned = prisma
        .user()
        .create_many_and_return(
            vec![
                UserCreateInput {
                    email: "d1@example.com".into(),
                    name: "D1".into(),
                },
                UserCreateInput {
                    email: "d2@example.com".into(),
                    name: "D2".into(),
                },
            ],
            UserCreateManyOptions::default(),
        )
        .await
        .expect("create_many_and_return");
    assert_eq!(returned.len(), 2);

    let one = prisma
        .user()
        .find_unique(user::id::equals(created.id))
        .await
        .expect("find_unique")
        .expect("row");
    assert_eq!(one.name, "Alice");

    let cnt = prisma
        .user()
        .count(UserWhere::and(vec![user::email::equals(
            "a@example.com".into(),
        )]))
        .await
        .expect("count");
    assert_eq!(cnt, 1);

    let updated = prisma
        .user()
        .update(user::id::equals(created.id), user::name::set("Bob".into()))
        .await
        .expect("update");
    assert_eq!(updated.name, "Bob");

    let n = prisma
        .user()
        .update_many(
            user::id::equals(created.id),
            UserUpdate {
                name: Some("Carol".into()),
                ..Default::default()
            },
        )
        .await
        .expect("update_many");
    assert_eq!(n, 1);

    let updated_rows = prisma
        .user()
        .update_many_and_return(
            UserWhere::or(vec![
                user::email::equals("b1@example.com".into()),
                user::email::equals("b2@example.com".into()),
            ]),
            UserUpdate {
                name: Some("BatchUpdated".into()),
                ..Default::default()
            },
        )
        .await
        .expect("update_many_and_return");
    assert_eq!(updated_rows.len(), 2);
    assert!(updated_rows.iter().all(|r| r.name == "BatchUpdated"));

    let many = prisma
        .user()
        .find_many(UserWhere::and(vec![]))
        .await
        .expect("find_many");
    assert_eq!(many.len(), 6);

    let distinct_names = prisma
        .user()
        .find_many_with_options(UserFindManyOptions {
            r#where: UserWhere::and(vec![]),
            order_by: None,
            take: None,
            skip: None,
            distinct: Some(vec![UserScalarField::Name]),
        })
        .await
        .expect("find_many distinct");
    assert!(distinct_names.len() < many.len());

    let selected_counts = prisma
        .user()
        .count_selected(UserWhere::and(vec![]), true, vec![UserScalarField::Name])
        .await
        .expect("count_selected");
    assert_eq!(selected_counts.get("_all").copied(), Some(6));
    assert_eq!(selected_counts.get("name").copied(), Some(6));

    let agg = prisma
        .user()
        .aggregate(
            UserAggregateOptions {
                r#where: UserWhere::and(vec![]),
                order_by: None,
                take: None,
                skip: None,
            },
            UserAggregateSelection {
                count_all: true,
                count: vec![UserScalarField::Id],
                avg: vec![UserScalarField::Id],
                sum: vec![UserScalarField::Id],
                min: vec![UserScalarField::Id],
                max: vec![UserScalarField::Id],
            },
        )
        .await
        .expect("aggregate");
    assert_eq!(agg.count.get("_all").copied(), Some(6));
    assert_eq!(agg.count.get("id").copied(), Some(6));
    assert!(agg.avg.get("id").copied().flatten().is_some());
    assert!(agg.sum.get("id").copied().flatten().is_some());
    assert!(agg.min.get("id").cloned().flatten().is_some());
    assert!(agg.max.get("id").cloned().flatten().is_some());

    let grouped = prisma
        .user()
        .group_by(
            UserGroupByOptions {
                by: vec![UserScalarField::Name],
                r#where: UserWhere::and(vec![]),
                order_by: None,
                take: None,
                skip: None,
                having: vec![UserHavingCondition {
                    field: UserScalarField::Id,
                    metric: UserAggregateMetric::Count,
                    op: UserHavingOp::Gt,
                    value: 1.0,
                }],
            },
            UserAggregateSelection {
                count_all: true,
                count: vec![UserScalarField::Id],
                avg: vec![],
                sum: vec![],
                min: vec![],
                max: vec![],
            },
        )
        .await
        .expect("group_by");
    assert!(grouped.iter().any(|g| {
        g.by.get("name")
            .map(|v| v.contains("BatchUpdated"))
            .unwrap_or(false)
    }));

    let first = prisma
        .user()
        .find_first(
            UserWhere::and(vec![]),
            Some(vec![user::id::order(nestrs_prisma::SortOrder::Desc)]),
        )
        .await
        .expect("find_first")
        .expect("row");
    assert!(first.id >= created.id);

    let upserted = prisma
        .user()
        .upsert(
            user::id::equals(999),
            UserCreateInput {
                email: "orphan@example.com".into(),
                name: "Orphan".into(),
            },
            user::name::set("Ignored".into()),
        )
        .await
        .expect("upsert insert");
    assert_eq!(upserted.email, "orphan@example.com");

    let _ = prisma
        .user()
        .upsert(
            user::id::equals(upserted.id),
            UserCreateInput {
                email: "x@y.z".into(),
                name: "Nope".into(),
            },
            user::name::set("UpdatedOrphan".into()),
        )
        .await
        .expect("upsert update");

    let deleted = prisma
        .user()
        .delete_many(UserWhere::or(vec![
            user::id::equals(created.id),
            user::id::equals(upserted.id),
        ]))
        .await
        .expect("delete_many");
    assert_eq!(deleted, 2);
}
