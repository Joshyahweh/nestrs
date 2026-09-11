// `find_*_authorized` / `CrudService` row-level coverage need the
// `authz-row-level` feature (which implies `authz` + `database-sqlx`); the
// previous `authz`-only gate let the file compile under feature sets where
// those methods don't exist (visible via `cargo test --workspace` feature
// unification, which enables `authz` + `database-sqlx` without row-level).
#![cfg(feature = "authz-row-level")]

//! `Repository<T>` + `CrudService<T>` integration tests using in-memory SQLite.
//! Covers Features B and D (RLS predicate → SQL WHERE).

use nestrs::{with_ability, Ability, Action, Conditions, CrudService, Entity, Repository};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

// -- Test entity --------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Post {
    id: Option<i64>,
    title: String,
    body: String,
    tenant_id: i64,
}

impl Entity for Post {
    const TABLE: &'static str = "posts";
    fn id(&self) -> Option<i64> {
        self.id
    }
    fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let id: i64 = row.try_get("id")?;
        let data: String = row.try_get("data")?;
        let parsed: serde_json::Value =
            serde_json::from_str(&data).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let body = parsed
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tenant_id = parsed
            .get("tenant_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(Post {
            id: Some(id),
            title,
            body,
            tenant_id,
        })
    }
}

// -- Per-test in-memory pool -------------------------------------------------
//
// Each test builds its own fresh `AnyPool` over a unique temp file so the
// tests don't share state. `install_default_drivers()` is idempotent inside
// sqlx::any.

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-repo-tests-{}-{}.sqlite",
        std::process::id(),
        uuid_like(),
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect sqlite");
    let pool = Arc::new(pool);
    sqlx::query("CREATE TABLE posts (id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL)")
        .execute(pool.as_ref())
        .await
        .expect("create posts");
    pool
}

/// Tiny uniquifier so concurrent test files don't collide on the same path.
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{n}")
}

// -- Tests --------------------------------------------------------------------

#[tokio::test]
async fn repository_save_then_find_one_round_trips() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool);
    let post = Post {
        id: None,
        title: "Hello".into(),
        body: "World".into(),
        tenant_id: 1,
    };
    let created = repo.repo_crud_create(&post).await.expect("create");
    assert!(created.id.is_some());
    let found = repo.find_one(created.id.unwrap()).await.expect("find_one");
    let found = found.expect("Some");
    assert_eq!(found.title, "Hello");
    assert_eq!(found.tenant_id, 1);
}

#[tokio::test]
async fn repository_save_then_find_all_returns_inserted() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool);
    let _a = repo
        .repo_crud_create(&Post {
            id: None,
            title: "A".into(),
            body: "a".into(),
            tenant_id: 1,
        })
        .await
        .expect("a");
    let _b = repo
        .repo_crud_create(&Post {
            id: None,
            title: "B".into(),
            body: "b".into(),
            tenant_id: 1,
        })
        .await
        .expect("b");
    let all = repo.find_all().await.expect("find_all");
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn repository_delete_removes_row() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool);
    let created = repo
        .repo_crud_create(&Post {
            id: None,
            title: "x".into(),
            body: "y".into(),
            tenant_id: 1,
        })
        .await
        .expect("create");
    let id = created.id.unwrap();
    let removed = repo.delete(id).await.expect("delete");
    assert!(removed);
    let gone = repo.find_one(id).await.expect("find");
    assert!(gone.is_none());
}

#[tokio::test]
async fn repository_count_reflects_inserts() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool);
    let initial = repo.count().await.expect("count");
    for i in 0..3 {
        repo.repo_crud_create(&Post {
            id: None,
            title: format!("t{i}"),
            body: "b".into(),
            tenant_id: 1,
        })
        .await
        .expect("create");
    }
    let after = repo.count().await.expect("count");
    assert_eq!(after, initial + 3);
}

#[tokio::test]
async fn repository_find_where_filters_rows() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool);
    repo.repo_crud_create(&Post {
        id: None,
        title: "T1".into(),
        body: "b".into(),
        tenant_id: 7,
    })
    .await
    .expect("c1");
    repo.repo_crud_create(&Post {
        id: None,
        title: "T2".into(),
        body: "b".into(),
        tenant_id: 8,
    })
    .await
    .expect("c2");
    let rows = repo
        .find_where("json_extract(data, '$.tenant_id') = $1", &[json!(7)])
        .await
        .expect("find_where");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "T1");
}

#[tokio::test]
async fn crud_service_create_read_update_delete_list() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool);
    // Under authz-row-level the CrudService is deny-closed: install an
    // unrestricted ability (Manage implies create/read/update/delete).
    let ability = Arc::new(Ability::builder().can(Action::Manage, "posts").build());
    with_ability(ability, async {
        let created = svc
            .create(json!({ "title": "S1", "body": "b1", "tenant_id": 2 }))
            .await
            .expect("create");
        let id = created.id.expect("id");
        let read = svc.read(id).await.expect("read").expect("Some");
        assert_eq!(read.title, "S1");
        let updated = svc
            .update(
                id,
                json!({ "title": "S1-new", "body": "b1", "tenant_id": 2 }),
            )
            .await
            .expect("update")
            .expect("Some");
        assert_eq!(updated.title, "S1-new");
        let listed = svc.list().await.expect("list");
        assert!(listed.iter().any(|p| p.id == Some(id)));
        let removed = svc.delete(id).await.expect("delete");
        assert!(removed);
    })
    .await;
}

// -- Feature D: RLS predicate → SQL WHERE -------------------------------------

fn ability_tenant(tenant_id: i64) -> Arc<Ability> {
    let mut conds = Conditions::new();
    conds.insert("tenant_id".into(), json!(tenant_id));
    Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "posts", conds)
            .can_with_conditions(Action::Update, "posts", Conditions::new())
            .build(),
    )
}

#[tokio::test]
async fn find_all_authorized_appends_tenant_id_predicate() {
    let pool = fresh_pool().await;
    // Insert two posts: one for tenant 7, one for tenant 8.
    let repo = Repository::<Post>::new(pool.clone());
    repo.repo_crud_create(&Post {
        id: None,
        title: "T7".into(),
        body: "b".into(),
        tenant_id: 7,
    })
    .await
    .expect("c1");
    repo.repo_crud_create(&Post {
        id: None,
        title: "T8".into(),
        body: "b".into(),
        tenant_id: 8,
    })
    .await
    .expect("c2");

    // Set the ability slot so find_*_authorized sees tenant_id = 7.
    let ability = ability_tenant(7);
    with_ability(ability, async {
        let rows = repo
            .find_all_authorized(Action::Read)
            .await
            .expect("find_all_authorized");
        // Only the tenant 7 row should come back.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "T7");
    })
    .await;
}

#[tokio::test]
async fn find_one_authorized_returns_none_when_constraint_misses() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool.clone());
    let created = repo
        .repo_crud_create(&Post {
            id: None,
            title: "T8".into(),
            body: "b".into(),
            tenant_id: 8,
        })
        .await
        .expect("c");
    let id = created.id.unwrap();
    let ability = ability_tenant(7);
    with_ability(ability, async {
        let found = repo
            .find_one_authorized(Action::Read, id)
            .await
            .expect("find");
        assert!(
            found.is_none(),
            "tenant 8 row must be invisible to tenant 7"
        );
    })
    .await;
}

#[tokio::test]
async fn find_one_authorized_skips_constraint_when_no_rule_matches() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool.clone());
    let created = repo
        .repo_crud_create(&Post {
            id: None,
            title: "T9".into(),
            body: "b".into(),
            tenant_id: 9,
        })
        .await
        .expect("c");
    let id = created.id.unwrap();
    // Ability that has NO rule for posts (different subject type only).
    let ability = Arc::new(Ability::builder().can(Action::Read, "Comment").build());
    with_ability(ability, async {
        let found = repo
            .find_one_authorized(Action::Read, id)
            .await
            .expect("find");
        assert!(found.is_none(), "no matching rule => deny");
    })
    .await;
}

#[tokio::test]
async fn find_all_authorized_appends_in_clause_for_array_constraint() {
    let pool = fresh_pool().await;
    let repo = Repository::<Post>::new(pool.clone());
    for t in [1, 2, 3, 4] {
        repo.repo_crud_create(&Post {
            id: None,
            title: format!("T{t}"),
            body: "b".into(),
            tenant_id: t,
        })
        .await
        .expect("c");
    }
    // Conditions: tenant_id in [2, 4]
    let mut conds = Conditions::new();
    conds.insert("tenant_id".into(), json!([2, 4]));
    let ability = Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "posts", conds)
            .build(),
    );
    with_ability(ability, async {
        let rows = repo
            .find_all_authorized(Action::Read)
            .await
            .expect("find_all_authorized");
        let tenants: Vec<i64> = rows.iter().map(|p| p.tenant_id).collect();
        assert_eq!(tenants.len(), 2);
        assert!(tenants.contains(&2));
        assert!(tenants.contains(&4));
    })
    .await;
}
