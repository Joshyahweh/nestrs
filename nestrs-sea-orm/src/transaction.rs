//! Ambient SeaORM transactions (NestJS-TypeORM style).
//!
//! Apply [`install_sea_orm_transactional_middleware`] with
//! `axum::middleware::from_fn_with_state(db, install_sea_orm_transactional_middleware)`.
//! Handlers and [`crate::Repo`] then see the in-flight transaction via
//! [`current_sea_orm_transaction`].
//!
//! Commit policy: **2xx / 3xx / 4xx → commit**, **5xx → rollback**.

use crate::error::RepoError;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use nestrs_core::with_request_scope;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait};
use std::any::{Any, TypeId};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

#[allow(clippy::incompatible_msrv)]
static SLOT_TID: std::sync::OnceLock<TypeId> = std::sync::OnceLock::new();

fn slot_tid() -> TypeId {
    *SLOT_TID.get_or_init(TypeId::of::<Arc<SeaOrmTransactionSlot>>)
}

/// Return the in-flight SeaORM transaction for the current request, if any.
pub fn current_sea_orm_transaction() -> Option<Arc<SeaOrmTransactionSlot>> {
    let any = nestrs_core::request_scope_get(slot_tid())?;
    any.downcast::<Arc<SeaOrmTransactionSlot>>()
        .ok()
        .map(|arc| (*arc).clone())
}

/// One SeaORM transaction per request.
pub struct SeaOrmTransactionSlot {
    tx: Mutex<Option<DatabaseTransaction>>,
}

impl SeaOrmTransactionSlot {
    pub fn new(tx: DatabaseTransaction) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }

    pub async fn commit(&self) -> Result<(), RepoError> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.commit().await?;
        }
        Ok(())
    }

    pub async fn rollback(&self) -> Result<(), RepoError> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.rollback().await?;
        }
        Ok(())
    }

    /// Lock the slot and run `f` against the live [`DatabaseTransaction`].
    pub async fn with_tx<F, R>(&self, f: F) -> Result<R, RepoError>
    where
        F: for<'t> FnOnce(
            &'t mut DatabaseTransaction,
        )
            -> Pin<Box<dyn Future<Output = Result<R, RepoError>> + Send + 't>>,
    {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_mut()
            .ok_or_else(|| RepoError::Denied("sea-orm transaction already finalized".into()))?;
        f(tx).await
    }
}

/// Opens a SeaORM transaction for the lifetime of one request.
pub async fn install_sea_orm_transactional_middleware(
    axum::extract::State(db): axum::extract::State<Arc<DatabaseConnection>>,
    req: Request,
    next: Next,
) -> Response {
    let tx = match db.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("sea-orm transaction begin failed: {e}"),
            )
                .into_response();
        }
    };
    let slot = Arc::new(SeaOrmTransactionSlot::new(tx));
    with_request_scope(async move {
        nestrs_core::request_scope_insert(
            slot_tid(),
            Arc::new(slot.clone()) as Arc<dyn Any + Send + Sync>,
        );
        let response = next.run(req).await;
        if response.status().is_server_error() {
            if let Err(e) = slot.rollback().await {
                tracing::warn!(target: "nestrs_sea_orm::transaction", "rollback failed: {e}");
            }
        } else if let Err(e) = slot.commit().await {
            tracing::warn!(target: "nestrs_sea_orm::transaction", "commit failed: {e}");
        }
        response
    })
    .await
}
