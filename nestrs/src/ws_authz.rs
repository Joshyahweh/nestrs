//! WebSocket side of the multi-transport Wave 3A extension: per-connection
//! `Arc<Ability>` resolution, ambient sqlx `TransactionSlot` lifetime, and
//! CASL-style outbound masking for WebSocket frames.
//!
//! ## Why this lives in the main `nestrs` crate
//!
//! `nestrs-ws` is a transport crate that the main `nestrs` crate re-exports
//! under `nestrs::ws`. The CASL `Ability`, the `TransactionSlot`, and the
//! `mask_value` walker all live in `nestrs`. Adding a `nestrs-ws -> nestrs`
//! dependency would create a Cargo workspace cycle (`nestrs -> nestrs-ws ->
//! nestrs`). So the WS authz glue lives here, in the main crate, where it
//! can reach the `nestrs` types freely and call into the `nestrs-ws`
//! primitives (`WsClient::emit_json`, `WsGateway`) directly.
//!
//! ## Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use nestrs::{Ability, WsScope, with_ability, run_in_ws_scope, emit_masked};
//!
//! let ability: Arc<Ability> = /* resolved from the upgrade request */;
//! let pool: Arc<sqlx::AnyPool> = /* shared pool */;
//! let scope = WsScope::new()
//!     .with_ability(ability)
//!     .with_pool(pool);
//!
//! // Inside a gateway handler:
//! run_in_ws_scope(scope, async move {
//!     let ability = current_ws_ability().unwrap();
//!     let data = serde_json::json!({ "type": "Post", "title": "hi", "body": "secret" });
//!     emit_masked(&client, "post", data, &ability).unwrap();
//! }).await;
//! ```
//!
//! Or — if the user prefers to drop down to the lower-level `nestrs-ws`
//! `serve_socket` API — they can call `current_ws_ability()` /
//! `current_ws_transaction()` from inside a handler that runs within
//! `run_in_ws_scope` and use the existing `client.emit(...)` path
//! unchanged (the per-message scope still installs the ambient ability +
//! tx; only the masking helper is opt-in).

use crate::policies::{with_ability, with_principal, Ability};
use crate::transactional::TransactionSlot;
use nestrs_core::{request_scope_insert, with_request_scope};
use std::any::{Any, TypeId};
use std::future::Future;
use std::sync::Arc;

// `TypeId::of` in const context is stable from Rust 1.91; project MSRV is
// 1.88. Mirror the `transactional.rs` pattern: a function-local `static`
// lazily resolved on first access. Cheap (TypeId is interned) and MSRV-safe.
#[allow(clippy::incompatible_msrv)]
static SLOT_TID: std::sync::OnceLock<TypeId> = std::sync::OnceLock::new();

fn slot_tid() -> TypeId {
    *SLOT_TID.get_or_init(TypeId::of::<Arc<TransactionSlot>>)
}

/// Per-connection state for a WebSocket gateway. Cloneable; the
/// per-message loop holds one copy and passes it through `run_in_ws_scope`
/// on every inbound message.
#[derive(Clone, Default)]
pub struct WsScope {
    /// Resolved ability from the upgrade request. When `Some`, every call
    /// to [`run_in_ws_scope`] installs it into the per-task `ABILITY_SLOT`,
    /// so [`current_ws_ability`] returns `Some(_)` from any handler.
    pub ability: Option<Arc<Ability>>,
    /// sqlx pool for opening per-message transactions. When `Some`, every
    /// call to [`run_in_ws_scope`] opens a fresh `TransactionSlot` and
    /// installs it into `REQUEST_SCOPE_CACHE`; [`current_ws_transaction`]
    /// returns it inside the handler.
    pub pool: Option<Arc<sqlx::AnyPool>>,
    /// Resolved principal from the upgrade request. When `Some`, every call
    /// to [`run_in_ws_scope`] installs it into the per-task `PRINCIPAL_SLOT`,
    /// so [`current_ws_principal`] and row-level predicates return it.
    pub principal: Option<Arc<crate::policies::Principal>>,
}

impl WsScope {
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

    pub fn with_principal(mut self, p: Arc<crate::policies::Principal>) -> Self {
        self.principal = Some(p);
        self
    }
}

/// Resolved ability for the current WebSocket message, if
/// `run_in_ws_scope` was invoked with a `WsScope` carrying one. Mirrors
/// `nestrs::policies::current_ability` for HTTP/GraphQL/MCP.
pub fn current_ws_ability() -> Option<Arc<Ability>> {
    crate::policies::current_ability()
}

/// Resolved principal for the current WebSocket message, if
/// `run_in_ws_scope` was invoked with a `WsScope` carrying one. Mirrors
/// `nestrs::policies::current_principal` for HTTP/GraphQL/MCP.
pub fn current_ws_principal() -> Option<Arc<crate::policies::Principal>> {
    crate::policies::current_principal()
}

/// In-flight per-message transaction, if a pool was configured on the
/// `WsScope` and the per-message scope installer ran. The slot is opened
/// without an explicit commit; handlers must call
/// `slot.commit()` (or `rollback()`) explicitly, or the sqlx
/// `Transaction` `Drop` impl rolls back when the message scope ends.
pub fn current_ws_transaction() -> Option<Arc<TransactionSlot>> {
    crate::transactional::current_transaction()
}

/// Run `body` inside the per-message scope: open a tx (if `scope.pool` is
/// `Some`), install the `ABILITY_SLOT` and `REQUEST_SCOPE_CACHE` slot,
/// then await `body`. The slot drops at the end of this call without a
/// commit unless the handler did so explicitly.
///
/// Returns whatever `body` returns. Errors from `pool.begin()` are
/// swallowed (the body runs with no slot installed) — handlers that need
/// stricter behaviour should check `current_ws_transaction()` at the
/// start.
pub async fn run_in_ws_scope<F, T>(scope: WsScope, body: F) -> T
where
    F: Future<Output = T>,
{
    let slot = if let Some(pool) = &scope.pool {
        match pool.begin().await {
            Ok(tx) => Some(Arc::new(TransactionSlot::new(tx))),
            Err(e) => {
                tracing::warn!(
                    target: "nestrs::ws_authz",
                    "failed to open per-message transaction: {e}"
                );
                None
            }
        }
    } else {
        None
    };

    let slot_for_scope = slot;
    let inner = async move {
        if let Some(s) = slot_for_scope {
            request_scope_insert(slot_tid(), Arc::new(s) as Arc<dyn Any + Send + Sync>);
        }
        body.await
    };

    let in_request_scope = with_request_scope(inner);
    // Ability and principal live in independent task-local slots, so the
    // scopes compose; same-slot nesting shadows (inner wins).
    match (scope.ability.clone(), scope.principal.clone()) {
        (Some(a), Some(p)) => with_ability(a, with_principal(p, in_request_scope)).await,
        (Some(a), None) => with_ability(a, in_request_scope).await,
        (None, Some(p)) => with_principal(p, in_request_scope).await,
        (None, None) => in_request_scope.await,
    }
}

/// Mask a `serde_json::Value` against an ability and emit it on a
/// `nestrs_ws::WsClient`. Drop-in for `client.emit(event, data)` when
/// the response should respect the per-connection `Ability`.
///
/// Re-uses `crate::masking::mask_value` (the same walker the HTTP
/// `PolicyMaskingInterceptor` uses on JSON response bodies) so
/// behaviour is identical across transports.
pub fn emit_masked<T: serde::Serialize>(
    client: &nestrs_ws::WsClient,
    event: &str,
    data: T,
    ability: &Ability,
) -> Result<(), nestrs_ws::WsSendError> {
    let mut value =
        serde_json::to_value(data).map_err(|e| nestrs_ws::WsSendError::Serialize(e.to_string()))?;
    crate::masking::mask_value(&mut value, ability);
    client.emit_json(event, value)
}
