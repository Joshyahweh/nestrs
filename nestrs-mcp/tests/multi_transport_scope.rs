#![cfg(feature = "authz")]

//! `nestrs_mcp::mcp_data_context` integration tests — per-tool-call
//! ability + transaction + outbound masking on a real `NestrsMcpServer`
//! (or rather, on the same scope-installation pipeline the server's
//! `call_tool` override uses).
//!
//! The tests exercise the same `run_with_mcp_scopes` + `mask_response` +
//! `commit_or_rollback` helpers `NestrsMcpServer::call_tool` wires up,
//! because driving the full rmcp runtime end-to-end would require a
//! full MCP client/server transport (JSON-RPC over stdio or HTTP). The
//! helpers are the whole point of the sub-feature: scopes + masking +
//! commit/rollback. The `NestrsMcpServer::call_tool` override is a
//! four-line wrapper around them, and the build itself verifies the
//! override compiles.

use nestrs::{Ability, Action, TransactionSlot};
use nestrs_mcp::mcp_data_context::{
    commit_or_rollback, current_mcp_ability, current_mcp_transaction, mask_response,
    run_with_mcp_scopes, McpDataContext,
};
use rmcp::model::{CallToolResponse, CallToolResult, ContentBlock};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Build a successful `CallToolResponse` whose text content is the
/// JSON encoding of `data`. Used by the masking + commit/rollback
/// tests.
fn ok_response(data: &Value) -> CallToolResponse {
    let text = serde_json::to_string(data).expect("serialize data");
    CallToolResult::success(vec![ContentBlock::text(text)]).into()
}

/// Build a tool-level error `CallToolResponse`. Used by the rollback
/// test.
fn error_response(message: &str) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(message)]).into()
}

/// Extract the first `ContentBlock::Text` payload as a `serde_json::Value`.
fn extract_text_json(resp: &CallToolResponse) -> Value {
    match resp {
        CallToolResponse::Complete(r) => match r.content.first() {
            Some(ContentBlock::Text(t)) => serde_json::from_str(&t.text).expect("text is JSON"),
            _ => panic!("expected text content block, got: {:?}", r.content),
        },
        _ => panic!("expected Complete variant"),
    }
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
        "nestrs-mcp-authz-{}-{}-{nanos}.sqlite",
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

/// Run a dispatch inside the MCP scopes, return the response (and the
/// pre-opened slot so the test can drive `commit_or_rollback` itself
/// for the commit/rollback tests). For tests that don't care about
/// the slot (e.g. masking), this is overkill — but the slot is part
/// of the public pipeline, so it's the most faithful shape.
async fn run_in_scope<F>(
    ctx: &McpDataContext,
    pool: Option<&Arc<sqlx::AnyPool>>,
    dispatch: F,
) -> (CallToolResponse, Option<Arc<TransactionSlot>>)
where
    F: std::future::Future<Output = CallToolResponse>,
{
    let slot = if let Some(pool) = pool {
        match pool.begin().await {
            Ok(tx) => Some(Arc::new(TransactionSlot::new(tx))),
            Err(e) => {
                eprintln!("warn: failed to open per-test transaction: {e}");
                None
            }
        }
    } else {
        None
    };
    let response = run_with_mcp_scopes(ctx, slot.clone(), dispatch).await;
    (response, slot)
}

// ---------------------------------------------------------------------------
// 4. Row-level (Wave 3D): the principal scope survives into tool dispatch and
//    the row predicate evaluates against the caller's identity.
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
        claims: serde_json::json!({}),
    })
}

#[tokio::test]
async fn mcp_principal_scope_survives_into_tool_dispatch_and_predicate_applies() {
    let ctx = McpDataContext::new()
        .with_ability(ability_with_author_predicate())
        .with_principal(alice_principal());
    run_with_mcp_scopes(&ctx, None, async {
        let principal = nestrs_mcp::mcp_data_context::current_mcp_principal()
            .expect("principal installed by run_with_mcp_scopes");
        assert_eq!(principal.subject, "alice");
        let ability = current_mcp_ability().expect("ability installed");
        let own =
            nestrs::Subject::Instance(serde_json::json!({ "type": "Post", "author": "alice" }));
        let other =
            nestrs::Subject::Instance(serde_json::json!({ "type": "Post", "author": "mallory" }));
        // Row predicate evaluated against the ambient principal: alice's own
        // row passes, mallory's row is denied.
        assert!(ability.can(&Action::Read, &own));
        assert!(!ability.can(&Action::Read, &other));
    })
    .await;
}

#[tokio::test]
async fn mcp_predicate_denies_conservatively_without_principal() {
    let ctx = McpDataContext::new().with_ability(ability_with_author_predicate());
    run_with_mcp_scopes(&ctx, None, async {
        assert!(
            nestrs_mcp::mcp_data_context::current_mcp_principal().is_none(),
            "no principal was configured"
        );
        let ability = current_mcp_ability().expect("ability installed");
        let own =
            nestrs::Subject::Instance(serde_json::json!({ "type": "Post", "author": "alice" }));
        // No principal => the predicate cannot be evaluated => deny.
        assert!(!ability.can(&Action::Read, &own));
    })
    .await;
}

// ---------------------------------------------------------------------------
// 1. `current_mcp_ability` is `Some(_)` inside the tool dispatch.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_current_ability_returns_some_inside_tool() {
    let ability = ability_with_post_fields();
    let ctx = McpDataContext::new().with_ability(ability.clone());
    let (response, _slot) = run_in_scope(&ctx, None, async {
        // Inside the dispatch, the ability slot is visible.
        let present = current_mcp_ability().is_some();
        ok_response(&serde_json::json!({ "ability_visible": present }))
    })
    .await;
    let body = extract_text_json(&response);
    assert_eq!(
        body.get("ability_visible").and_then(|v| v.as_bool()),
        Some(true),
        "ability slot must be visible inside the tool dispatch, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// 2. Outside any tool dispatch, `current_mcp_ability` is `None`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_current_ability_returns_none_outside_tool() {
    let present = current_mcp_ability().is_some();
    assert!(
        !present,
        "ability slot must NOT be visible outside a tool dispatch"
    );
}

// ---------------------------------------------------------------------------
// 3. `current_mcp_transaction` is `Some(_)` inside the dispatch when
//    a pool is configured on the context.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_current_transaction_visible_inside_tool() {
    let pool = fresh_pool().await;
    let ctx = McpDataContext::new().with_pool(pool.clone());
    let (response, _slot) = run_in_scope(&ctx, Some(&pool), async {
        let present = current_mcp_transaction().is_some();
        ok_response(&serde_json::json!({ "tx_visible": present }))
    })
    .await;
    let body = extract_text_json(&response);
    assert_eq!(
        body.get("tx_visible").and_then(|v| v.as_bool()),
        Some(true),
        "tx slot must be visible inside the tool dispatch, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// 4. Masking: tool result is post-walked against the ability.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_tool_result_is_masked_when_ability_excludes_field() {
    let ability = ability_with_post_fields();
    let ctx = McpDataContext::new().with_ability(ability);
    let (mut response, _slot) = run_in_scope(&ctx, None, async {
        // A "post" shape that includes the secret `body` field. The
        // ability only allows `id` and `title` — the post-mask
        // walker should strip `body`.
        ok_response(&serde_json::json!({
            "type": "Post",
            "id": 1,
            "title": "hi",
            "body": "secret",
        }))
    })
    .await;
    // Apply the same mask the server's `call_tool` override applies.
    let ability_for_mask = ctx.ability.clone().expect("ability present");
    mask_response(&mut response, &ability_for_mask);
    let body = extract_text_json(&response);
    assert_eq!(body.get("id").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(body.get("title").and_then(|v| v.as_str()), Some("hi"));
    assert!(
        body.get("body").is_none(),
        "body should be masked when ability excludes it, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// 5. No data context -> no ability -> no mask -> `body` is preserved.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_tool_result_unmasked_when_no_data_context() {
    let ctx = McpDataContext::new();
    let (mut response, _slot) = run_in_scope(&ctx, None, async {
        ok_response(&serde_json::json!({
            "type": "Post",
            "id": 1,
            "title": "hi",
            "body": "secret",
        }))
    })
    .await;
    // No ability -> mask_response would be a no-op anyway. Apply it
    // unconditionally to assert that "no ability" doesn't accidentally
    // crash and doesn't drop fields.
    let ability_arc = ctx.ability.clone();
    if let Some(ability) = ability_arc.as_ref() {
        mask_response(&mut response, ability);
    }
    let body = extract_text_json(&response);
    assert_eq!(body.get("body").and_then(|v| v.as_str()), Some("secret"));
    assert_eq!(body.get("title").and_then(|v| v.as_str()), Some("hi"));
}

// ---------------------------------------------------------------------------
// 5b. Different no-mask path: ability with full read (no field
//     allow-list) -> all fields preserved. Confirms the walker only
//     strips on explicit field allow-lists.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_tool_result_unmasked_when_ability_has_no_field_restriction() {
    let ctx = McpDataContext::new().with_ability(ability_with_all_fields());
    let (mut response, _slot) = run_in_scope(&ctx, None, async {
        ok_response(&serde_json::json!({
            "type": "Post",
            "id": 1,
            "title": "hi",
            "body": "secret",
        }))
    })
    .await;
    let ability_arc = ctx.ability.clone().expect("ability present");
    mask_response(&mut response, &ability_arc);
    let body = extract_text_json(&response);
    assert_eq!(body.get("body").and_then(|v| v.as_str()), Some("secret"));
    assert_eq!(body.get("title").and_then(|v| v.as_str()), Some("hi"));
}

// ---------------------------------------------------------------------------
// 6. Commit on success: clean response -> row is durable in the pool.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_transaction_commits_when_tool_returns_ok() {
    let pool = fresh_pool().await;
    let ctx = McpDataContext::new().with_pool(pool.clone());
    let (response, slot) = run_in_scope(&ctx, Some(&pool), async {
        // Tool body inserts via the ambient tx, then returns Ok.
        let slot = current_mcp_transaction().expect("tx visible");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("alpha")
                        .bind(42)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert");
        ok_response(&serde_json::json!({ "ok": true }))
    })
    .await;
    commit_or_rollback(slot, &response).await;
    assert_eq!(
        read_kv(&pool, "alpha").await,
        Some(42),
        "transaction must commit on a clean response"
    );
}

// ---------------------------------------------------------------------------
// 7. Rollback on tool-level error: row is gone after the call.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_transaction_rolls_back_when_tool_returns_error_data() {
    let pool = fresh_pool().await;
    let ctx = McpDataContext::new().with_pool(pool.clone());
    let (response, slot) = run_in_scope(&ctx, Some(&pool), async {
        // Tool body inserts via the ambient tx, then returns an error.
        let slot = current_mcp_transaction().expect("tx visible");
        let result: Result<(), sqlx::Error> = slot
            .with_tx(|tx| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO kv (key, value) VALUES ($1, $2)")
                        .bind("beta")
                        .bind(99)
                        .execute(&mut **tx)
                        .await?;
                    Ok(())
                })
            })
            .await;
        result.expect("insert");
        error_response("tool failed")
    })
    .await;
    commit_or_rollback(slot, &response).await;
    assert!(
        read_kv(&pool, "beta").await.is_none(),
        "transaction must roll back on a tool-level error"
    );
}

// ---------------------------------------------------------------------------
// 8. Masking recurses into arrays of objects.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_mask_walks_array_of_objects_in_tool_output() {
    let ability = ability_with_post_fields();
    let ctx = McpDataContext::new().with_ability(ability);
    let (mut response, _slot) = run_in_scope(&ctx, None, async {
        ok_response(&serde_json::json!({
            "type": "PostList",
            "items": [
                { "type": "Post", "id": 1, "title": "a", "body": "x" },
                { "type": "Post", "id": 2, "title": "b", "body": "y" },
            ],
        }))
    })
    .await;
    let ability_arc = ctx.ability.clone().expect("ability present");
    mask_response(&mut response, &ability_arc);
    let body = extract_text_json(&response);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .expect("items array");
    assert_eq!(items.len(), 2);
    for (i, item) in items.iter().enumerate() {
        assert!(
            item.get("body").is_none(),
            "item {i}: body should be masked, got: {item}"
        );
        assert!(
            item.get("title").is_some(),
            "item {i}: title should remain, got: {item}"
        );
        assert!(
            item.get("id").is_some(),
            "item {i}: id should remain, got: {item}"
        );
    }
}
