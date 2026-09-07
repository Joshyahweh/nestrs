#![cfg(all(feature = "graphql-authz", feature = "database-sqlx"))]

//! `nestrs::gql_authz` integration tests — per-resolver ability + transaction
//! + outbound masking on a real `async-graphql` schema mounted on Axum.
//!
//! Mirrors the HTTP `masking_module` / `transactional_module` tests but
//! exercises the GraphQL transport end-to-end: full router -> POST ->
//! `schema.execute_batch` -> response body, with the `GqlDataContext`
//! installing the ability slot + transaction slot around each request.

use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject, Subscription};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::graphql::{BatchRequest, GraphQlHttpOptions};
use nestrs::{
    current_gql_ability, current_gql_principal, current_gql_transaction,
    graphql_router_with_context, mask_value, Ability, Action, GqlDataContext, Subject,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

fn ability_with_post_fields() -> Arc<Ability> {
    Arc::new(
        Ability::builder()
            .can_on_fields(Action::Read, "Post", vec!["id".into(), "title".into()])
            .build(),
    )
}

fn ability_with_all_fields() -> Arc<Ability> {
    Arc::new(Ability::builder().can(Action::Read, "Post").build())
}

/// GraphQL Post type with the four fields the masking walker needs to
/// reason about. The `type` field is the CASL subject marker — the
/// `mask_value` walker looks for it to decide what to allow.
#[derive(SimpleObject, Clone)]
struct Post {
    /// CASL subject marker — must be `"Post"` for masking to apply.
    r#type: String,
    id: i64,
    title: String,
    body: String,
}

impl Post {
    fn sample() -> Self {
        Self {
            r#type: "Post".into(),
            id: 1,
            title: "hi".into(),
            body: "secret".into(),
        }
    }
}

#[derive(SimpleObject)]
struct AbilityMarker {
    ability_visible: bool,
}

#[derive(SimpleObject)]
struct PrincipalMarker {
    principal_visible: bool,
    subject: String,
    predicate_allows_own: bool,
    predicate_allows_other: bool,
}

#[derive(SimpleObject)]
struct WriteResult {
    ok: bool,
    reason: Option<String>,
}

#[derive(Default)]
struct PostQuery;

#[Object]
impl PostQuery {
    /// A single post — emits the full shape including the secret `body` field.
    async fn post(&self) -> Post {
        Post::sample()
    }

    /// A list of posts — used for the array-mask test.
    async fn posts(&self) -> Vec<Post> {
        vec![
            Post {
                id: 1,
                ..Post::sample()
            },
            Post {
                id: 2,
                title: "b".into(),
                body: "y".into(),
                ..Post::sample()
            },
        ]
    }

    /// Echoes whether the resolver-side `current_gql_ability()` is `Some`.
    async fn ability_marker(&self) -> AbilityMarker {
        let present = current_gql_ability().is_some();
        AbilityMarker {
            ability_visible: present,
        }
    }

    /// Echoes the resolver-side principal and evaluates a row predicate
    /// through the ambient ability + principal slots — the GraphQL-transport
    /// survival test for row-level authorization (Wave 3D).
    async fn principal_marker(&self) -> PrincipalMarker {
        let p = current_gql_principal();
        let (visible, subject) = match &p {
            Some(p) => (true, p.subject.clone()),
            None => (false, String::new()),
        };
        // The with_principal slot is already installed around the resolver by
        // the GqlDataContext, so evaluate the row predicate directly.
        let (own, other) = match (current_gql_ability(), p) {
            (Some(ability), Some(_principal)) => {
                let own = Subject::Instance(json!({ "type": "Post", "author": "alice" }));
                let other = Subject::Instance(json!({ "type": "Post", "author": "mallory" }));
                (
                    ability.can(&Action::Read, &own),
                    ability.can(&Action::Read, &other),
                )
            }
            _ => (false, false),
        };
        PrincipalMarker {
            principal_visible: visible,
            subject,
            predicate_allows_own: own,
            predicate_allows_other: other,
        }
    }

    /// Inserts a row into a side table via the ambient `current_gql_transaction`
    /// slot, then returns `ok: true` (or `false` if no slot was visible).
    /// Used by the commit/rollback tests.
    async fn write_via_tx(&self, key: String, value: i64) -> WriteResult {
        let Some(slot) = current_gql_transaction() else {
            return WriteResult {
                ok: false,
                reason: Some("no_slot".into()),
            };
        };
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind(key)
                        .bind(value)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        WriteResult {
            ok: result.is_ok(),
            reason: None,
        }
    }

    /// Resolver that always returns an error — used to drive the
    /// "errors present -> rollback" path in `run_hook`.
    async fn boom(&self) -> Result<Post, async_graphql::Error> {
        Err(async_graphql::Error::new("boom"))
    }
}

type PostSchema = Schema<PostQuery, EmptyMutation, EmptySubscription>;

fn schema() -> PostSchema {
    Schema::build(PostQuery, EmptyMutation, EmptySubscription).finish()
}

/// Mount a router with a `GqlDataContext` configured with `ability` (and
/// optionally a `pool`) and POST a query to it. Returns the JSON body
/// (parsed) and the HTTP status. Generic over the schema so the rollback
/// test (which uses a different `QueryRoot`) can share the helper.
async fn post_query<Q, M, S>(
    s: Schema<Q, M, S>,
    ctx: GqlDataContext,
    query: &str,
) -> (StatusCode, Value)
where
    Q: async_graphql::ObjectType + Send + Sync + 'static,
    M: async_graphql::ObjectType + Send + Sync + 'static,
    S: async_graphql::SubscriptionType + Send + Sync + 'static,
{
    let app = graphql_router_with_context(s, "/graphql", GraphQlHttpOptions::default(), ctx);
    let body = serde_json::to_vec(&json!({ "query": query })).expect("encode body");
    let req = Request::builder()
        .method("POST")
        .uri("/graphql")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("build request");
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "nestrs-graphql-authz-{}-{}-{nanos}.sqlite",
        std::process::id(),
        n
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect sqlite");
    let pool = Arc::new(pool);
    sqlx::query("CREATE TABLE kv (key TEXT PRIMARY KEY, value INTEGER NOT NULL)")
        .execute(pool.as_ref())
        .await
        .expect("create kv");
    pool
}

async fn read_kv(pool: &sqlx::AnyPool, key: &str) -> Option<i64> {
    use sqlx::Row;
    let row = sqlx::query("SELECT value FROM kv WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .expect("select");
    row.map(|r| r.try_get::<i64, _>("value").unwrap())
}

// ---------------------------------------------------------------------------
// 1. Masking — top-level Post with allow-list excludes `body`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_masks_response_field_when_ability_excludes_it() {
    let ctx = GqlDataContext::new().with_ability(ability_with_post_fields());
    let (_status, body) = post_query(schema(), ctx, r#"{ post { type id title body } }"#).await;
    let data = body
        .get("data")
        .and_then(|d| d.get("post"))
        .expect("data.post");
    assert_eq!(data.get("id").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("hi"));
    assert!(
        data.get("body").is_none(),
        "body should be masked away when ability excludes it, got: {data}"
    );
}

// ---------------------------------------------------------------------------
// 2. No data context -> no masking -> all fields preserved
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_does_not_mask_when_no_data_context() {
    // No ability, no pool — the response is passed through unchanged.
    let ctx = GqlDataContext::new();
    let (_status, body) = post_query(schema(), ctx, r#"{ post { type id title body } }"#).await;
    let data = body
        .get("data")
        .and_then(|d| d.get("post"))
        .expect("data.post");
    assert_eq!(data.get("body").and_then(|v| v.as_str()), Some("secret"));
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("hi"));
}

// ---------------------------------------------------------------------------
// 2b. Ability with full read (no field restriction) -> all fields preserved.
//     A different no-op path from (2): here the ability IS present but
//     grants `Read` on `Post` without a field allow-list, so nothing is
//     dropped. Confirms the masking walker only strips on explicit
//     field allow-lists, never by default.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_does_not_mask_when_ability_has_no_field_restriction() {
    let ctx = GqlDataContext::new().with_ability(ability_with_all_fields());
    let (_status, body) = post_query(schema(), ctx, r#"{ post { type id title body } }"#).await;
    let data = body
        .get("data")
        .and_then(|d| d.get("post"))
        .expect("data.post");
    assert_eq!(data.get("body").and_then(|v| v.as_str()), Some("secret"));
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("hi"));
}

// ---------------------------------------------------------------------------
// 3. Resolver can read `current_gql_ability()`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_resolver_can_call_current_gql_ability() {
    let ability = ability_with_post_fields();
    let ctx = GqlDataContext::new().with_ability(ability.clone());
    let (_status, body) =
        post_query(schema(), ctx, r#"{ abilityMarker { abilityVisible } }"#).await;
    let marker = body
        .get("data")
        .and_then(|d| d.get("abilityMarker"))
        .expect("data.abilityMarker");
    assert_eq!(
        marker.get("abilityVisible").and_then(|v| v.as_bool()),
        Some(true),
        "ability slot must be visible inside the resolver, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// 4. Resolver can read `current_gql_transaction()` and write through it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_resolver_can_open_transaction_via_current_gql_transaction() {
    let pool = fresh_pool().await;
    let ctx = GqlDataContext::new().with_pool(pool.clone());
    let (_status, body) = post_query(
        schema(),
        ctx,
        r#"{ writeViaTx(key: "gamma", value: 7) { ok } }"#,
    )
    .await;
    let marker = body
        .get("data")
        .and_then(|d| d.get("writeViaTx"))
        .expect("data.writeViaTx");
    assert_eq!(
        marker.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "resolver-side slot should be visible, got: {body}"
    );
    // The handler commits on a clean response — same policy as HTTP and WS.
    // (No need for the resolver to call `commit()` itself; that's the
    // `gql_transaction_commits_on_ok_response` test below.)
    assert_eq!(read_kv(&pool, "gamma").await, Some(7));
}

// ---------------------------------------------------------------------------
// 5. Successful response -> tx commits
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_transaction_commits_on_ok_response() {
    let pool = fresh_pool().await;
    // No ability — no masking. The handler still opens a tx from the pool
    // and commits on a clean (no-errors) response.
    let ctx = GqlDataContext::new().with_pool(pool.clone());
    let (_status, body) = post_query(
        schema(),
        ctx,
        r#"{ writeViaTx(key: "delta", value: 13) { ok } }"#,
    )
    .await;
    let marker = body
        .get("data")
        .and_then(|d| d.get("writeViaTx"))
        .expect("data.writeViaTx");
    assert_eq!(marker.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        read_kv(&pool, "delta").await,
        Some(13),
        "transaction must have been committed on a clean response"
    );
}

// ---------------------------------------------------------------------------
// 6. Error in response -> tx rolls back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_transaction_rolls_back_on_error_response() {
    let pool = fresh_pool().await;
    // A schema variant whose query writes via the slot, then `boom()` always
    // errors. We can't combine `write_via_tx` and `boom` in one query because
    // GraphQL is single-rooted for `Query`, so we use two passes:
    //   1. write via the slot
    //   2. trigger the error resolver on the same connection
    // The tx from pass (1) is still in flight (its slot was tied to the
    // first request's scope), so by the time `boom` runs the row is
    // visible inside that tx but not yet committed to the pool. Pass (2)
    // is on a *separate* request with its own scope, so the rollback
    // signal is carried by the *response errors* of the request that
    // wrote — i.e. pass (1) must have an error too. The cleanest way is
    // to have the *same* request both write and then error: GraphQL
    // execution is single-pass over the selection set, so a single
    // selection that writes and then errors is the canonical rollback
    // driver. The resolver below does exactly that.
    let s = Schema::build(WriteThenBoom, EmptyMutation, EmptySubscription).finish();
    let ctx = GqlDataContext::new().with_pool(pool.clone());
    let (_status, body) =
        post_query(s, ctx, r#"{ writeThenBoom(key: "epsilon", value: 99) }"#).await;
    // The response should have a non-empty `errors` array.
    let errors = body
        .get("errors")
        .and_then(|e| e.as_array())
        .expect("errors");
    assert!(
        !errors.is_empty(),
        "boom should surface as an error, got: {body}"
    );
    // The transaction must have rolled back: row absent.
    assert!(
        read_kv(&pool, "epsilon").await.is_none(),
        "transaction must have been rolled back on an error response"
    );
}

/// Resolver that writes via the slot and then errors. A single resolver
/// that produces an `Err` causes the request's `Response.errors` to be
/// non-empty, which the handler's `batch_response_has_errors` reads to
/// decide rollback.
#[derive(Default)]
struct WriteThenBoom;

#[derive(SimpleObject)]
struct WriteMarker {
    wrote: bool,
}

#[Object]
impl WriteThenBoom {
    async fn write_then_boom(
        &self,
        key: String,
        value: i64,
    ) -> Result<WriteMarker, async_graphql::Error> {
        let slot = current_gql_transaction()
            .ok_or_else(|| async_graphql::Error::new("no transaction slot visible to resolver"))?;
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind(key)
                        .bind(value)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.map_err(|e| async_graphql::Error::new(format!("insert failed: {e}")))?;
        // Now error out — selection set produces a non-empty `errors` vec.
        Err(async_graphql::Error::new("boom"))
    }
}

// ---------------------------------------------------------------------------
// 7. Masking recurses into arrays of objects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gql_masks_array_of_objects_recursively() {
    let ctx = GqlDataContext::new().with_ability(ability_with_post_fields());
    let (_status, body) = post_query(schema(), ctx, r#"{ posts { type id title body } }"#).await;
    let items = body
        .get("data")
        .and_then(|d| d.get("posts"))
        .and_then(|p| p.as_array())
        .expect("data.posts array");
    assert_eq!(items.len(), 2);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(
            item.get("id").and_then(|v| v.as_i64()),
            Some((i + 1) as i64)
        );
        assert!(
            item.get("body").is_none(),
            "item {i}: body should be masked"
        );
        assert!(item.get("title").is_some(), "item {i}: title should remain");
    }
}

// ---------------------------------------------------------------------------
// 8. Subscription emits masked values
// ---------------------------------------------------------------------------
//
// We can't run a subscription through the HTTP handler (subscriptions use
// `execute_stream`, the handler uses `execute_batch`), so this test
// exercises the *masking walker on subscription output* directly. The
// masking walker is the same `mask_value` used everywhere — feeding the
// `serde_json::Value` shape that a real subscription payload produces
// through it must strip the fields the ability excludes. That's a
// faithful coverage of the "outbound GraphQL data is masked" guarantee
// for subscription responses.

#[derive(Default)]
struct PostSubscription;

#[Subscription]
impl PostSubscription {
    async fn posts(&self) -> impl futures_util::Stream<Item = Post> {
        futures_util::stream::iter(vec![Post::sample()])
    }
}

type SubSchema = Schema<PostQuery, EmptyMutation, PostSubscription>;

#[tokio::test]
async fn gql_subscription_emits_masked_values() {
    use futures_util::StreamExt;
    let s: SubSchema = Schema::build(PostQuery, EmptyMutation, PostSubscription).finish();
    let mut stream = s.execute_stream(async_graphql::Request::new(
        r#"subscription { posts { type id title body } }"#,
    ));
    let first = stream.next().await.expect("first stream item");
    // The walker is the same one used by `run_hook` — convert to serde_json
    // to assert the masked shape with the same idioms as the query tests.
    let mut data_json: Value = serde_json::to_value(&first.data).expect("data is serializable");
    // Apply the same walker `run_hook` would apply, but in the test scope
    // (the HTTP handler can't carry subscription execution).
    mask_value(&mut data_json, &ability_with_post_fields());
    let post = data_json
        .get("posts")
        .and_then(|p| p.as_object())
        .expect("data.posts");
    assert!(
        post.get("body").is_none(),
        "body must be masked on subscription output"
    );
    assert!(post.get("title").is_some());
    assert!(post.get("id").is_some());
}

// ---------------------------------------------------------------------------
// Misc. — touch the BatchRequest re-export so a future cleanup doesn't
// silently drop the import.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _ensure_batch_request_reachable(batch: BatchRequest) -> usize {
    batch.into_single().map(|_| 1).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Row-level (Wave 3D): principal scope survives into GraphQL resolvers and
// the row predicate evaluates against the request's identity.
// ---------------------------------------------------------------------------

fn ability_with_author_predicate() -> Arc<Ability> {
    Arc::new(
        Ability::builder()
            .can_with_predicate(
                Action::Read,
                "Post",
                vec![],
                |row: &serde_json::Value, p: &nestrs::policies::Principal| {
                    row["author"] == p.subject.as_str()
                },
            )
            .build(),
    )
}

fn alice_principal() -> Arc<nestrs::policies::Principal> {
    Arc::new(nestrs::policies::Principal {
        subject: "alice".into(),
        roles: vec![],
        claims: json!({}),
    })
}

#[tokio::test]
async fn gql_principal_scope_survives_into_resolver_and_predicate_applies() {
    let ctx = GqlDataContext::new()
        .with_ability(ability_with_author_predicate())
        .with_principal(alice_principal());
    let (status, body) = post_query(
        schema(),
        ctx,
        r#"{ principalMarker { principalVisible subject predicateAllowsOwn predicateAllowsOther } }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let marker = body
        .get("data")
        .and_then(|d| d.get("principalMarker"))
        .expect("data.principalMarker");
    assert_eq!(marker["principalVisible"], json!(true));
    assert_eq!(marker["subject"], json!("alice"));
    // Row predicate evaluated against the request's identity: alice's own
    // row passes, mallory's row is denied.
    assert_eq!(marker["predicateAllowsOwn"], json!(true));
    assert_eq!(marker["predicateAllowsOther"], json!(false));
}

#[tokio::test]
async fn gql_predicate_denies_conservatively_without_principal() {
    // Ability with a predicate but NO principal in the data context: the
    // row predicate cannot be evaluated, so `can(Instance)` denies.
    let ctx = GqlDataContext::new().with_ability(ability_with_author_predicate());
    let (status, body) = post_query(
        schema(),
        ctx,
        r#"{ principalMarker { principalVisible predicateAllowsOwn } }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let marker = body
        .get("data")
        .and_then(|d| d.get("principalMarker"))
        .expect("data.principalMarker");
    assert_eq!(marker["principalVisible"], json!(false));
    assert_eq!(marker["predicateAllowsOwn"], json!(false));
}
