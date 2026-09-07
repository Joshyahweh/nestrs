#![cfg(feature = "authz-row-level")]

//! `Repository::find_many_authorized` + the post-load predicate retrofit on
//! the existing authorized finds, against real SQLite.

use nestrs::policies::Principal;
use nestrs::{with_ability, with_principal, Ability, Action, Entity, FindManyParams, Repository};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Doc {
    id: Option<i64>,
    author: String,
    tenant_id: i64,
}

impl Entity for Doc {
    const TABLE: &'static str = "docs";
    fn id(&self) -> Option<i64> {
        self.id
    }
    fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let id: i64 = row.try_get("id")?;
        let data: String = row.try_get("data")?;
        let parsed: serde_json::Value =
            serde_json::from_str(&data).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        Ok(Doc {
            id: Some(id),
            author: parsed
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tenant_id: parsed
                .get("tenant_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        })
    }
}

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-row-level-tests-{}-{}.sqlite",
        std::process::id(),
        {
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
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect sqlite");
    let pool = Arc::new(pool);
    sqlx::query("CREATE TABLE docs (id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL)")
        .execute(pool.as_ref())
        .await
        .expect("create docs");
    pool
}

fn doc(author: &str, tenant_id: i64) -> Doc {
    Doc {
        id: None,
        author: author.into(),
        tenant_id,
    }
}

fn alice_principal() -> Arc<Principal> {
    Arc::new(Principal {
        subject: "alice".into(),
        roles: vec![],
        claims: json!({ "tenant_id": 7 }),
    })
}

/// A custom closure predicate: only the author's own rows. Captures nothing
/// but the environment-independent logic.
fn own_author_only() -> Arc<Ability> {
    Arc::new(
        Ability::builder()
            .can_with_predicate(
                Action::Read,
                "docs",
                vec![],
                |row: &serde_json::Value, p: &Principal| row["author"] == p.subject,
            )
            .build(),
    )
}

#[tokio::test]
async fn custom_closure_predicate_post_filters_find_many_authorized() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for author in ["alice", "bob", "carol", "alice"] {
        repo.repo_crud_create(&doc(author, 1)).await.unwrap();
    }
    // Custom closures have no `sql_conditions`, so the fetch is unfiltered
    // and the closure drops rows post-load. Result is still correct.
    let rows = with_ability(
        own_author_only(),
        with_principal(alice_principal(), async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|d| d.author == "alice"));
}

#[tokio::test]
async fn find_many_authorized_plain_returns_all_rows_for_unrestricted_ability() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for i in 0..5 {
        repo.repo_crud_create(&doc("a", i)).await.unwrap();
    }
    let ability = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let rows = with_ability(ability, async {
        repo.find_many_authorized(Action::Read, Default::default())
            .await
            .unwrap()
    })
    .await;
    assert_eq!(rows.len(), 5);
}

#[tokio::test]
async fn find_many_authorized_limit_and_offset_paginate() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for i in 0..5 {
        repo.repo_crud_create(&doc(&format!("a{i}"), 1))
            .await
            .unwrap();
    }
    let ability = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let rows = with_ability(ability, async {
        repo.find_many_authorized(
            Action::Read,
            FindManyParams {
                limit: Some(2),
                offset: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    })
    .await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].author, "a1");
    assert_eq!(rows[1].author, "a2");
}

#[tokio::test]
async fn find_many_authorized_extra_where_binds_interleave_with_policy_binds() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for t in [7, 8, 7, 9] {
        repo.repo_crud_create(&doc("alice", t)).await.unwrap();
    }
    // Ability with a declarative tenant constraint (binds continue after the
    // caller's $1) + a caller-supplied extra filter on the author.
    let mut conds = serde_json::Map::new();
    conds.insert("tenant_id".into(), json!([7, 8]));
    let ability = Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "docs", conds)
            .build(),
    );
    let rows = with_ability(ability, async {
        repo.find_many_authorized(
            Action::Read,
            FindManyParams {
                extra_where: Some("json_extract(data, '$.author') = $1".into()),
                extra_binds: vec![json!("alice")],
                ..Default::default()
            },
        )
        .await
        .unwrap()
    })
    .await;
    // tenant in (7, 8) AND author = alice => the two tenant-7 rows and the
    // one tenant-8 row (the tenant-9 row is dropped by the policy bind).
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|d| d.author == "alice"));
}

#[tokio::test]
async fn predicate_without_principal_fails_loud() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    repo.repo_crud_create(&doc("alice", 1)).await.unwrap();
    // Ability installed but NO principal: the predicate cannot be evaluated,
    // so the repository errors (deny-closed) instead of leaking rows.
    let err = with_ability(own_author_only(), async {
        repo.find_many_authorized(Action::Read, Default::default())
            .await
    })
    .await
    .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("Principal")),
        "{err:?}"
    );
}

#[tokio::test]
async fn find_one_authorized_post_filters_through_predicate() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    let mine = repo.repo_crud_create(&doc("alice", 1)).await.unwrap();
    let theirs = repo.repo_crud_create(&doc("bob", 1)).await.unwrap();
    // Leak prevention retrofit: the existing authorized find also applies
    // the rule's predicate, not just its declarative conditions.
    with_ability(
        own_author_only(),
        with_principal(alice_principal(), async {
            let ok = repo
                .find_one_authorized(Action::Read, mine.id.unwrap())
                .await
                .unwrap();
            assert!(ok.is_some());
            let denied = repo
                .find_one_authorized(Action::Read, theirs.id.unwrap())
                .await
                .unwrap();
            assert!(denied.is_none(), "predicate-rejected row must not leak");
            let all = repo.find_all_authorized(Action::Read).await.unwrap();
            assert_eq!(all.len(), 1);
        }),
    )
    .await;
}

#[tokio::test]
async fn find_many_authorized_without_ability_errors() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool);
    // No with_ability wrapper — deny-closed.
    let err = repo
        .find_many_authorized(Action::Read, Default::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("Ability")),
        "{err:?}"
    );
}

#[tokio::test]
async fn find_many_authorized_denies_empty_for_ungranted_action() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    repo.repo_crud_create(&doc("alice", 1)).await.unwrap();
    // Rule only grants Read; Delete must come back as an empty set, not an
    // error (same channel as find_all_authorized).
    let ability = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let rows = with_ability(ability, async {
        repo.find_many_authorized(Action::Delete, Default::default())
            .await
            .unwrap()
    })
    .await;
    assert!(rows.is_empty());
}

/// Federation-safety: the predicate closure receives `(&row, &principal)` and
/// nothing else. This test's closure captures only a compile-time constant
/// config value — proving the predicate is a pure function of (row, principal)
/// plus its own frozen config, with no access to headers, environment, or
/// ambient state. The Rust type system enforces the shape; this test
/// demonstrates it.
#[tokio::test]
async fn federation_safe_predicate_is_a_pure_function_of_row_and_principal() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for t in [7, 8] {
        repo.repo_crud_create(&doc("alice", t)).await.unwrap();
    }
    // Frozen config (e.g. sourced from app settings at startup, not per-request).
    const ALLOWED_TENANTS: &[i64] = &[7, 42];
    let ability = Arc::new(
        Ability::builder()
            .can_with_predicate(
                Action::Read,
                "docs",
                vec![],
                move |row: &serde_json::Value, p: &Principal| {
                    // Only `row` and `p` are in scope here — the compiler
                    // rejects anything else that isn't captured. The row is
                    // visible when the author matches, the row's tenant
                    // matches the principal's claim, and that tenant is in
                    // the frozen allowed list.
                    let row_tenant = row["tenant_id"].as_i64();
                    let claim_tenant = p.claims.get("tenant_id").and_then(|v| v.as_i64());
                    row["author"] == p.subject
                        && row_tenant.is_some()
                        && row_tenant == claim_tenant
                        && claim_tenant
                            .map(|t| ALLOWED_TENANTS.contains(&t))
                            .unwrap_or(false)
                },
            )
            .build(),
    );
    let rows = with_ability(
        ability,
        with_principal(alice_principal(), async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tenant_id, 7);
}
