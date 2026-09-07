#![cfg(feature = "authz-row-level")]

//! Pre-built row-level predicates (`nestrs::predicates`).
//!
//! Two layers are exercised per predicate where relevant:
//! * the closure check — `can(Subject::Instance(row))` with a principal
//!   installed in the task-local slot;
//! * the SQL pushdown — `sql_conditions(&principal)` compiled by
//!   `find_many_authorized` against a real SQLite pool, asserting the
//!   pushdown row set equals the closure row set.

use nestrs::policies::Principal;
use nestrs::predicates::*;
use nestrs::{
    with_ability, with_principal, Ability, Action, Entity, Repository, RowPredicate, Subject,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

fn principal(subject: &str, roles: &[&str], claims: serde_json::Value) -> Arc<Principal> {
    Arc::new(Principal {
        subject: subject.into(),
        roles: roles.iter().map(|s| s.to_string()).collect(),
        claims,
    })
}

fn ability_with<P: RowPredicate + 'static>(predicate: P) -> Arc<Ability> {
    Arc::new(
        Ability::builder()
            .can_with_predicate(Action::Read, "docs", vec![], predicate)
            .build(),
    )
}

// -- Closure checks (no DB) ----------------------------------------------------

#[tokio::test]
async fn author_is_current_user_matches_own_rows_only() {
    let ability = ability_with(AuthorIsCurrentUser::new());
    let mine = Subject::Instance(json!({ "type": "docs", "author": "alice" }));
    let theirs = Subject::Instance(json!({ "type": "docs", "author": "bob" }));
    let p = principal("alice", &[], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &mine));
        assert!(!ability.can(&Action::Read, &theirs));
    })
    .await;
}

#[tokio::test]
async fn belongs_to_user_matches_user_id_column() {
    let ability = ability_with(BelongsToUser::new());
    let mine = Subject::Instance(json!({ "type": "docs", "user_id": "u1" }));
    let theirs = Subject::Instance(json!({ "type": "docs", "user_id": "u2" }));
    let p = principal("u1", &[], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &mine));
        assert!(!ability.can(&Action::Read, &theirs));
    })
    .await;
}

#[tokio::test]
async fn within_tenant_uses_principal_claim() {
    let ability = ability_with(WithinTenant::new());
    let same = Subject::Instance(json!({ "type": "docs", "tenant_id": 12 }));
    let other = Subject::Instance(json!({ "type": "docs", "tenant_id": 13 }));
    let p = principal("u1", &[], json!({ "tenant_id": 12 }));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &same));
        assert!(!ability.can(&Action::Read, &other));
    })
    .await;
    // No tenant claim => deny everything (conservative).
    let p2 = principal("u1", &[], json!({}));
    with_principal(p2, async {
        assert!(!ability.can(&Action::Read, &same));
    })
    .await;
}

#[tokio::test]
async fn owner_or_admin_admins_see_all_others_own_rows() {
    let ability = ability_with(OwnerOrAdmin::new());
    let someone_elses = Subject::Instance(json!({ "type": "docs", "owner": "bob" }));
    let admin = principal("a1", &["admin"], json!({}));
    with_principal(admin, async {
        assert!(ability.can(&Action::Read, &someone_elses));
    })
    .await;
    let user = principal("u1", &[], json!({}));
    let own = Subject::Instance(json!({ "type": "docs", "owner": "u1" }));
    with_principal(user, async {
        assert!(ability.can(&Action::Read, &own));
        assert!(!ability.can(&Action::Read, &someone_elses));
    })
    .await;
}

#[tokio::test]
async fn tenant_or_admin_admin_bypasses_tenant_check() {
    let ability = ability_with(TenantOrAdmin::new());
    let other_tenant = Subject::Instance(json!({ "type": "docs", "tenant_id": 99 }));
    let admin = principal("a1", &["admin"], json!({ "tenant_id": 1 }));
    with_principal(admin, async {
        assert!(ability.can(&Action::Read, &other_tenant));
    })
    .await;
    let user = principal("u1", &[], json!({ "tenant_id": 7 }));
    with_principal(user, async {
        assert!(!ability.can(&Action::Read, &other_tenant));
        let own = Subject::Instance(json!({ "type": "docs", "tenant_id": 7 }));
        assert!(ability.can(&Action::Read, &own));
    })
    .await;
}

#[tokio::test]
async fn self_or_admin_row_is_the_principal() {
    // The JSON-blob id is a string here (uid), so use the pub field to
    // point the predicate at the right column.
    let ability = ability_with(SelfOrAdmin {
        field: "uid".into(),
        admin_role: "admin".into(),
    });
    let self_row = Subject::Instance(json!({ "type": "docs", "uid": "u1" }));
    let other_row = Subject::Instance(json!({ "type": "docs", "uid": "u2" }));
    let user = principal("u1", &[], json!({}));
    with_principal(user, async {
        assert!(ability.can(&Action::Read, &self_row));
        assert!(!ability.can(&Action::Read, &other_row));
    })
    .await;
    let admin = principal("a1", &["admin"], json!({}));
    with_principal(admin, async {
        assert!(ability.can(&Action::Read, &other_row));
    })
    .await;
}

#[tokio::test]
async fn published_only_hides_drafts() {
    let ability = ability_with(PublishedOnly::new());
    let published = Subject::Instance(json!({ "type": "docs", "published": true }));
    let draft = Subject::Instance(json!({ "type": "docs", "published": false }));
    // Static predicate (no principal fields consulted) — but the deny-
    // conservative convention still requires SOME principal in scope before
    // any predicate rule is evaluated.
    let p = principal("anyone", &[], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &published));
        assert!(!ability.can(&Action::Read, &draft));
    })
    .await;
}

#[tokio::test]
async fn not_deleted_hides_soft_deleted_rows() {
    let ability = ability_with(NotDeleted::new());
    let live = Subject::Instance(json!({ "type": "docs", "deleted_at": null }));
    let tombstoned =
        Subject::Instance(json!({ "type": "docs", "deleted_at": "2026-01-01T00:00:00Z" }));
    let p = principal("anyone", &[], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &live));
        assert!(!ability.can(&Action::Read, &tombstoned));
    })
    .await;
}

#[tokio::test]
async fn public_or_owner_row_dependent_or() {
    let ability = ability_with(PublicOrOwner::new());
    let public = Subject::Instance(json!({ "type": "docs", "visibility": "public" }));
    let own_private =
        Subject::Instance(json!({ "type": "docs", "visibility": "private", "owner": "u1" }));
    let others_private =
        Subject::Instance(json!({ "type": "docs", "visibility": "private", "owner": "bob" }));
    let p = principal("u1", &[], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &public));
        assert!(ability.can(&Action::Read, &own_private));
        assert!(!ability.can(&Action::Read, &others_private));
    })
    .await;
}

#[tokio::test]
async fn has_role_is_a_row_agnostic_role_gate() {
    let ability = ability_with(HasRole::new("auditor"));
    let any_row = Subject::Instance(json!({ "type": "docs", "author": "bob" }));
    let p = principal("u1", &["auditor"], json!({}));
    with_principal(p, async {
        assert!(ability.can(&Action::Read, &any_row));
    })
    .await;
    let p2 = principal("u1", &[], json!({}));
    with_principal(p2, async {
        assert!(!ability.can(&Action::Read, &any_row));
    })
    .await;
}

// -- SQL pushdown round-trips (real SQLite) ------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Doc {
    id: Option<i64>,
    author: String,
    tenant_id: i64,
    published: bool,
    deleted_at: Option<String>,
    visibility: String,
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
        let s = |k: &str| {
            parsed
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        Ok(Doc {
            id: Some(id),
            author: s("author"),
            tenant_id: parsed
                .get("tenant_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            published: parsed
                .get("published")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            deleted_at: parsed
                .get("deleted_at")
                .filter(|v| !v.is_null())
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            visibility: s("visibility"),
        })
    }
}

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-predicates-tests-{}-{}.sqlite",
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

fn doc(author: &str, tenant_id: i64, deleted_at: Option<&str>) -> Doc {
    Doc {
        id: None,
        author: author.into(),
        tenant_id,
        published: true,
        deleted_at: deleted_at.map(|s| s.into()),
        visibility: "private".into(),
    }
}

/// Pushdown equivalence: the rows returned by `find_many_authorized` (WHERE
/// compiled from `sql_conditions`) must equal the rows the closure accepts.
/// The closure post-filter runs *after* the WHERE, so a wrong pushdown shows
/// up as missing rows, not extra ones — hence the baseline is a plain
/// (unrestricted) ability fetch.
#[tokio::test]
async fn author_is_current_user_pushdown_equals_closure_row_set() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for author in ["alice", "bob", "alice", "carol"] {
        repo.repo_crud_create(&doc(author, 1, None)).await.unwrap();
    }

    let p = principal("alice", &[], json!({}));
    let base = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let filtered = ability_with(AuthorIsCurrentUser::new());

    let expected = with_ability(
        base,
        with_principal(p.clone(), async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    let got = with_ability(
        filtered,
        with_principal(p, async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    assert_eq!(expected.len(), 4);
    let expected_authors: Vec<&str> = expected.iter().map(|d| d.author.as_str()).collect();
    let got_authors: Vec<&str> = got.iter().map(|d| d.author.as_str()).collect();
    // The filtered fetch is the closure-accepted SUBSET of the baseline —
    // every surviving row exists in the unrestricted set and every
    // alice-authored row survived (no pushdown over-filtering).
    assert!(got_authors.iter().all(|a| expected_authors.contains(a)));
    assert_eq!(got_authors, vec!["alice", "alice"]);
}

#[tokio::test]
async fn within_tenant_pushdown_equals_closure_row_set() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    for t in [7, 8, 7, 9] {
        repo.repo_crud_create(&doc("alice", t, None)).await.unwrap();
    }

    let p = principal("u1", &[], json!({ "tenant_id": 7 }));
    let base = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let filtered = ability_with(WithinTenant::new());

    let expected = with_ability(base, async {
        repo.find_many_authorized(Action::Read, Default::default())
            .await
            .unwrap()
    })
    .await;
    let got = with_ability(
        filtered,
        with_principal(p, async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    assert_eq!(expected.len(), 4);
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|d| d.tenant_id == 7));
}

#[tokio::test]
async fn not_deleted_pushdown_equals_closure_row_set() {
    let pool = fresh_pool().await;
    let repo = Repository::<Doc>::new(pool.clone());
    repo.repo_crud_create(&doc("a", 1, None)).await.unwrap();
    repo.repo_crud_create(&doc("b", 1, Some("2026-01-01")))
        .await
        .unwrap();
    repo.repo_crud_create(&doc("c", 1, None)).await.unwrap();

    let p = principal("u1", &[], json!({}));
    let base = Arc::new(Ability::builder().can(Action::Read, "docs").build());
    let filtered = ability_with(NotDeleted::new());

    let expected = with_ability(base, async {
        repo.find_many_authorized(Action::Read, Default::default())
            .await
            .unwrap()
    })
    .await;
    let got = with_ability(
        filtered,
        with_principal(p, async {
            repo.find_many_authorized(Action::Read, Default::default())
                .await
                .unwrap()
        }),
    )
    .await;
    assert_eq!(expected.len(), 3);
    assert_eq!(got.len(), 2);
}
