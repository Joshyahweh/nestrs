#![cfg(all(feature = "ws-authz", feature = "database-sqlx"))]

//! `nestrs::ws_authz` integration tests — per-message ability + transaction
//! + outbound masking on a real `WsClient`.
//!
//! Mirrors the HTTP `masking_module` / `transactional_module` tests but
//! exercises the WebSocket transport.

use nestrs::ws::{WsClient, WsEvent, WsGateway, WsHandshake, WS_ERROR_EVENT};
use nestrs::{
    current_ws_ability, current_ws_principal, current_ws_transaction, emit_masked, run_in_ws_scope,
    Ability, Action, Subject, WsScope,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

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

/// In-memory mock client: the same shape as `WsClient` but exposes the
/// mpsc receiver so tests can assert on the emitted frame without
/// spinning up a real Axum route + WebSocketUpgrade.
fn mock_client() -> (
    WsClient,
    mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) {
    let (tx, rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();
    let client = WsClient::from_tx_for_test(tx, WsHandshake::default());
    (client, rx)
}

async fn recv_event(
    rx: &mut mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) -> WsEvent<Value> {
    let msg = rx.recv().await.expect("recv");
    let axum::extract::ws::Message::Text(text) = msg else {
        panic!("expected text frame");
    };
    serde_json::from_str(&text).expect("frame json")
}

#[tokio::test]
async fn emit_masked_strips_field_when_ability_excludes_it() {
    let (client, mut rx) = mock_client();
    let ability = ability_with_post_fields();
    let data = json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" });
    emit_masked(&client, "post", data, &ability).expect("emit");
    let frame = recv_event(&mut rx).await;
    assert_eq!(frame.event, "post");
    assert_eq!(frame.data.get("title").and_then(|v| v.as_str()), Some("hi"));
    assert!(
        frame.data.get("body").is_none(),
        "body should be masked away"
    );
    assert_eq!(frame.data.get("id").and_then(|v| v.as_i64()), Some(1));
}

#[tokio::test]
async fn emit_masked_keeps_field_when_ability_has_full_read() {
    let (client, mut rx) = mock_client();
    let ability = ability_with_all_fields();
    let data = json!({ "type": "Post", "id": 1, "title": "hi", "body": "ok" });
    emit_masked(&client, "post", data, &ability).expect("emit");
    let frame = recv_event(&mut rx).await;
    assert_eq!(frame.data.get("body").and_then(|v| v.as_str()), Some("ok"));
    assert_eq!(frame.data.get("title").and_then(|v| v.as_str()), Some("hi"));
}

#[tokio::test]
async fn emit_masked_passes_through_object_without_type_marker() {
    let (client, mut rx) = mock_client();
    let ability = ability_with_post_fields();
    let data = json!([{ "id": 1, "title": "a" }, { "id": 2, "title": "b" }]);
    emit_masked(&client, "list", data, &ability).expect("emit");
    let frame = recv_event(&mut rx).await;
    let items = frame.data.as_array().expect("array");
    assert_eq!(items.len(), 2);
    // No `type` marker => no masking decision => both items pass through.
    assert_eq!(items[0].get("title").and_then(|v| v.as_str()), Some("a"));
    assert_eq!(items[1].get("title").and_then(|v| v.as_str()), Some("b"));
}

#[tokio::test]
async fn run_in_ws_scope_makes_current_ability_visible() {
    let ability = ability_with_post_fields();
    let scope = WsScope::new().with_ability(ability.clone());
    run_in_ws_scope(scope, async move {
        let seen = current_ws_ability().expect("ability");
        assert!(Arc::ptr_eq(&seen, &ability));
    })
    .await;
}

#[tokio::test]
async fn run_in_ws_scope_current_ability_is_none_outside_scope() {
    let ability = ability_with_post_fields();
    let scope = WsScope::new().with_ability(ability);
    run_in_ws_scope(scope, async {
        // inside
        assert!(current_ws_ability().is_some());
    })
    .await;
    // outside the scope
    assert!(current_ws_ability().is_none());
}

// -- Row-level (Wave 3D): principal scope + row predicate over WS ---------------

#[tokio::test]
async fn ws_principal_scope_survives_and_row_predicate_applies() {
    let ability = Arc::new(
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
    );
    let principal = Arc::new(nestrs::policies::Principal {
        subject: "alice".into(),
        roles: vec![],
        claims: json!({}),
    });
    let scope = WsScope::new()
        .with_ability(ability.clone())
        .with_principal(principal);
    run_in_ws_scope(scope, async move {
        let p = current_ws_principal().expect("principal installed by run_in_ws_scope");
        assert_eq!(p.subject, "alice");
        let seen = current_ws_ability().expect("ability");
        let own = Subject::Instance(json!({ "type": "Post", "author": "alice" }));
        let other = Subject::Instance(json!({ "type": "Post", "author": "mallory" }));
        // Row predicate evaluated against the connection's principal.
        assert!(seen.can(&Action::Read, &own));
        assert!(!seen.can(&Action::Read, &other));
    })
    .await;
}

// -- sqlx ambient-tx path ----------------------------------------------------
//
// `run_in_ws_scope` opens a `TransactionSlot` from the pool if one is
// configured. Handlers can call `commit()` / `rollback()` explicitly. If
// they don't, sqlx rolls back when the slot drops.

async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "nestrs-ws-authz-{}-{}-{nanos}.sqlite",
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

#[tokio::test]
async fn ws_transaction_persists_when_handler_commits() {
    let pool = fresh_pool().await;
    let scope = WsScope::new().with_pool(pool.clone());
    run_in_ws_scope(scope, async {
        let slot = current_ws_transaction().expect("slot");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("alpha")
                        .bind(42_i64)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert");
        slot.commit().await.expect("commit");
    })
    .await;
    assert_eq!(read_kv(&pool, "alpha").await, Some(42));
}

#[tokio::test]
async fn ws_transaction_rolls_back_when_handler_does_not_commit() {
    let pool = fresh_pool().await;
    let scope = WsScope::new().with_pool(pool.clone());
    run_in_ws_scope(scope, async {
        let slot = current_ws_transaction().expect("slot");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("beta")
                        .bind(7_i64)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert");
        // Intentionally do not call commit — sqlx rolls back on drop.
    })
    .await;
    assert!(read_kv(&pool, "beta").await.is_none());
}

#[tokio::test]
async fn ws_current_transaction_is_none_when_pool_not_configured() {
    let scope = WsScope::new(); // no pool
    run_in_ws_scope(scope, async {
        assert!(current_ws_transaction().is_none());
        // And no ability either:
        assert!(current_ws_ability().is_none());
    })
    .await;
}

#[tokio::test]
async fn ws_scope_is_per_message_not_per_connection() {
    // Each run_in_ws_scope invocation opens its own slot. Two calls in
    // sequence don't see each other's state.
    let pool = fresh_pool().await;
    let scope1 = WsScope::new().with_pool(pool.clone());
    run_in_ws_scope(scope1, async {
        let slot = current_ws_transaction().expect("slot1");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("first")
                        .bind(1_i64)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert1");
        slot.commit().await.expect("commit1");
    })
    .await;

    let scope2 = WsScope::new().with_pool(pool.clone());
    run_in_ws_scope(scope2, async {
        // Different slot identity; no leakage from the first.
        let slot = current_ws_transaction().expect("slot2");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("second")
                        .bind(2_i64)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert2");
        slot.commit().await.expect("commit2");
    })
    .await;

    assert_eq!(read_kv(&pool, "first").await, Some(1));
    assert_eq!(read_kv(&pool, "second").await, Some(2));
}

// -- A WsGateway impl that exercises the per-message scope plumbing ----------
//
// This is the realistic end-to-end pattern: a user's gateway handler
// reads `current_ws_ability()` / `current_ws_transaction()` and uses
// `emit_masked` for outbound frames. The test wires the scope around a
// manual `gateway.on_message` call (mimicking what `serve_socket` does
// per inbound frame).

struct PostGateway {
    ability: Arc<Ability>,
    pool: Arc<sqlx::AnyPool>,
}

#[async_trait::async_trait]
impl WsGateway for PostGateway {
    async fn on_message(&self, client: WsClient, _event: &str, _payload: Value) {
        let scope = WsScope::new()
            .with_ability(self.ability.clone())
            .with_pool(self.pool.clone());
        run_in_ws_scope(scope, async {
            let ability = current_ws_ability().expect("ability inside gateway");
            let slot = current_ws_transaction().expect("tx inside gateway");

            // Persist something
            let result: Result<(), sqlx::Error> = slot
                .with_tx(|tx| {
                    Box::pin(async move {
                        sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                            .bind("gateway")
                            .bind(99_i64)
                            .execute(&mut **tx)
                            .await?;
                        Ok(())
                    })
                })
                .await;
            result.expect("insert");
            slot.commit().await.expect("commit");

            // Emit a masked frame
            let data = json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" });
            emit_masked(&client, "post", data, &ability).expect("emit");
        })
        .await;
    }
}

#[tokio::test]
async fn gateway_with_post_fields_masks_and_commits() {
    let pool = fresh_pool().await;
    let ability = ability_with_post_fields();
    let gateway = Arc::new(PostGateway {
        ability: ability.clone(),
        pool: pool.clone(),
    });

    // Simulate a single inbound message by calling on_message directly.
    let (client, mut rx) = mock_client();
    gateway.on_message(client, "any", json!({})).await;

    // 1. The masked frame arrived.
    let frame = recv_event(&mut rx).await;
    assert_eq!(frame.event, "post");
    assert!(frame.data.get("body").is_none(), "body masked");
    assert_eq!(frame.data.get("title").and_then(|v| v.as_str()), Some("hi"));

    // 2. The transaction committed.
    assert_eq!(read_kv(&pool, "gateway").await, Some(99));
}

#[tokio::test]
async fn gateway_with_full_read_keeps_all_fields() {
    let pool = fresh_pool().await;
    let ability = ability_with_all_fields();
    let gateway = Arc::new(PostGateway {
        ability,
        pool: pool.clone(),
    });
    let (client, mut rx) = mock_client();
    gateway.on_message(client, "any", json!({})).await;
    let frame = recv_event(&mut rx).await;
    assert_eq!(
        frame.data.get("body").and_then(|v| v.as_str()),
        Some("secret")
    );
}

// The WsGateway impl uses WS_ERROR_EVENT (re-exported from nestrs-ws) in
// the error path. Touch the import so it isn't dead-code.
#[allow(dead_code)]
fn _ensure_error_event_reachable() -> &'static str {
    WS_ERROR_EVENT
}
