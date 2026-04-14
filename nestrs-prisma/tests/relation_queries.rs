#![cfg(feature = "sqlx")]

use std::sync::Arc;
use std::sync::OnceLock;

use nestrs_prisma::relation_queries::{
    ForeignKeyMutationSpec, IncludeOptions, JoinMutationSpec, ManyToManyIncludeSpec,
    OneToManyIncludeSpec, OneToOneIncludeSpec, RelationIdValue,
};
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService, SortOrder};

static TEST_GUARD: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn test_guard() -> &'static tokio::sync::Mutex<()> {
    TEST_GUARD.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Debug, sqlx::FromRow)]
struct PostRow {
    id: i64,
    title: String,
    author_id: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ProfileRow {
    id: i64,
    user_id: i64,
    bio: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct CategoryRow {
    id: i64,
    name: String,
}

nestrs_prisma::prisma_model!(User => "users", {
    id: i64,
    email: String,
});
nestrs_prisma::prisma_model!(Post => "posts", {
    id: i64,
    title: String,
    author_id: i64,
});
nestrs_prisma::prisma_model!(Profile => "profiles", {
    id: i64,
    user_id: Option<i64>,
    bio: Option<String>,
});
nestrs_prisma::prisma_model!(Category => "categories", {
    id: i64,
    name: String,
});

nestrs_prisma::prisma_model_relations!(User {
    (one_to_many posts: Post { child_table: "posts", child_fk: "author_id" })
    (one_to_one profile: Profile { table: "profiles", fk: "user_id" })
});

nestrs_prisma::prisma_model_relations!(Post {
    (many_to_many categories: Category {
        related_table: "categories",
        related_pk: "id",
        join_table: "post_categories",
        join_left: "post_id",
        join_right: "category_id"
    })
    (join_mutation category_links {
        join_table: "post_categories",
        left: "post_id",
        right: "category_id"
    })
});

nestrs_prisma::prisma_model_relations!(Profile {
    (foreign_key user_link {
        table: "profiles",
        record_pk: "id",
        fk: "user_id",
        nullable: true
    })
});

#[tokio::test]
async fn include_connect_disconnect_helpers_work() {
    let _guard = test_guard().lock().await;
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite:file:relation_queries?mode=memory&cache=shared")
            .pool_min(1)
            .pool_max(1),
    );
    let prisma = Arc::new(PrismaService::default());

    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "users" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "email" TEXT NOT NULL UNIQUE
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "posts" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "title" TEXT NOT NULL,
                "author_id" INTEGER NOT NULL
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "profiles" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "user_id" INTEGER UNIQUE,
                "bio" TEXT
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "categories" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "name" TEXT NOT NULL UNIQUE
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "post_categories" (
                "post_id" INTEGER NOT NULL,
                "category_id" INTEGER NOT NULL,
                PRIMARY KEY ("post_id", "category_id")
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(r#"DELETE FROM "post_categories""#)
        .await
        .unwrap();
    prisma.execute(r#"DELETE FROM "profiles""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "posts""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "categories""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "users""#).await.unwrap();

    prisma
        .execute(r#"INSERT INTO "users" ("email") VALUES ('a@prisma.io')"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "users" ("email") VALUES ('b@prisma.io')"#)
        .await
        .unwrap();
    prisma
        .execute(
            r#"INSERT INTO "posts" ("title", "author_id") VALUES ('P1', 1), ('P2', 1), ('P3', 1)"#,
        )
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "profiles" ("user_id", "bio") VALUES (1, 'hello')"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "categories" ("name") VALUES ('rust'), ('db')"#)
        .await
        .unwrap();

    let posts = prisma
        .include_one_to_many_as::<PostRow>(
            &OneToManyIncludeSpec::new("posts", "author_id"),
            RelationIdValue::Int(1),
            IncludeOptions {
                order_by: Some(("id".to_string(), SortOrder::Desc)),
                take: Some(2),
                skip: Some(0),
            },
        )
        .await
        .unwrap();
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].title, "P3");
    assert!(posts[0].id > 0);
    assert_eq!(posts[0].author_id, 1);

    let profile = prisma
        .include_one_to_one_as::<ProfileRow>(
            &OneToOneIncludeSpec::new("profiles", "user_id"),
            RelationIdValue::Int(1),
        )
        .await
        .unwrap();
    assert!(profile.is_some());
    let profile = profile.unwrap();
    assert!(profile.id > 0);
    assert_eq!(profile.user_id, 1);
    assert_eq!(profile.bio.as_deref(), Some("hello"));

    // connect/disconnect FK
    prisma
        .execute(r#"INSERT INTO "profiles" ("user_id", "bio") VALUES (NULL, 'temp')"#)
        .await
        .unwrap();
    let temp_profile_id: i64 = prisma
        .query_scalar(
            r#"SELECT "id" FROM "profiles" WHERE "bio" = 'temp' ORDER BY "id" DESC LIMIT 1"#,
        )
        .await
        .unwrap()
        .parse()
        .unwrap();
    let fk_spec = ForeignKeyMutationSpec::new("profiles", "id", "user_id", true);
    let c = prisma
        .connect_fk(
            &fk_spec,
            RelationIdValue::Int(temp_profile_id),
            RelationIdValue::Int(2),
        )
        .await
        .unwrap();
    assert_eq!(c, 1);
    let d = prisma
        .disconnect_fk(&fk_spec, RelationIdValue::Int(temp_profile_id))
        .await
        .unwrap();
    assert_eq!(d, 1);

    // connect/disconnect many-to-many
    let join_spec = JoinMutationSpec::new("post_categories", "post_id", "category_id");
    let ins = prisma
        .connect_many_to_many(&join_spec, RelationIdValue::Int(1), RelationIdValue::Int(1))
        .await
        .unwrap();
    assert_eq!(ins, 1);
    let del = prisma
        .disconnect_many_to_many(&join_spec, RelationIdValue::Int(1), RelationIdValue::Int(1))
        .await
        .unwrap();
    assert_eq!(del, 1);

    let rel_categories = prisma
        .include_many_to_many_as::<CategoryRow>(
            &ManyToManyIncludeSpec::new(
                "categories",
                "id",
                "post_categories",
                "post_id",
                "category_id",
            ),
            RelationIdValue::Int(1),
            IncludeOptions::default(),
        )
        .await
        .unwrap();
    assert!(rel_categories.is_empty());

    let categories = prisma
        .query_all_as::<CategoryRow>(r#"SELECT "id", "name" FROM "categories" ORDER BY "id""#)
        .await
        .unwrap();
    assert_eq!(categories.len(), 2);
    assert_eq!(categories[1].name, "db");
    assert_eq!(categories[1].id, 2);
}

#[tokio::test]
async fn model_repository_relation_extensions_work() {
    let _guard = test_guard().lock().await;
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("sqlite:file:relation_queries_repo?mode=memory&cache=shared")
            .pool_min(1)
            .pool_max(1),
    );
    let prisma = Arc::new(PrismaService::default());

    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "users" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "email" TEXT NOT NULL UNIQUE
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "posts" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "title" TEXT NOT NULL,
                "author_id" INTEGER NOT NULL
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "profiles" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "user_id" INTEGER UNIQUE,
                "bio" TEXT
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "categories" (
                "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                "name" TEXT NOT NULL UNIQUE
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(
            r#"CREATE TABLE IF NOT EXISTS "post_categories" (
                "post_id" INTEGER NOT NULL,
                "category_id" INTEGER NOT NULL,
                PRIMARY KEY ("post_id", "category_id")
            )"#,
        )
        .await
        .unwrap();
    prisma
        .execute(r#"DELETE FROM "post_categories""#)
        .await
        .unwrap();
    prisma.execute(r#"DELETE FROM "profiles""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "posts""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "categories""#).await.unwrap();
    prisma.execute(r#"DELETE FROM "users""#).await.unwrap();

    prisma
        .execute(r#"INSERT INTO "users" ("email") VALUES ('repo@prisma.io')"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "users" ("email") VALUES ('repo2@prisma.io')"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "posts" ("title", "author_id") VALUES ('RepoPost', 1)"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "profiles" ("user_id", "bio") VALUES (NULL, 'temp')"#)
        .await
        .unwrap();
    prisma
        .execute(r#"INSERT INTO "categories" ("name") VALUES ('orm')"#)
        .await
        .unwrap();

    let posts = prisma
        .user()
        .include_posts(RelationIdValue::Int(1), IncludeOptions::default())
        .await
        .unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "RepoPost");

    let linked = prisma
        .profile()
        .connect_user_link(RelationIdValue::Int(1), RelationIdValue::Int(2))
        .await
        .unwrap();
    assert_eq!(linked, 1);

    let maybe_profile = prisma
        .user()
        .include_profile(RelationIdValue::Int(2))
        .await
        .unwrap();
    assert!(maybe_profile.is_some());

    let inserted = prisma
        .post()
        .connect_category_links(RelationIdValue::Int(1), RelationIdValue::Int(1))
        .await
        .unwrap();
    assert_eq!(inserted, 1);

    let post_categories = prisma
        .post()
        .include_categories(RelationIdValue::Int(1), IncludeOptions::default())
        .await
        .unwrap();
    assert_eq!(post_categories.len(), 1);
    assert_eq!(post_categories[0].name, "orm");

    let removed = prisma
        .post()
        .disconnect_category_links(RelationIdValue::Int(1), RelationIdValue::Int(1))
        .await
        .unwrap();
    assert_eq!(removed, 1);

    let unlinked = prisma
        .profile()
        .disconnect_user_link(RelationIdValue::Int(1))
        .await
        .unwrap();
    assert_eq!(unlinked, 1);
}
