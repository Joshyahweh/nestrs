//! Per-tool-call ambient ability + transaction + outbound masking for the
//! `nestrs-mcp` server.
//!
//! Mirrors the HTTP/WS/GraphQL accessors in `nestrs::policies`,
//! `nestrs::ws_authz`, and `nestrs::gql_authz`. The motivation is the same:
//! tool bodies in `IntrospectionTools` / `RuntimeTools` / `ScaffoldTools` /
//! `DocsTools` are zero-arg and currently cannot see the resolved ability
//! or the ambient transaction. With this module (gated on
//! `feature = "authz"`) a tool body can call
//! [`crate::mcp_data_context::current_mcp_ability`] /
//! [`crate::mcp_data_context::current_mcp_transaction`] and read the same
//! task-locals the rest of the framework uses, and the server's
//! `ServerHandler::call_tool` override post-masks the `CallToolResponse`
//! before returning.
//!
//! ## Why this lives in the transport crate
//!
//! `nestrs-mcp` is a sibling of `nestrs`, not a child: the main `nestrs`
//! crate re-exports `nestrs::mcp` as a thin facade. Putting
//! `McpDataContext` here (and the `call_tool` override in `server.rs`)
//! keeps the `nestrs -> nestrs-mcp` edge one-way.
//!
//! The downcast dance follows the same rules the GraphQL sub-feature
//! uses: the value installed via `with_ability_erased` is an
//! `Arc<dyn Any + Send + Sync>` whose unsized pointee is `Ability`, so
//! `downcast::<Ability>().ok().map(|arc| arc as Arc<Ability>)` is the
//! correct read path.

use std::any::TypeId;
use std::future::Future;
use std::sync::{Arc, OnceLock};

use nestrs::Ability;
use nestrs::TransactionSlot;
use nestrs_core::{request_scope_insert, with_ability_erased, with_request_scope};
use rmcp::model::{CallToolResponse, CallToolResult, ContentBlock};

/// `TypeId` used to key the MCP per-tool-call transaction in
/// `nestrs_core::REQUEST_SCOPE_CACHE`. The `call_tool` override opens the
/// slot and stashes it; this module reads it back.
fn tx_slot_tid() -> TypeId {
    *TX_SLOT_TID.get_or_init(TypeId::of::<Arc<TransactionSlot>>)
}

#[allow(clippy::incompatible_msrv)]
static TX_SLOT_TID: OnceLock<TypeId> = OnceLock::new();

/// Per-server context that controls the ambient scopes installed around
/// every `call_tool` invocation. Clone-cheap (only `Arc`s + `Option`s).
///
/// Wire it up on the `NestrsMcpServer` builder:
///
/// ```ignore
/// use nestrs_mcp::McpDataContext;
/// let server = nestrs_mcp::NestrsMcpServer::new()
///     .with_data_context(McpDataContext::new().with_ability(ability));
/// ```
#[derive(Debug, Default, Clone)]
pub struct McpDataContext {
    /// Resolved ability for this server. When `Some`, every tool call
    /// installs it into `nestrs_core::ABILITY_SLOT` so
    /// [`current_mcp_ability`] returns `Some(_)` from inside any tool
    /// body. The tool result is also post-walked by
    /// `nestrs::mask_value` before it leaves the server.
    pub ability: Option<Arc<Ability>>,
    /// sqlx pool for opening per-tool-call transactions. When `Some`,
    /// the `call_tool` override opens a fresh `TransactionSlot` and
    /// installs it into `nestrs_core::REQUEST_SCOPE_CACHE`; tool bodies
    /// read it via [`current_mcp_transaction`].
    pub pool: Option<Arc<sqlx::AnyPool>>,
    /// Resolved principal for this server. When `Some`, every tool call
    /// installs it into the per-task principal slot so
    /// [`current_mcp_principal`] and nestrs row-level predicates return
    /// it from inside any tool body.
    pub principal: Option<Arc<nestrs::policies::Principal>>,
}

impl McpDataContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ability(mut self, a: Arc<Ability>) -> Self {
        self.ability = Some(a);
        self
    }

    pub fn with_pool(mut self, p: Arc<sqlx::AnyPool>) -> Self {
        self.pool = Some(p);
        self
    }

    pub fn with_principal(mut self, p: Arc<nestrs::policies::Principal>) -> Self {
        self.principal = Some(p);
        self
    }
}

/// Resolved ability for the current MCP tool call, if the server was
/// constructed with an [`McpDataContext`] that carries one.
///
/// **TypeId subtlety**: the value installed via `with_ability_erased` is
/// an `Arc<dyn Any + Send + Sync>` whose unsized pointee is `Ability`
/// (the `Arc` is a pointer; the TypeId stored in the trait object is
/// the TypeId of `Ability`, *not* `Arc<Ability>`). Downcast to
/// `Ability`, then re-`Arc` it for the public API.
pub fn current_mcp_ability() -> Option<Arc<Ability>> {
    nestrs_core::current_ability_erased()
        .and_then(|a| a.downcast::<Ability>().ok())
        .map(|arc| arc as Arc<Ability>)
}

/// Resolved principal for the current MCP tool call, if the server was
/// constructed with an [`McpDataContext`] that carries one. Mirrors
/// `nestrs::policies::current_principal` for HTTP/WS/GraphQL.
pub fn current_mcp_principal() -> Option<Arc<nestrs::policies::Principal>> {
    nestrs::current_principal()
}

/// In-flight per-tool-call transaction, if a pool was configured on
/// the `McpDataContext` and the `call_tool` override ran. Tool bodies
/// can read it; they must `commit()` / `rollback()` explicitly, or the
/// sqlx `Transaction` `Drop` impl rolls back at the end of the call.
pub fn current_mcp_transaction() -> Option<Arc<TransactionSlot>> {
    nestrs_core::request_scope_get(tx_slot_tid()).and_then(|a| {
        a.downcast::<Arc<TransactionSlot>>()
            .ok()
            .map(|arc| (*arc).clone())
    })
}

/// Install the per-task scopes from `data_context` and run `dispatch`.
/// If `slot` is `Some`, also install the transaction slot into the
/// request scope so `current_mcp_transaction()` returns it from
/// inside the dispatch.
///
/// Used by:
///
/// - `NestrsMcpServer::call_tool` (the production override) — wraps
///   `self.tool_router.call(tcc)` so every tool call sees the same
///   task-locals the rest of the framework uses.
/// - Integration tests — they construct a `NestrsMcpServer` and call
///   `run_with_mcp_scopes(&ctx, slot, async { ... })` directly to
///   drive specific scope combinations without needing the rmcp
///   runtime.
pub async fn run_with_mcp_scopes<F, T>(
    data_context: &McpDataContext,
    slot: Option<Arc<TransactionSlot>>,
    dispatch: F,
) -> T
where
    F: Future<Output = T>,
{
    let ability = data_context.ability.clone();
    let principal = data_context.principal.clone();

    let inner = async move {
        if let Some(s) = slot {
            install_tx_slot_in_scope(s);
        }
        dispatch.await
    };

    // Ability and principal live in independent task-local slots, so the
    // scopes compose; same-slot nesting shadows (inner wins).
    match (ability, principal) {
        (Some(a), Some(p)) => {
            with_ability_erased(a as Arc<dyn std::any::Any + Send + Sync>, async move {
                nestrs_core::with_principal_erased(
                    p as Arc<dyn std::any::Any + Send + Sync>,
                    async move { with_request_scope(inner).await },
                )
                .await
            })
            .await
        }
        (Some(a), None) => {
            with_ability_erased(a as Arc<dyn std::any::Any + Send + Sync>, async move {
                with_request_scope(inner).await
            })
            .await
        }
        (None, Some(p)) => {
            nestrs_core::with_principal_erased(
                p as Arc<dyn std::any::Any + Send + Sync>,
                with_request_scope(inner),
            )
            .await
        }
        (None, None) => with_request_scope(inner).await,
    }
}

/// Insert the per-tool-call transaction slot into the request scope
/// so `current_mcp_transaction()` returns it from inside the tool body.
/// Must be called from inside a `with_request_scope` future — the
/// `nestrs-mcp` `call_tool` override wraps the tool dispatch in
/// `with_request_scope` (via `run_with_mcp_scopes`) and then calls
/// this before dispatching.
///
/// **TypeId subtlety**: the slot is an `Arc<TransactionSlot>`. We
/// wrap it in `Arc::new(slot)` before the unsize coercion to
/// `Arc<dyn Any>` so the slot's `TypeId` in the trait-object vtable
/// is `Arc<TransactionSlot>` (matching the downcast in
/// `current_mcp_transaction`).
pub fn install_tx_slot_in_scope(slot: Arc<TransactionSlot>) {
    request_scope_insert(
        tx_slot_tid(),
        Arc::new(slot) as Arc<dyn std::any::Any + Send + Sync>,
    );
}

/// Apply the ability's masking rules to a `CallToolResponse` in
/// place. Walks every `ContentBlock::Text` JSON payload and the
/// `structured_content` value. Tool-error responses (`is_error ==
/// Some(true)`) are returned untouched — masking on errors would
/// hide the diagnostic the caller is supposed to read.
///
/// The walker is the same `mask_value` HTTP/WS/GraphQL use, so the
/// "what gets stripped" rules are identical across transports.
pub fn mask_response(resp: &mut CallToolResponse, ability: &Ability) {
    if let CallToolResponse::Complete(result) = resp {
        // InputRequired / Task variants are routing payloads, not
        // data the caller is going to render directly, so the
        // `if let` skips them.
        mask_result(result, ability);
    }
}

fn mask_result(result: &mut CallToolResult, ability: &Ability) {
    // Don't mask error results — the caller's MCP client renders
    // `is_error == Some(true)` content as the failure message, and
    // stripping fields would hide the diagnostic.
    if result.is_error == Some(true) {
        return;
    }
    for block in &mut result.content {
        if let ContentBlock::Text(text) = block {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text.text) {
                nestrs::mask_value(&mut value, ability);
                if let Ok(serialized) = serde_json::to_string(&value) {
                    text.text = serialized;
                }
            }
        }
    }
    if let Some(value) = result.structured_content.as_mut() {
        nestrs::mask_value(value, ability);
    }
}

/// Determine whether a `CallToolResponse` represents a tool-level
/// error (caller-visible) vs. a clean success. Used to decide commit
/// vs. rollback in [`commit_or_rollback`].
pub fn is_tool_error(resp: &CallToolResponse) -> bool {
    match resp {
        CallToolResponse::Complete(r) => r.is_error == Some(true),
        // InputRequired / Task are neither "ok" nor "error" in the
        // data-mutation sense — the transaction stays open until
        // the next round. Treat them as non-error for commit policy.
        _ => false,
    }
}

/// Commit the slot if `resp` is a clean success; roll back if it's a
/// tool-level error. No-op when the slot is `None`. Errors are
/// logged but never propagated — a failed commit/rollback must not
/// change the caller-visible tool result.
pub async fn commit_or_rollback(slot: Option<Arc<TransactionSlot>>, resp: &CallToolResponse) {
    let Some(s) = slot else { return };
    let result: Result<(), sqlx::Error> = if is_tool_error(resp) {
        s.rollback().await
    } else {
        s.commit().await
    };
    if let Err(e) = result {
        tracing::warn!(
            target: "nestrs::mcp_data_context",
            "commit/rollback failed: {e}"
        );
    }
}
