#![cfg(all(feature = "database-sqlx", feature = "test-hooks"))]

//! Wave 4.4 — `#[nestrs::crud(...)]` proc-macro integration tests.
//!
//! Each test builds a fresh `axum::Router` with a real SQLite pool
//! (per-test tempfile, dropped on test teardown), seeds 3 rows, and
//! exercises the macro-generated 5-verb controller through real HTTP
//! requests via `tower::ServiceExt::oneshot`. The auth-integration
//! tests additionally install a request-scoped `Ability` and a
//! `Principal` to validate that `CrudService`'s deny-closed posture
//! flows through the macro's generated service unchanged.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::middleware;
use nestrs::{
    controller, crud, install_policies_middleware, module, Ability, Action, CrudService, Entity,
    NestApplication,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

// Conditionally import authz types only when the feature is enabled.
#[cfg(feature = "authz-row-level")]
use nestrs::predicates::AuthorIsCurrentUser;
#[cfg(feature = "authz")]
use nestrs::{policies::Principal, with_ability, with_principal};
#[cfg(feature = "authz")]
use serde_json::json;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

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

#[derive(Debug, Clone, Serialize, Deserialize, nestrs::NestDto)]
struct PostDto {
    pub id: i64,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, nestrs::NestDto)]
struct CreatePostDto {
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, nestrs::NestDto)]
struct UpdatePostDto {
    pub body: String,
}

/// The macro-generated controller. The `pool` field is the only
/// `Arc<sqlx::AnyPool>` the macro needs; everything else (service,
/// state, handlers) is emitted by `#[crud]` itself.
#[controller(prefix = "/posts")]
#[crud(
    entity = Post,
    output = PostDto,
    create = CreatePostDto,
    update = UpdatePostDto,
)]
struct PostController {
    // The macro needs a typed `pool` field at expansion time to drive its
    // hidden `__PostCrudState`, but the generated handlers carry the real
    // pool through state — the field itself is never read.
    #[allow(dead_code)]
    pool: Arc<sqlx::AnyPool>,
}

#[module(controllers = [PostController], providers = [__PostCrudState])]
struct PostsModule;

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "nestrs-crud-macro-tests-{}-{}-{}.sqlite",
        std::process::id(),
        nanos,
        n
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

/// Build the router with 3 seeded rows: alice posts 2, bob posts 1.
async fn setup_router() -> axum::Router {
    // Clear the process-wide module build cache so each test gets a fresh
    // build with its own pool override. The memoized `Module::build()`
    // would otherwise return a cached router from a previous test.
    nestrs::core::clear_module_cache_for_tests();

    let pool = fresh_pool().await;
    let svc = CrudService::<Post>::new(pool.clone());
    svc.repo()
        .repo_crud_create(&Post {
            id: None,
            author: "alice".into(),
            body: "alice first".into(),
        })
        .await
        .expect("seed alice 1");
    svc.repo()
        .repo_crud_create(&Post {
            id: None,
            author: "alice".into(),
            body: "alice second".into(),
        })
        .await
        .expect("seed alice 2");
    svc.repo()
        .repo_crud_create(&Post {
            id: None,
            author: "bob".into(),
            body: "bob only".into(),
        })
        .await
        .expect("seed bob");

    // Use DynamicModuleBuilder to apply the provider override BEFORE
    // controllers are registered, so the generated handlers get the real pool.
    let dynamic_module = nestrs::core::DynamicModuleBuilder::<PostsModule>::new()
        .override_provider::<__PostCrudState>(Arc::new(__PostCrudState::from_pool(pool)))
        .build();
    let app = NestApplication::from_registry_and_router(
        std::sync::Arc::new(dynamic_module.registry),
        dynamic_module.router,
    );
    // Install policies middleware so CrudService can find the Ability
    let ability = Arc::new(
        Ability::builder()
            .can(Action::Read, "posts")
            .can(Action::Create, "posts")
            .can(Action::Update, "posts")
            .can(Action::Delete, "posts")
            .build(),
    );
    app.into_router().layer(middleware::from_fn_with_state(
        ability,
        install_policies_middleware,
    ))
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, String) {
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    let status = res.status();
    let body = to_bytes(res.into_body(), 64 * 1024).await.expect("body");
    let body_str = String::from_utf8_lossy(&body).to_string();
    eprintln!("GET {} -> {} : {}", uri, status, body_str);
    (status, body_str)
}

async fn send_json(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, String) {
    let req = if let Some(b) = body {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .expect("request")
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request")
    };
    let res = router.clone().oneshot(req).await.expect("serve");
    let status = res.status();
    let body = to_bytes(res.into_body(), 64 * 1024).await.expect("body");
    let body_str = String::from_utf8_lossy(&body).to_string();
    eprintln!("{} {} -> {} : {}", method, uri, status, body_str);
    (status, body_str)
}

// ---------------------------------------------------------------------------
// GET /
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_returns_all_seeded_rows() {
    let r = setup_router().await;
    let (status, body) = get(&r, "/posts/").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn list_paginates_by_page_and_per_page() {
    let r = setup_router().await;
    let (status, body) = get(&r, "/posts/?page=1&per_page=2").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn list_rejects_page_zero() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/?page=0").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_rejects_negative_page() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/?page=-1").await;
    // u32 parse fails on negative input — Axum returns 400.
    assert!(matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ));
}

#[tokio::test]
async fn list_rejects_per_page_zero() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/?per_page=0").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_rejects_per_page_over_max() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/?per_page=10000").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_sorts_descending_by_author() {
    let r = setup_router().await;
    let (status, body) = get(&r, "/posts/?sort=author:DESC").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    let first = rows[0]["author"].as_str().expect("author");
    assert_eq!(first, "bob", "DESC puts bob first");
}

#[tokio::test]
async fn list_filters_by_author_via_bracketed_query() {
    let r = setup_router().await;
    let (status, body) = get(&r, "/posts/?filter%5Bauthor%5D=alice").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row["author"].as_str().expect("author"), "alice");
    }
}

// ---------------------------------------------------------------------------
// GET /:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_one_returns_existing_row() {
    let r = setup_router().await;
    let (status, body) = get(&r, "/posts/1").await;
    assert_eq!(status, StatusCode::OK);
    let row: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(row["id"], 1);
}

#[tokio::test]
async fn get_one_returns_404_for_missing_id() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_one_returns_400_for_non_numeric_id() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_one_returns_404_for_negative_id() {
    let r = setup_router().await;
    let (status, _body) = get(&r, "/posts/-1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// POST /
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_inserts_a_new_row() {
    let r = setup_router().await;
    let (status, body) = send_json(
        &r,
        "POST",
        "/posts/",
        Some(r#"{"author":"carol","body":"hello"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let row: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(row["author"], "carol");
    assert!(row["id"].as_i64().expect("id") > 0);
}

#[tokio::test]
async fn create_rejects_malformed_json() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "POST", "/posts/", Some(r#"{not json"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_accepts_valid_payload_after_listing() {
    let r = setup_router().await;
    let (status, _body) = send_json(
        &r,
        "POST",
        "/posts/",
        Some(r#"{"author":"dave","body":"yo"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = get(&r, "/posts/").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_eq!(rows.len(), 4);
}

// ---------------------------------------------------------------------------
// PATCH /:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_patches_existing_row() {
    let r = setup_router().await;
    let (status, body) = send_json(&r, "PATCH", "/posts/1", Some(r#"{"body":"edited"}"#)).await;
    assert_eq!(status, StatusCode::OK);
    let row: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(row["body"], "edited");
}

#[tokio::test]
async fn update_returns_404_for_missing_id() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "PATCH", "/posts/999", Some(r#"{"body":"nope"}"#)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_rejects_malformed_body() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "PATCH", "/posts/1", Some(r#"{not json"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// DELETE /:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_removes_existing_row() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "DELETE", "/posts/1", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _body) = get(&r, "/posts/1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_returns_404_for_missing_id() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "DELETE", "/posts/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn double_delete_is_idempotently_404() {
    let r = setup_router().await;
    let (status, _body) = send_json(&r, "DELETE", "/posts/1", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _body) = send_json(&r, "DELETE", "/posts/1", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// RouteRegistry integration (OpenAPI surface)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_five_routes_are_registered() {
    let _ = setup_router().await; // force the module to build + register
    let routes = nestrs::core::RouteRegistry::list();
    let paths: Vec<&str> = routes.iter().map(|r| r.path).collect();
    for verb_path in [
        ("GET", "/posts/"),
        ("GET", "/posts/:id"),
        ("POST", "/posts/"),
        ("PATCH", "/posts/:id"),
        ("DELETE", "/posts/:id"),
    ] {
        let (verb, path) = verb_path;
        assert!(
            routes.iter().any(|r| r.method == verb && r.path == path),
            "expected {verb} {path} in RouteRegistry; got {paths:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Authz integration (deny-closed row-level authz through the macro)
// ---------------------------------------------------------------------------

#[cfg(feature = "authz-row-level")]
mod authz {
    use super::*;

    fn alice_only_ability() -> Arc<Ability> {
        Arc::new(
            Ability::builder()
                .can_with_predicate(Action::Read, "posts", vec![], AuthorIsCurrentUser::new())
                .build(),
        )
    }

    fn alice_principal() -> Arc<Principal> {
        Arc::new(Principal {
            subject: "alice".into(),
            roles: vec![],
            claims: json!({}),
        })
    }

    async fn build_authz_router() -> axum::Router {
        // Clear the process-wide module build cache so each test gets a fresh
        // build with its own pool override.
        nestrs::core::clear_module_cache_for_tests();

        let pool = fresh_pool().await;
        let svc = CrudService::<Post>::new(pool.clone());
        svc.repo()
            .repo_crud_create(&Post {
                id: None,
                author: "alice".into(),
                body: "alice first".into(),
            })
            .await
            .expect("seed alice");
        svc.repo()
            .repo_crud_create(&Post {
                id: None,
                author: "bob".into(),
                body: "bob's secret".into(),
            })
            .await
            .expect("seed bob");

        // Use DynamicModuleBuilder to apply the provider override BEFORE
        // controllers are registered, so the generated handlers get the real pool.
        let dynamic_module = nestrs::core::DynamicModuleBuilder::<PostsModule>::new()
            .override_provider::<__PostCrudState>(Arc::new(__PostCrudState::from_pool(pool)))
            .build();
        let app = NestApplication::from_registry_and_router(
            std::sync::Arc::new(dynamic_module.registry),
            dynamic_module.router,
        );
        // Do NOT install middleware here — the test uses `with_ability`/
        // `with_principal` to set task-locals directly, which is the
        // intended pattern for unit tests. The middleware would shadow
        // the test's ability if both were present.
        app.into_router()
    }

    #[tokio::test]
    async fn author_predicate_blocks_read_of_others_row() {
        let router = build_authz_router().await;
        let ability = alice_only_ability();
        let principal = alice_principal();
        // Both the Ability and the Principal must be in the task-local
        // scope when the request runs — `AuthorIsCurrentUser` reads
        // both. `with_ability` / `with_principal` nest to mirror the
        // production middleware stack.
        with_principal(
            principal,
            with_ability(ability, async move {
                // Row 1 is alice's: 200.
                let res = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri("/posts/1")
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("serve");
                assert_eq!(res.status(), StatusCode::OK);
                // Row 2 is bob's: 404 (authz invisible-channel).
                let res2 = router
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri("/posts/2")
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("serve");
                assert_eq!(res2.status(), StatusCode::NOT_FOUND);
            }),
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Uninitialised state — the 500 path when no pool was injected
// ---------------------------------------------------------------------------

/// Boot the module WITHOUT `override_provider::<__PostCrudState>` — the
/// state falls back to its default constructor (`pool: None`) and every
/// handler must 500 with a message naming the *actual* generated state
/// type (regression: the message used to render the literal text
/// `__{Pascal}CrudState`, which is meaningless to the user).
#[tokio::test]
async fn uninitialised_state_500_names_the_real_state_type() {
    nestrs::core::clear_module_cache_for_tests();
    let dynamic_module = nestrs::core::DynamicModuleBuilder::<PostsModule>::new().build();
    let app = NestApplication::from_registry_and_router(
        std::sync::Arc::new(dynamic_module.registry),
        dynamic_module.router,
    );
    let ability = Arc::new(
        Ability::builder()
            .can(Action::Read, "posts")
            .build(),
    );
    let router = app
        .into_router()
        .layer(middleware::from_fn_with_state(ability, install_policies_middleware));

    let (status, body) = get(&router, "/posts/").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        body.contains("__PostCrudState"),
        "the 500 message must name the real generated state type, got: {body}"
    );
    assert!(
        !body.contains("{Pascal}"),
        "the 500 message must not contain the un-interpolated macro placeholder, got: {body}"
    );
    assert!(
        body.contains("override_provider"),
        "the 500 message should tell the user how to fix it, got: {body}"
    );
}
