//! Ambient ORM **transactions** — `#[transactional]` handler attribute that
//! opens a `sqlx::AnyTransaction` for the lifetime of the request, commits on
//! 2xx/3xx/4xx, and rolls back on 5xx (NestJS-TypeORM default behavior).
//!
//! ## Usage
//!
//! Wire the shared pool into request extensions, then apply
//! [`install_transactional_middleware`] (or use the [`TransactionalInterceptor`]
//! wrapper) on the route. The middleware opens a transaction, stashes the
//! slot in `REQUEST_SCOPE_CACHE` so any handler-internal code can call
//! [`current_transaction`], and commits/rolls back at the end based on the
//! response status code.
//!
//! Handlers that need access to the in-flight tx call
//! [`TransactionSlot::transaction`] (locks the slot) and use it like a normal
//! `&mut sqlx::Transaction`. Outside of `#[transactional]` middleware, the
//! slot is `None` and handlers should grab the pool directly.

use crate::core::with_request_scope;
use crate::interceptor::Interceptor;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::any::{Any, TypeId};
use std::sync::Arc;
use tokio::sync::Mutex;

// `TypeId::of` in const context is stable from Rust 1.91; project MSRV is
// 1.88. Use a lazily-resolved `OnceLock` so we can call `TypeId::of` at
// runtime instead. The TypeId is interned, so the cost is one atomic load
// per call site — well below any measurement threshold.
#[allow(clippy::incompatible_msrv)]
static SLOT_TID: std::sync::OnceLock<TypeId> = std::sync::OnceLock::new();

fn slot_tid() -> TypeId {
    *SLOT_TID.get_or_init(TypeId::of::<Arc<TransactionSlot>>)
}

/// Return the in-flight [`TransactionSlot`] for the current request, if any.
/// Returns `None` when no `#[transactional]` middleware ran.
pub fn current_transaction() -> Option<Arc<TransactionSlot>> {
    let any = crate::core::request_scope_get(slot_tid())?;
    any.downcast::<Arc<TransactionSlot>>()
        .ok()
        .map(|arc| (*arc).clone())
}

/// One transaction per request. The `Mutex<Option<…>>` lets the middleware
/// extract the inner `Transaction` at commit/rollback time without blocking
/// the rest of the request flow.
pub struct TransactionSlot {
    tx: Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>,
}

impl TransactionSlot {
    pub fn new(tx: sqlx::Transaction<'static, sqlx::Any>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }

    pub async fn commit(&self) -> Result<(), sqlx::Error> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.commit().await?;
        }
        Ok(())
    }

    pub async fn rollback(&self) -> Result<(), sqlx::Error> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.rollback().await?;
        }
        Ok(())
    }

    /// Lock the slot and hand the inner `Transaction` to `f` as a
    /// `&mut sqlx::Transaction`. Errors if the slot is empty.
    pub async fn with_tx<F, R>(&self, f: F) -> Result<R, sqlx::Error>
    where
        F: for<'t> FnOnce(
            &'t mut sqlx::Transaction<'static, sqlx::Any>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<R, sqlx::Error>> + Send + 't>,
        >,
    {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_mut()
            .ok_or_else(|| sqlx::Error::Protocol("transaction already finalized".into()))?;
        f(tx).await
    }
}

/// Middleware that opens a transaction for the lifetime of one request.
///
/// 2xx/3xx/4xx → commit. 5xx → rollback. Matches NestJS-TypeORM default.
pub async fn install_transactional_middleware(
    axum::extract::State(pool): axum::extract::State<Arc<sqlx::AnyPool>>,
    req: Request,
    next: Next,
) -> Response {
    let tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("transaction begin failed: {e}"),
            )
                .into_response();
        }
    };
    let slot = Arc::new(TransactionSlot::new(tx));
    let response = with_request_scope(async move {
        crate::core::request_scope_insert(
            slot_tid(),
            Arc::new(slot.clone()) as Arc<dyn Any + Send + Sync>,
        );
        let response = next.run(req).await;
        if response.status().is_server_error() {
            if let Err(e) = slot.rollback().await {
                tracing::warn!(target: "nestrs::transactional", "rollback failed: {e}");
            }
        } else if let Err(e) = slot.commit().await {
            tracing::warn!(target: "nestrs::transactional", "commit failed: {e}");
        }
        response
    })
    .await;
    response
}

/// Interceptor wrapper around [`install_transactional_middleware`]. Use with
/// `#[use_interceptors(TransactionalInterceptor)]` so the macro shape matches
/// the rest of the framework. The pool must be in request extensions; the
/// `NestApplication` builder takes care of that when the user opts in.
#[derive(Default)]
pub struct TransactionalInterceptor;

#[async_trait::async_trait]
impl Interceptor for TransactionalInterceptor {
    async fn intercept(&self, req: Request, next: Next) -> Response {
        let pool = match req.extensions().get::<Arc<sqlx::AnyPool>>().cloned() {
            Some(p) => p,
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "TransactionalInterceptor needs an AnyPool in extensions; \
                     register it via NestApplication::with_transactional_pool(...)",
                )
                    .into_response();
            }
        };
        install_transactional_middleware(axum::extract::State(pool), req, next).await
    }
}
