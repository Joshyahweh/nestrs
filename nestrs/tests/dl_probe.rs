#![cfg(all(feature = "graphql-authz", feature = "graphql-dataloader"))]
use async_graphql::SimpleObject;
use nestrs::dataloader;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, SimpleObject)]
struct User {
    id: i64,
}

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
        self.keys_seen.lock().unwrap().extend_from_slice(keys);
        let failing = self.fail_on.lock().unwrap().clone();
        if keys.iter().any(|k| failing.contains(k)) {
            return Err(nestrs::graphql::Error::new("batch_load failed"));
        }
        Ok(keys.iter().map(|k| (*k, User { id: *k })).collect())
    }
}

#[tokio::test]
async fn probe_concurrent_errors() {
    let loader = UserLoader {
        fail_on: Arc::new(Mutex::new([7i64, 8].into())),
        ..Default::default()
    };
    let dl = nestrs::graphql::data_loader(loader.clone());
    let (a, b) = tokio::join!(dl.load_one(7), dl.load_one(8));
    println!(
        "a={a:?} b={b:?} calls={}",
        loader.calls.load(Ordering::SeqCst)
    );
    assert!(a.is_err());
    assert!(b.is_err());
}

#[tokio::test]
async fn probe_mixed_success_failure() {
    let loader = UserLoader {
        fail_on: Arc::new(Mutex::new([7i64].into())),
        ..Default::default()
    };
    let dl = nestrs::graphql::data_loader(loader.clone());
    let (a, b) = tokio::join!(dl.load_one(1), dl.load_one(7));
    println!(
        "a={a:?} b={b:?} calls={}",
        loader.calls.load(Ordering::SeqCst)
    );
}

struct ProbeQuery;

#[async_graphql::Object]
impl ProbeQuery {
    async fn user(
        &self,
        ctx: &async_graphql::Context<'_>,
        id: i64,
    ) -> async_graphql::Result<Option<User>> {
        match ctx.data::<nestrs::graphql::DataLoader<UserLoader>>() {
            Ok(loader) => Ok(loader.load_one(id).await?),
            Err(_) => Ok(None),
        }
    }
}

#[tokio::test]
async fn probe_executor_two_failing_fields() {
    let loader = UserLoader {
        fail_on: Arc::new(Mutex::new([7i64, 8].into())),
        ..Default::default()
    };
    let mut request = nestrs::graphql::Request::new(
        "{ u1: user(id: 7) { id } u2: user(id: 8) { id } }".to_string(),
    );
    request
        .data
        .insert(nestrs::graphql::data_loader(loader.clone()));
    let resp = async_graphql::Schema::new(
        ProbeQuery,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .execute(request)
    .await;
    println!("errors={:?} data={:?}", resp.errors, resp.data);
}

struct DirectQuery;

#[async_graphql::Object]
impl DirectQuery {
    async fn bad(&self, id: i64) -> async_graphql::Result<Option<String>> {
        let _ = id;
        Err(async_graphql::Error::new("direct fail"))
    }
}

#[tokio::test]
async fn probe_direct_errors_no_loader() {
    let resp = async_graphql::Schema::new(
        DirectQuery,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .execute("{ u1: bad(id: 7) u2: bad(id: 8) }")
    .await;
    println!("direct errors={:?} data={:?}", resp.errors, resp.data);
}

#[tokio::test]
async fn probe_router_mixed_body() {
    let loader = UserLoader {
        fail_on: Arc::new(Mutex::new([7i64].into())),
        ..Default::default()
    };
    let mut request = nestrs::graphql::Request::new(
        "{ ping: pingOne ok: user(id: 1) { id } bad: user(id: 7) { id } }".to_string(),
    );
    request
        .data
        .insert(nestrs::graphql::data_loader(loader.clone()));
    let resp = async_graphql::Schema::new(
        ProbeQuery2,
        async_graphql::EmptyMutation,
        async_graphql::EmptySubscription,
    )
    .execute(request)
    .await;
    println!("mixed errors={:?} data={:?}", resp.errors, resp.data);
}

struct ProbeQuery2;

#[async_graphql::Object]
impl ProbeQuery2 {
    async fn ping_one(&self) -> &'static str {
        "pong"
    }
    async fn user(
        &self,
        ctx: &async_graphql::Context<'_>,
        id: i64,
    ) -> async_graphql::Result<Option<User>> {
        match ctx.data::<nestrs::graphql::DataLoader<UserLoader>>() {
            Ok(loader) => Ok(loader.load_one(id).await?),
            Err(_) => Ok(None),
        }
    }
}
