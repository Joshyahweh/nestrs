//! GraphQL side of the multi-transport Wave 3A extension: per-request
//! `Arc<Ability>` resolution, ambient sqlx `TransactionSlot` lifetime,
//! and CASL-style outbound masking on GraphQL responses.
//!
//! ## Why this lives in the main `nestrs` crate
//!
//! `nestrs-graphql` is a transport crate that the main `nestrs` crate
//! re-exports under `nestrs::graphql`. The CASL `Ability`, the
//! `TransactionSlot`, and the `mask_value` walker all live in
//! `nestrs`. Adding a `nestrs-graphql -> nestrs` dependency would
//! create a Cargo workspace cycle (`nestrs -> nestrs-graphql ->
//! nestrs`).
//!
//! The cycle is broken the same way as for the WebSocket sub-feature:
//! the transport crate (`nestrs-graphql`) defines a small
//! `GqlHandlerHook` trait, and the main `nestrs` crate implements it
//! for [`GqlDataContext`]. The trait carries only type-erased values;
//! the typed [`Ability`] and [`TransactionSlot`] stay on the
//! `nestrs` side.
//!
//! ## Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use nestrs::{Ability, GqlDataContext, GraphQlHttpOptions, graphql_router_with_context};
//!
//! let ability: Arc<Ability> = /* resolved from request */;
//! let pool: Arc<sqlx::AnyPool> = /* shared pool */;
//! let ctx = GqlDataContext::new()
//!     .with_ability(ability)
//!     .with_pool(pool);
//!
//! let router = graphql_router_with_context(
//!     schema,
//!     "/graphql",
//!     GraphQlHttpOptions::default(),
//!     ctx,
//! );
//! ```
//!
//! Inside a resolver, `nestrs::graphql::current_gql_ability()` returns
//! the downcasted `Arc<Ability>`; `current_gql_transaction()` returns
//! the per-request `TransactionSlot`. The `data` field of the response
//! is post-walked by `nestrs::masking::mask_value` before it's
//! serialized.

use crate::masking::mask_value;
use crate::policies::Ability;
use crate::transactional::TransactionSlot;
use nestrs_core::{request_scope_insert, with_ability_erased, with_request_scope};
use nestrs_graphql::{BatchResponse, GqlHandlerHook, Response as GqlResponse};
use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// `TypeId` used to key the GraphQL per-request transaction in
/// `nestrs_core::REQUEST_SCOPE_CACHE`. The handler in `nestrs-graphql`
/// opens the slot and stashes it; this module reads it back.
fn tx_slot_tid() -> TypeId {
    *TX_SLOT_TID.get_or_init(TypeId::of::<Arc<TransactionSlot>>)
}

#[allow(clippy::incompatible_msrv)]
static TX_SLOT_TID: std::sync::OnceLock<TypeId> = std::sync::OnceLock::new();

/// Per-request GraphQL context. Holds the typed ability and the
/// optional `sqlx::AnyPool` for opening a per-request transaction.
///
/// Use the `with_*` builder methods to populate it. The whole
/// `GqlDataContext` is `Clone`-cheap (`Arc`s + `Option`s).
#[derive(Clone, Default)]
pub struct GqlDataContext {
    /// Resolved ability from the request. When `Some`, every call to
    /// [`graphql_router_with_context`] installs it into
    /// `nestrs_core::ABILITY_SLOT`, so [`crate::current_gql_ability`]
    /// returns `Some(_)` from any resolver.
    pub ability: Option<Arc<Ability>>,
    /// sqlx pool for opening per-request transactions. When `Some`,
    /// the handler opens a fresh `TransactionSlot` and installs it
    /// into `nestrs_core::REQUEST_SCOPE_CACHE`; resolvers read it via
    /// [`crate::current_gql_transaction`].
    pub pool: Option<Arc<sqlx::AnyPool>>,
    /// Resolved principal from the request. When `Some`, every call to
    /// [`graphql_router_with_context`] installs it into the per-task
    /// principal slot, so [`current_gql_principal`] and row-level
    /// predicates return it.
    pub principal: Option<Arc<crate::policies::Principal>>,
    /// Batching-loader factories (`graphql-dataloader` feature). The
    /// hook's `prepare` runs this registry once per request, inserting
    /// a fresh `DataLoader` per registered loader into the request's
    /// data map — resolvers read them back via
    /// `ctx.data_unchecked::<nestrs::graphql::DataLoader<UserLoader>>()`.
    #[cfg(feature = "graphql-dataloader")]
    pub loaders: nestrs_graphql::DataLoaderRegistry,
}

impl GqlDataContext {
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

    /// Register the batching-loader factories installed per request
    /// (`graphql-dataloader` feature). See
    /// `nestrs::graphql::DataLoaderRegistry`.
    #[cfg(feature = "graphql-dataloader")]
    pub fn with_loaders(mut self, loaders: nestrs_graphql::DataLoaderRegistry) -> Self {
        self.loaders = loaders;
        self
    }
}

/// Resolved ability for the current GraphQL request, if
/// `graphql_router_with_context` was invoked with a context carrying
/// one. Mirrors `nestrs::policies::current_ability` for HTTP/WS.
///
/// **TypeId subtlety**: the value installed via `with_ability_erased` is
/// an `Arc<dyn Any + Send + Sync>` whose unsized pointee is `Ability`
/// (the `Arc` is a pointer; the TypeId stored in the trait object is
/// the TypeId of `Ability`, *not* `Arc<Ability>`). Downcast to `Ability`,
/// then re-`Arc` it for the public API.
pub fn current_gql_ability() -> Option<Arc<Ability>> {
    nestrs_core::current_ability_erased()
        .and_then(|a| a.downcast::<Ability>().ok())
        .map(|arc| arc as Arc<Ability>)
}

/// Resolved principal for the current GraphQL request, if
/// `graphql_router_with_context` was invoked with a context carrying one.
/// Mirrors `nestrs::policies::current_principal` for HTTP/WS.
pub fn current_gql_principal() -> Option<Arc<crate::policies::Principal>> {
    crate::policies::current_principal()
}

/// In-flight per-request transaction, if a pool was configured on the
/// `GqlDataContext` and the per-request scope installer ran. Resolvers
/// can read it; they must `commit()` / `rollback()` explicitly, or
/// the sqlx `Transaction` `Drop` impl rolls back at the end of the
/// request.
pub fn current_gql_transaction() -> Option<Arc<TransactionSlot>> {
    nestrs_core::request_scope_get(tx_slot_tid()).and_then(|a| {
        a.downcast::<Arc<TransactionSlot>>()
            .ok()
            .map(|arc| (*arc).clone())
    })
}

impl GqlHandlerHook for GqlDataContext {
    fn run<'a>(
        &'a self,
        execute: Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>,
    ) -> Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>> {
        Box::pin(self.run_hook(execute))
    }

    fn prepare(&self, request: &mut nestrs_graphql::Request) {
        // Fresh loaders per request: a `DataLoader` memoizes internally,
        // so a schema-global instance would leak results across requests.
        #[cfg(feature = "graphql-dataloader")]
        self.loaders.install(request);
        #[cfg(not(feature = "graphql-dataloader"))]
        {
            let _ = request;
        }
    }
}

impl GqlDataContext {
    /// Open a fresh transaction from the pool, install the ability
    /// into the per-task slot, run `execute`, commit/rollback the tx
    /// based on the response (errors roll back), and post-mask the
    /// response's `data` field against the ability. This is the
    /// `GqlHandlerHook` implementation that
    /// `graphql_router_with_context` wires into the router.
    pub async fn run_hook<'a>(
        &'a self,
        execute: Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>,
    ) -> BatchResponse {
        // Snapshot what we need before moving into the async block.
        let ability = self.ability.clone();
        let principal = self.principal.clone();
        let pool = self.pool.clone();

        // Pre-open a transaction if a pool is configured. Errors
        // swallowing the body: handlers that need stricter behaviour
        // should check `current_gql_transaction()` at the start.
        let slot = if let Some(pool) = &pool {
            match pool.begin().await {
                Ok(tx) => Some(Arc::new(TransactionSlot::new(tx))),
                Err(e) => {
                    tracing::warn!(
                        target: "nestrs::gql_authz",
                        "failed to open per-request transaction: {e}"
                    );
                    None
                }
            }
        } else {
            None
        };

        let slot_for_scope = slot;
        let ability_for_mask = ability.clone();

        let inner = async move {
            if let Some(s) = slot_for_scope.clone() {
                // Wrap in `Arc::new(s)` to preserve the inner `Arc<TransactionSlot>`
                // type identity in the slot. Without the extra wrap, the unsize
                // coercion of `s` (an `Arc<TransactionSlot>`) to `Arc<dyn Any>`
                // would store the TypeId of the *pointee* (`TransactionSlot`),
                // and `current_gql_transaction()` (which downcasts to
                // `Arc<TransactionSlot>` to match the HTTP/WS convention) would
                // fail to read it back.
                request_scope_insert(
                    tx_slot_tid(),
                    Arc::new(s) as Arc<dyn std::any::Any + Send + Sync>,
                );
            }
            let mut resp = execute.await;
            if let Some(ability) = &ability_for_mask {
                mask_response(&mut resp, ability);
            }
            // Commit/rollback based on whether the response carries
            // any errors. `BatchResponse` is a sum type — must match
            // on the variant to inspect the inner `Response.errors`
            // list. Same commit policy as HTTP: any error → rollback;
            // otherwise commit.
            let has_errors = batch_response_has_errors(&resp);
            if let Some(s) = slot_for_scope {
                if has_errors {
                    if let Err(e) = s.rollback().await {
                        tracing::warn!(
                            target: "nestrs::gql_authz",
                            "rollback failed: {e}"
                        );
                    }
                } else if let Err(e) = s.commit().await {
                    tracing::warn!(
                        target: "nestrs::gql_authz",
                        "commit failed: {e}"
                    );
                }
            }
            resp
        };

        // Ability and principal live in independent task-local slots, so
        // the scopes compose; same-slot nesting shadows (inner wins).
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
}

/// Walk the GraphQL response's `data` field (a `serde_json::Value`
/// under the hood) and apply `mask_value` to every detected subject.
/// Errors and extensions are not masked.
fn mask_response(resp: &mut BatchResponse, ability: &Ability) {
    mask_batch(resp, ability);
}

/// `BatchResponse` is a sum type (`Single(Response) | Batch(Vec<Response>)`),
/// not a struct — so the `errors` field is one level down, on each
/// inner `Response`. Walk the variant and return `true` if any
/// inner response carries a non-empty `errors` vec.
fn batch_response_has_errors(resp: &BatchResponse) -> bool {
    match resp {
        BatchResponse::Single(r) => !r.errors.is_empty(),
        BatchResponse::Batch(items) => items.iter().any(|r| !r.errors.is_empty()),
    }
}

fn mask_batch(batch: &mut BatchResponse, ability: &Ability) {
    match batch {
        BatchResponse::Single(resp) => mask_single(resp, ability),
        BatchResponse::Batch(items) => {
            for resp in items {
                mask_single(resp, ability);
            }
        }
    }
}

fn mask_single(resp: &mut GqlResponse, ability: &Ability) {
    use nestrs_graphql::Value;
    let mut value: serde_json::Value =
        serde_json::to_value(&resp.data).unwrap_or(serde_json::Value::Null);
    mask_value(&mut value, ability);
    if let Ok(new_data) = serde_json::from_value::<Value>(value) {
        resp.data = new_data;
    }
}

/// Build a `Router` that serves `schema` at `path` with the per-request
/// `ctx` installed around each execution.
///
/// `graphql_router_with_options` (no context) keeps the legacy
/// behaviour — no ability, no tx, no masking. This function is the
/// opt-in path.
pub fn graphql_router_with_context<Q, M, S>(
    schema: nestrs_graphql::Schema<Q, M, S>,
    path: impl Into<String>,
    options: nestrs_graphql::GraphQlHttpOptions,
    ctx: GqlDataContext,
) -> nestrs::axum::Router
where
    Q: nestrs_graphql::ObjectType + Send + Sync + 'static,
    M: nestrs_graphql::ObjectType + Send + Sync + 'static,
    S: nestrs_graphql::SubscriptionType + Send + Sync + 'static,
{
    nestrs_graphql::graphql_router_with_hook(schema, path, options, Arc::new(ctx))
}
