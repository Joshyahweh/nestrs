#![cfg(feature = "authz-row-level")]

//! `CrudService` end-to-end row-level enforcement: under `authz-row-level`
//! every method auto-applies the request-scoped `Ability`'s row predicate —
//! mandatory, deny-closed, no opt-out.

use nestrs::policies::Principal;
use nestrs::predicates::AuthorIsCurrentUser;
use nestrs::{with_ability, with_principal, Ability, Action, CrudService, Entity};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Post {
    id: Option<i64>,
    author: String,
    body: String,
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
        let s = |k: &str| {
            parsed
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        Ok(Post {
            id: Some(id),
            author: s("author"),
            body: s("body"),
        })
    }
}

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-crud-row-level-tests-{}-{}.sqlite",
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
    sqlx::query("CREATE TABLE posts (id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL)")
        .execute(pool.as_ref())
        .await
        .expect("create posts");
    pool
}

fn alice() -> Arc<Principal> {
    Arc::new(Principal {
        subject: "alice".into(),
        roles: vec![],
        claims: json!({}),
    })
}

fn bob() -> Arc<Principal> {
    Arc::new(Principal {
        subject: "bob".into(),
        roles: vec![],
        claims: json!({}),
    })
}

/// Author-owns-post rules for every action the tests exercise.
fn author_rules() -> Arc<Ability> {
    Arc::new(
        Ability::builder()
            .can_with_predicate(Action::Create, "posts", vec![], AuthorIsCurrentUser::new())
            .can_with_predicate(Action::Read, "posts", vec![], AuthorIsCurrentUser::new())
            .can_with_predicate(Action::Update, "posts", vec![], AuthorIsCurrentUser::new())
            .can_with_predicate(Action::Delete, "posts", vec![], AuthorIsCurrentUser::new())
            .build(),
    )
}

#[tokio::test]
async fn create_with_matching_author_succeeds() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool);
    let created = with_ability(
        author_rules(),
        with_principal(alice(), async {
            svc.create(json!({ "author": "alice", "body": "hi" }))
                .await
                .expect("create own row")
        }),
    )
    .await;
    assert!(created.id.is_some());
    assert_eq!(created.author, "alice");
}

#[tokio::test]
async fn create_with_mismatched_author_is_denied() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool);
    // Alice can't insert a row authored as bob — the predicate judges the
    // CANDIDATE row before it exists.
    let err = with_ability(
        author_rules(),
        with_principal(alice(), async {
            svc.create(json!({ "author": "bob", "body": "spoof" }))
                .await
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("policy denied")),
        "{err:?}"
    );
}

#[tokio::test]
async fn read_of_another_authors_row_is_none() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool.clone());
    // Seed bob's row through the repository primitive (no ability needed).
    let theirs = svc
        .repo()
        .repo_crud_create(&Post {
            id: None,
            author: "bob".into(),
            body: "secret".into(),
        })
        .await
        .unwrap();
    let id = theirs.id.unwrap();
    let got = with_ability(
        author_rules(),
        with_principal(alice(), async { svc.read(id).await.expect("read") }),
    )
    .await;
    assert!(got.is_none(), "another author's row must be invisible");
}

#[tokio::test]
async fn list_returns_only_visible_rows() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool.clone());
    let repo = svc.repo();
    repo.repo_crud_create(&Post {
        id: None,
        author: "alice".into(),
        body: "a".into(),
    })
    .await
    .unwrap();
    repo.repo_crud_create(&Post {
        id: None,
        author: "bob".into(),
        body: "b".into(),
    })
    .await
    .unwrap();
    let rows = with_ability(
        author_rules(),
        with_principal(alice(), async { svc.list().await.expect("list") }),
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].author, "alice");
}

#[tokio::test]
async fn update_by_another_author_is_invisible_then_denied() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool.clone());
    let mine = svc
        .repo()
        .repo_crud_create(&Post {
            id: None,
            author: "alice".into(),
            body: "v1".into(),
        })
        .await
        .unwrap();
    // A second author's row exists but isn't touched directly in this test —
    // the invisible-channel case below operates on `mine` from Bob's view.
    let _theirs = svc
        .repo()
        .repo_crud_create(&Post {
            id: None,
            author: "bob".into(),
            body: "v1".into(),
        })
        .await
        .unwrap();

    // Alice updating her own row works, and the replacement must stay in
    // scope: rewriting author => bob moves the row out of her visibility.
    let kept = with_ability(
        author_rules(),
        with_principal(alice(), async {
            svc.update(mine.id.unwrap(), json!({ "author": "alice", "body": "v2" }))
                .await
                .expect("update own row")
        }),
    )
    .await;
    assert_eq!(kept.unwrap().body, "v2");

    let err = with_ability(
        author_rules(),
        with_principal(alice(), async {
            svc.update(mine.id.unwrap(), json!({ "author": "bob", "body": "v3" }))
                .await
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("policy denied")),
        "{err:?}"
    );

    // Bob updating Alice's row: the row is invisible to him => Ok(None)
    // (can't update what you can't see), NOT a spoofed success.
    let invisible = with_ability(
        author_rules(),
        with_principal(bob(), async {
            svc.update(
                mine.id.unwrap(),
                json!({ "author": "bob", "body": "hijack" }),
            )
            .await
            .expect("update")
        }),
    )
    .await;
    assert!(invisible.is_none());
}

#[tokio::test]
async fn delete_channels_invisible_err_and_ok() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool.clone());
    let mine = svc
        .repo()
        .repo_crud_create(&Post {
            id: None,
            author: "alice".into(),
            body: "a".into(),
        })
        .await
        .unwrap();
    let theirs = svc
        .repo()
        .repo_crud_create(&Post {
            id: None,
            author: "bob".into(),
            body: "b".into(),
        })
        .await
        .unwrap();

    // Another author's row is INVISIBLE to Alice (fetch is post-filtered
    // through the predicate) => Ok(false), never an error and never a leak.
    let invisible = with_ability(
        author_rules(),
        with_principal(alice(), async { svc.delete(theirs.id.unwrap()).await }),
    )
    .await;
    assert!(!invisible.expect("delete"));

    // Same channel for Bob on Alice's row.
    let invisible = with_ability(
        author_rules(),
        with_principal(bob(), async { svc.delete(mine.id.unwrap()).await }),
    )
    .await;
    assert!(!invisible.expect("delete"));

    // An ability with NO delete grant => Err (policy denied, not invisible).
    let no_delete = Arc::new(
        Ability::builder()
            .can_with_predicate(Action::Read, "posts", vec![], AuthorIsCurrentUser::new())
            .build(),
    );
    let err = with_ability(
        no_delete,
        with_principal(alice(), async { svc.delete(theirs.id.unwrap()).await }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("policy denied")),
        "{err:?}"
    );

    // Own row deletes fine.
    let ok = with_ability(
        author_rules(),
        with_principal(alice(), async {
            svc.delete(mine.id.unwrap()).await.expect("delete")
        }),
    )
    .await;
    assert!(ok);
}

#[tokio::test]
async fn no_ability_in_scope_is_deny_closed() {
    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool);
    // No with_ability wrapper anywhere: every method must refuse.
    let err = svc
        .create(json!({ "author": "alice", "body": "x" }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, sqlx::Error::Protocol(ref msg) if msg.contains("Ability")),
        "{err:?}"
    );
    let err = svc.read(1).await.unwrap_err();
    assert!(matches!(err, sqlx::Error::Protocol(_)), "{err:?}");
    let err = svc.list().await.unwrap_err();
    assert!(matches!(err, sqlx::Error::Protocol(_)), "{err:?}");
    let err = svc
        .update(1, json!({ "author": "a", "body": "b" }))
        .await
        .unwrap_err();
    assert!(matches!(err, sqlx::Error::Protocol(_)), "{err:?}");
    let err = svc.delete(1).await.unwrap_err();
    assert!(matches!(err, sqlx::Error::Protocol(_)), "{err:?}");
}
