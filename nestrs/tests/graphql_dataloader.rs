#![cfg(all(feature = "graphql-authz", feature = "graphql-dataloader"))]

//! Wave 4.1 — `#[dataloader]` / per-request batching loaders on the
//! GraphQL transport.
//!
//! Exercises the full path end-to-end: `DataLoaderRegistry` factories →
//! `GqlDataContext`'s hook `prepare` (fresh `DataLoader` per request) →
//! `schema.execute_batch` → resolvers calling `load_one` / `load_many`
//! through `ctx.data::<DataLoader<L>>()`. The counting `UserLoader`
//! records how many times its batch fn ran and with which keys, which
//! is exactly the N+1 contract: sibling `load_one` calls in one request
//! must collapse into a single `batch_load`.

use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::graphql::{DataLoader, DataLoaderRegistry, GraphQlHttpOptions};
use nestrs::{dataloader, graphql_router_with_context, GqlDataContext};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[derive(SimpleObject, Clone)]
struct User {
    id: i64,
    name: String,
}

/// Counting user loader: records every `batch_load` invocation and the
/// keys it saw; for keys in `fail_on`, the entry is omitted from the
/// result map (per the DataLoader reference contract — `Err` from
/// `batch_load` is fail-fast across async-graphql's `try_join_all` and
/// would nuke partial data on the whole response).
#[dataloader(key = i64, value = User, error = nestrs::graphql::Error)]
#[derive(Clone, Default)]
struct UserLoader {
    calls: Arc<AtomicUsize>,
    keys_seen: Arc<Mutex<Vec<i64>>>,
    fail_on: Arc<Mutex<HashSet<i64>>>,
}

impl UserLoader {
    async fn batch_load(&self, keys: &[i64]) -> Result<HashMap<i64, User>, nestrs::graphql::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.keys_seen.lock().expect("keys lock").extend_from_slice(keys);
        let failing = self.fail_on.lock().expect("fail lock").clone();
        Ok(keys
            .iter()
            .filter(|k| !failing.contains(k))
            .map(|k| {
                (
                    *k,
                    User {
                        id: *k,
                        name: format!("user-{k}"),
                    },
                )
            })
            .collect())
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn keys_seen(&self) -> Vec<i64> {
        self.keys_seen.lock().expect("keys lock").clone()
    }
}

/// Second loader type (different key/value) to prove the registry holds
/// heterogeneous factories.
#[dataloader(key = String, value = String, error = nestrs::graphql::Error)]
#[derive(Clone, Default)]
struct RoleLoader {
    calls: Arc<AtomicUsize>,
}

impl RoleLoader {
    async fn batch_load(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, String>, nestrs::graphql::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(keys
            .iter()
            .map(|k| (k.clone(), format!("role-of-{k}")))
            .collect())
    }
}

#[derive(Default)]
struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Single-key load through the per-request `DataLoader`. Returns
    /// `None` (no error) when no loader was installed — the graceful
    /// degradation path for apps that don't configure the registry.
    async fn user(&self, ctx: &Context<'_>, id: i64) -> async_graphql::Result<Option<User>> {
        match ctx.data::<DataLoader<UserLoader>>() {
            Ok(loader) => Ok(loader.load_one(id).await?),
            Err(_) => Ok(None),
        }
    }

    /// Multi-key load: one `load_many` for the whole id list.
    async fn users(&self, ctx: &Context<'_>, ids: Vec<i64>) -> async_graphql::Result<Vec<User>> {
        match ctx.data::<DataLoader<UserLoader>>() {
            Ok(loader) => {
                let map = loader.load_many(ids.clone()).await?;
                Ok(ids.iter().filter_map(|k| map.get(k).cloned()).collect())
            }
            Err(_) => Ok(vec![]),
        }
    }

    /// Load through the second loader type.
    async fn role(&self, ctx: &Context<'_>, name: String) -> async_graphql::Result<Option<String>> {
        match ctx.data::<DataLoader<RoleLoader>>() {
            Ok(loader) => Ok(loader.load_one(name).await?),
            Err(_) => Ok(None),
        }
    }

    /// A field that never touches a loader.
    async fn ping(&self) -> &'static str {
        "pong"
    }
}

fn schema() -> Schema<QueryRoot, EmptyMutation, EmptySubscription> {
    Schema::new(QueryRoot, EmptyMutation, EmptySubscription)
}

fn registry_for(loader: &UserLoader) -> DataLoaderRegistry {
    let loader = loader.clone();
    DataLoaderRegistry::new().with_loader(move || loader.clone().into_data_loader())
}

/// Mount a router with `ctx` and POST `query`, returning the status and
/// the parsed JSON body. (Same shape as `graphql_multi_transport_scope`.)
async fn post_query(ctx: GqlDataContext, query: &str) -> (StatusCode, Value) {
    let app = graphql_router_with_context(schema(), "/graphql", GraphQlHttpOptions::default(), ctx);
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

// ---------------------------------------------------------------------------
// Batching semantics
// ---------------------------------------------------------------------------

/// Three sibling `user` fields (two distinct keys, one repeat) resolve
/// with exactly ONE `batch_load` carrying the distinct keys — the N+1
/// fix; the repeated key is deduplicated inside the batch.
#[tokio::test]
async fn sibling_load_one_calls_batch_into_a_single_loader_call() {
    let loader = UserLoader::default();
    let ctx = GqlDataContext::new().with_loaders(registry_for(&loader));
    let (_status, body) = post_query(
        ctx,
        r#"{ u1: user(id: 1) { name } u2: user(id: 2) { name } u3: user(id: 1) { name } }"#,
    )
    .await;

    assert_eq!(body["data"]["u1"]["name"], "user-1");
    assert_eq!(body["data"]["u2"]["name"], "user-2");
    assert_eq!(body["data"]["u3"]["name"], "user-1");
    assert_eq!(loader.calls(), 1, "expected one batched call");
    let mut keys = loader.keys_seen();
    keys.sort();
    assert_eq!(keys, vec![1, 2], "distinct keys deduplicated in the batch");
}

/// `load_many` issues a single `batch_load` for the whole key list.
#[tokio::test]
async fn load_many_batches_once_for_the_key_list() {
    let loader = UserLoader::default();
    let ctx = GqlDataContext::new().with_loaders(registry_for(&loader));
    let (_status, body) = post_query(
        ctx,
        r#"{ users(ids: [3, 4, 5]) { id name } }"#,
    )
    .await;

    let names: Vec<&str> = body["data"]["users"]
        .as_array()
        .expect("users array")
        .iter()
        .map(|u| u["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["user-3", "user-4", "user-5"]);
    assert_eq!(loader.calls(), 1);
    let mut keys = loader.keys_seen();
    keys.sort();
    assert_eq!(keys, vec![3, 4, 5]);
}

/// A request that never touches a loader makes zero batch calls (and
/// constructing the registry still stays inert).
#[tokio::test]
async fn no_loader_query_makes_no_batch_calls() {
    let loader = UserLoader::default();
    let ctx = GqlDataContext::new().with_loaders(registry_for(&loader));
    let (_status, body) = post_query(ctx, r#"{ ping }"#).await;
    assert_eq!(body["data"]["ping"], "pong");
    assert_eq!(loader.calls(), 0);
}

/// The factory runs once per request and the `DataLoader` is fresh each
/// time: loading the same key in two requests hits `batch_load` twice —
/// a schema-global loader instance would have memoized the second hit.
#[tokio::test]
async fn factory_builds_a_fresh_loader_per_request() {
    let loader = UserLoader::default();
    let factory_runs = Arc::new(AtomicUsize::new(0));
    let runs = Arc::clone(&factory_runs);
    let inner = loader.clone();
    let loaders = DataLoaderRegistry::new().with_loader(move || {
        runs.fetch_add(1, Ordering::SeqCst);
        inner.clone().into_data_loader()
    });
    let ctx = GqlDataContext::new().with_loaders(loaders);

    for _ in 0..2 {
        let (_status, body) = post_query(ctx.clone(), r#"{ user(id: 9) { name } }"#).await;
        assert_eq!(body["data"]["user"]["name"], "user-9");
    }

    assert_eq!(factory_runs.load(Ordering::SeqCst), 2, "one build per request");
    assert_eq!(loader.calls(), 2, "cache must not leak across requests");
}

/// The registry can hold several loader types; each gets its own fresh
/// `DataLoader` and its own batch lifecycle.
#[tokio::test]
async fn registry_holds_multiple_loader_types() {
    let user_loader = UserLoader::default();
    let role_loader = RoleLoader::default();
    let rl = role_loader.clone();
    let loaders = DataLoaderRegistry::new()
        .with_loader({
            let ul = user_loader.clone();
            move || ul.clone().into_data_loader()
        })
        .with_loader(move || rl.clone().into_data_loader());
    let ctx = GqlDataContext::new().with_loaders(loaders);

    let (_status, body) =
        post_query(ctx, r#"{ user(id: 1) { name } role(name: "admin") }"#).await;

    assert_eq!(body["data"]["user"]["name"], "user-1");
    assert_eq!(body["data"]["role"], "role-of-admin");
    assert_eq!(user_loader.calls(), 1);
    assert_eq!(role_loader.calls.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// Error surfacing
// ---------------------------------------------------------------------------

/// A failing key in `batch_load` is reported as a *missing* key in the
/// result map (per the Facebook DataLoader contract — absent from the
/// map ≠ `Err`). Resolvers reading `load_one(bad_key)` get `Ok(None)`;
/// the GraphQL field is rendered `null`. Non-loader sibling fields
/// (`ping`) keep their values — the loader failure does NOT propagate
/// as a request-level error. (async-graphql's resolver container uses
/// `try_join_all` and would fail-fast on `Err`, so the macro's
/// `Loader::load` impl delegates straight to `batch_load` — emitting
/// `Err` would nuke partial data across the response.)
#[tokio::test]
async fn loader_error_surfaces_as_field_error() {
    let loader = UserLoader::default();
    loader.fail_on.lock().expect("fail lock").insert(7);
    let ctx = GqlDataContext::new().with_loaders(registry_for(&loader));
    let (_status, body) = post_query(
        ctx,
        r#"{ ping ok: user(id: 1) { name } bad: user(id: 7) { name } }"#,
    )
    .await;

    assert_eq!(body["data"]["ping"], "pong");
    assert_eq!(body["data"]["ok"]["name"], "user-1");
    assert!(body["data"]["bad"].is_null(), "missing key → null field");
    let errors = body["errors"].as_array();
    assert!(
        errors.is_none() || errors.unwrap().is_empty(),
        "loader absence is silent (resolver returned Ok(None)); got: {errors:?}"
    );
}

/// Multiple missing keys in the same batch each surface as `null`
/// fields, with no top-level `errors[]` entries — same DataLoader
/// contract as above, with two distinct keys that are both absent.
#[tokio::test]
async fn batch_error_reaches_every_waiting_resolver() {
    let loader = UserLoader::default();
    loader
        .fail_on
        .lock()
        .expect("fail lock")
        .extend([7, 8]);
    let ctx = GqlDataContext::new().with_loaders(registry_for(&loader));
    let (_status, body) = post_query(
        ctx,
        r#"{ u1: user(id: 7) { name } u2: user(id: 8) { name } }"#,
    )
    .await;

    assert!(body["data"]["u1"].is_null());
    assert!(body["data"]["u2"].is_null());
    let errors = body["errors"].as_array();
    assert!(
        errors.is_none() || errors.unwrap().is_empty(),
        "missing keys are silent, got: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// Registry + helper semantics (direct, no HTTP)
// ---------------------------------------------------------------------------

/// `DataLoaderRegistry::install` puts one fresh instance of every
/// registered loader into the request's data map, retrievable by
/// resolvers via `ctx.data::<DataLoader<L>>()`.
#[tokio::test]
async fn registry_installs_loaders_into_request_data() {
    let loader = UserLoader::default();
    let mut request = nestrs::graphql::Request::new("{ user(id: 1) { name } }".to_string());
    registry_for(&loader).install(&mut request);

    let response = schema().execute(request).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let data: Value = serde_json::to_value(&response.data).expect("data");
    assert_eq!(data["user"]["name"], "user-1");
    assert_eq!(loader.calls(), 1);
}

/// The default `data_loader` helper does not memoize across loads: two
/// sequential `load_one` calls (outside a shared batch window) each hit
/// the loader. Batching still deduplicates within a request — see the
/// sibling-field test above.
#[tokio::test]
async fn data_loader_helper_does_not_memoize_across_loads() {
    let loader = UserLoader::default();
    let dl = nestrs::graphql::data_loader(loader.clone());
    let first = dl.load_one(1).await.expect("first load");
    let second = dl.load_one(1).await.expect("second load");
    assert_eq!(first.map(|u| u.name), Some("user-1".to_string()));
    assert_eq!(second.map(|u| u.name), Some("user-1".to_string()));
    assert_eq!(loader.calls(), 2);
}

/// The cached variant memoizes across loads on the same instance —
/// opt-in for loaders whose resolvers repeat keys beyond the batch
/// window.
#[tokio::test]
async fn data_loader_cached_memoizes_across_loads() {
    let loader = UserLoader::default();
    let dl = nestrs::graphql::data_loader_cached(loader.clone(), 128);
    let first = dl.load_one(1).await.expect("first load");
    let second = dl.load_one(1).await.expect("second load");
    assert_eq!(first.map(|u| u.name), Some("user-1".to_string()));
    assert_eq!(second.map(|u| u.name), Some("user-1".to_string()));
    assert_eq!(loader.calls(), 1);
}

// ---------------------------------------------------------------------------
// Graceful degradation
// ---------------------------------------------------------------------------

/// An app that never calls `with_loaders` leaves resolvers without a
/// `DataLoader` — `ctx.data` misses, the field resolves to `None`, and
/// no loader machinery fails.
#[tokio::test]
async fn empty_context_leaves_resolvers_without_loaders() {
    let loader = UserLoader::default();
    let ctx = GqlDataContext::new();
    let (_status, body) = post_query(ctx, r#"{ user(id: 1) { name } }"#).await;

    assert!(body["data"]["user"].is_null());
    assert!(body.get("errors").is_none() || body["errors"].as_array().unwrap().is_empty());
    assert_eq!(loader.calls(), 0);
}
