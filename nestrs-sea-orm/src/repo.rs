//! SeaORM [`Repo`] — pool + ambient-transaction aware data access.

use crate::authz::RowAuthz;
use crate::error::RepoError;
use crate::transaction::current_sea_orm_transaction;
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait,
    IntoActiveModel, PrimaryKeyTrait, QueryFilter,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::marker::PhantomData;
use std::sync::Arc;

/// Typed repository over a SeaORM [`EntityTrait`].
///
/// When [`crate::current_sea_orm_transaction`] is set (via
/// [`crate::install_sea_orm_transactional_middleware`]), all methods run
/// inside that transaction; otherwise they use the shared pool.
#[derive(Clone)]
pub struct Repo<E: EntityTrait> {
    db: Arc<DatabaseConnection>,
    _marker: PhantomData<E>,
}

impl<E> Repo<E>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel> + Send,
    E::ActiveModel: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send + 'static,
{
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            db,
            _marker: PhantomData,
        }
    }

    /// Shared pool connection (does not observe an ambient transaction).
    pub fn db(&self) -> &Arc<DatabaseConnection> {
        &self.db
    }

    /// `SELECT` by primary key.
    pub async fn find_by_id(
        &self,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<Option<E::Model>, RepoError>
    where
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(
                        async move { E::find_by_id(id).one(tx).await.map_err(RepoError::from) },
                    )
                })
                .await;
        }
        E::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(RepoError::from)
    }

    /// `SELECT` all rows.
    pub async fn find_all(&self) -> Result<Vec<E::Model>, RepoError> {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(async move { E::find().all(tx).await.map_err(RepoError::from) })
                })
                .await;
        }
        E::find()
            .all(self.db.as_ref())
            .await
            .map_err(RepoError::from)
    }

    /// Filtered find (`Condition` from SeaORM / [`eq_condition`]).
    pub async fn find_with_filter(&self, filter: Condition) -> Result<Vec<E::Model>, RepoError> {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(async move {
                        E::find()
                            .filter(filter)
                            .all(tx)
                            .await
                            .map_err(RepoError::from)
                    })
                })
                .await;
        }
        E::find()
            .filter(filter)
            .all(self.db.as_ref())
            .await
            .map_err(RepoError::from)
    }

    /// Insert an active model and return the inserted model.
    pub async fn insert(&self, model: E::ActiveModel) -> Result<E::Model, RepoError> {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(async move { model.insert(tx).await.map_err(RepoError::from) })
                })
                .await;
        }
        model
            .insert(self.db.as_ref())
            .await
            .map_err(RepoError::from)
    }

    /// Update an active model and return the updated model.
    pub async fn update(&self, model: E::ActiveModel) -> Result<E::Model, RepoError> {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(async move { model.update(tx).await.map_err(RepoError::from) })
                })
                .await;
        }
        model
            .update(self.db.as_ref())
            .await
            .map_err(RepoError::from)
    }

    /// Delete by primary key. Returns rows affected (`0` when missing).
    pub async fn delete_by_id(
        &self,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<u64, RepoError>
    where
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    {
        if let Some(slot) = current_sea_orm_transaction() {
            return slot
                .with_tx(|tx| {
                    Box::pin(async move {
                        let res = E::delete_by_id(id).exec(tx).await?;
                        Ok(res.rows_affected)
                    })
                })
                .await;
        }
        let res = E::delete_by_id(id).exec(self.db.as_ref()).await?;
        Ok(res.rows_affected)
    }

    /// Deny-closed read via [`RowAuthz`].
    pub async fn find_by_id_authorized<A: RowAuthz + ?Sized>(
        &self,
        authz: &A,
        subject_type: &str,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<Option<E::Model>, RepoError>
    where
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
        E::Model: Serialize,
    {
        if !authz.can("read", subject_type) {
            return Err(RepoError::Denied(format!("read on {subject_type}")));
        }
        let Some(model) = self.find_by_id(id).await? else {
            return Ok(None);
        };
        let json = to_json(&model, subject_type)?;
        if !authz.allows_row("read", subject_type, &json) {
            return Ok(None);
        }
        Ok(Some(model))
    }

    /// Deny-closed list via [`RowAuthz`] (post-load filter).
    pub async fn find_all_authorized<A: RowAuthz + ?Sized>(
        &self,
        authz: &A,
        subject_type: &str,
    ) -> Result<Vec<E::Model>, RepoError>
    where
        E::Model: Serialize,
    {
        if !authz.can("read", subject_type) {
            return Err(RepoError::Denied(format!("read on {subject_type}")));
        }
        let rows = self.find_all().await?;
        let mut out = Vec::with_capacity(rows.len());
        for model in rows {
            let json = to_json(&model, subject_type)?;
            if authz.allows_row("read", subject_type, &json) {
                out.push(model);
            }
        }
        Ok(out)
    }

    /// Deny-closed insert. `candidate` is the JSON shape judged by the row
    /// predicate **before** the write (same convention as sqlx `CrudService`).
    pub async fn insert_authorized<A: RowAuthz + ?Sized>(
        &self,
        authz: &A,
        subject_type: &str,
        model: E::ActiveModel,
        candidate: &JsonValue,
    ) -> Result<E::Model, RepoError> {
        if !authz.can("create", subject_type) {
            return Err(RepoError::Denied(format!("create on {subject_type}")));
        }
        if !authz.allows_row("create", subject_type, candidate) {
            return Err(RepoError::Denied(format!(
                "create on {subject_type} (row predicate)"
            )));
        }
        self.insert(model).await
    }

    /// Deny-closed update. Loads the row, checks `update` + row predicate,
    /// then writes `model`.
    pub async fn update_authorized<A: RowAuthz + ?Sized>(
        &self,
        authz: &A,
        subject_type: &str,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
        model: E::ActiveModel,
    ) -> Result<E::Model, RepoError>
    where
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
        E::Model: Serialize,
    {
        if !authz.can("update", subject_type) {
            return Err(RepoError::Denied(format!("update on {subject_type}")));
        }
        let Some(existing) = self.find_by_id(id).await? else {
            return Err(RepoError::Denied(format!(
                "update on {subject_type} (missing row)"
            )));
        };
        let json = to_json(&existing, subject_type)?;
        if !authz.allows_row("update", subject_type, &json) {
            return Err(RepoError::Denied(format!(
                "update on {subject_type} (row predicate)"
            )));
        }
        self.update(model).await
    }

    /// Deny-closed delete.
    pub async fn delete_by_id_authorized<A: RowAuthz + ?Sized>(
        &self,
        authz: &A,
        subject_type: &str,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<bool, RepoError>
    where
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
        E::Model: Serialize,
    {
        if !authz.can("delete", subject_type) {
            return Err(RepoError::Denied(format!("delete on {subject_type}")));
        }
        let Some(model) = self.find_by_id(id.clone()).await? else {
            return Ok(false);
        };
        let json = to_json(&model, subject_type)?;
        if !authz.allows_row("delete", subject_type, &json) {
            return Err(RepoError::Denied(format!(
                "delete on {subject_type} (row predicate)"
            )));
        }
        Ok(self.delete_by_id(id).await? > 0)
    }
}

fn to_json<T: Serialize>(value: &T, subject_type: &str) -> Result<JsonValue, RepoError> {
    serde_json::to_value(value)
        .map_err(|e| RepoError::Denied(format!("serialize {subject_type} for authz: {e}")))
}

/// Equality condition helper (`col = value`).
pub fn eq_condition<C, V>(col: C, value: V) -> Condition
where
    C: ColumnTrait,
    V: Into<sea_orm::Value>,
{
    Condition::all().add(col.eq(value))
}
