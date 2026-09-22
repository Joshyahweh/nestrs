//! NestRS-style **`Bind`** — load an authorized row from the path id.
//!
//! ```ignore
//! use nestrs_sea_orm::{bind_read, Repo};
//! use nestrs::current_ability_authz;
//!
//! async fn show(
//!     State(db): State<Arc<DatabaseConnection>>,
//!     Path(id): Path<i32>,
//! ) -> Result<Json<post::Model>, BindError> {
//!     let repo = Repo::<post::Entity>::new(db);
//!     let authz = current_ability_authz().ok_or(BindError::MissingAuthz)?;
//!     let model = bind_read(&repo, &authz, "Post", id).await?;
//!     Ok(Json(model))
//! }
//! ```
//!
//! Prefer [`bind_read`] / [`bind_update`] / [`bind_delete`] from handlers, or
//! the umbrella helpers `nestrs::bind_entity_read` (uses ambient ability).
//! Insert [`BoundAuthz`] via `attach_row_authz_middleware` when you want the
//! authz handle available on request extensions for custom extractors.

use crate::authz::RowAuthz;
use crate::error::RepoError;
use crate::repo::Repo;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, EntityTrait, IntoActiveModel, PrimaryKeyTrait,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

/// Errors from [`bind_read`] / [`Bind`].
#[derive(Debug)]
pub enum BindError {
    /// No [`BoundAuthz`] / ability in scope.
    MissingAuthz,
    /// Type- or row-level policy denied the operation.
    Denied(String),
    /// Row missing or invisible under deny-closed read.
    NotFound,
    /// Underlying database failure.
    Db(String),
}

impl From<RepoError> for BindError {
    fn from(value: RepoError) -> Self {
        match value {
            RepoError::Denied(msg) => Self::Denied(msg),
            RepoError::MissingAuthz(_) => Self::MissingAuthz,
            RepoError::Db(e) => Self::Db(e.to_string()),
        }
    }
}

impl IntoResponse for BindError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::MissingAuthz => (
                StatusCode::UNAUTHORIZED,
                "missing_authz",
                "row authz context is not installed".to_string(),
            ),
            Self::Denied(msg) => (StatusCode::FORBIDDEN, "denied", msg.clone()),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found".into(),
            ),
            Self::Db(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "db_error", msg.clone()),
        };
        (
            status,
            Json(json!({
                "error": code,
                "message": message,
            })),
        )
            .into_response()
    }
}

/// Request-extension handle for deny-closed [`RowAuthz`].
///
/// Insert with `nestrs::attach_row_authz_middleware` (Ability bridge) or
/// manually: `req.extensions_mut().insert(BoundAuthz::new(arc_dyn))`.
#[derive(Clone)]
pub struct BoundAuthz(pub Arc<dyn RowAuthz>);

impl BoundAuthz {
    pub fn new(authz: Arc<dyn RowAuthz>) -> Self {
        Self(authz)
    }
}

/// Declare the CASL / policy subject name for an entity (e.g. `"Post"`).
pub trait EntitySubject {
    const SUBJECT: &'static str;
}

/// Newtype around an authorized model (NestRS `Bind` analogue).
///
/// Construct via [`bind_read`] / [`Bind::read`].
pub struct Bind<E: EntityTrait> {
    pub model: E::Model,
}

impl<E: EntityTrait> std::ops::Deref for Bind<E> {
    type Target = E::Model;
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<E: EntityTrait> Bind<E> {
    pub fn into_inner(self) -> E::Model {
        self.model
    }

    /// Deny-closed read into a [`Bind`] wrapper.
    pub async fn read<A: RowAuthz + ?Sized>(
        repo: &Repo<E>,
        authz: &A,
        subject_type: &str,
        id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
    ) -> Result<Self, BindError>
    where
        E::Model: IntoActiveModel<E::ActiveModel> + Serialize + Send,
        E::ActiveModel: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send + 'static,
        <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    {
        Ok(Self {
            model: bind_read(repo, authz, subject_type, id).await?,
        })
    }
}

/// Deny-closed read: load by primary key or [`BindError::NotFound`].
pub async fn bind_read<E, A>(
    repo: &Repo<E>,
    authz: &A,
    subject_type: &str,
    id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
) -> Result<E::Model, BindError>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel> + Serialize + Send,
    E::ActiveModel: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send + 'static,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    A: RowAuthz + ?Sized,
{
    match repo.find_by_id_authorized(authz, subject_type, id).await? {
        Some(model) => Ok(model),
        None => Err(BindError::NotFound),
    }
}

/// Deny-closed update gate: load + ensure `update` is allowed on the row.
pub async fn bind_update<E, A>(
    repo: &Repo<E>,
    authz: &A,
    subject_type: &str,
    id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
) -> Result<E::Model, BindError>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel> + Serialize + Send,
    E::ActiveModel: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send + 'static,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    A: RowAuthz + ?Sized,
{
    if !authz.can("update", subject_type) {
        return Err(BindError::Denied(format!("update on {subject_type}")));
    }
    let model = bind_read(repo, authz, subject_type, id).await?;
    let json = serde_json::to_value(&model).map_err(|e| BindError::Denied(e.to_string()))?;
    if !authz.allows_row("update", subject_type, &json) {
        return Err(BindError::Denied(format!(
            "update on {subject_type} (row predicate)"
        )));
    }
    Ok(model)
}

/// Deny-closed delete gate: load + ensure `delete` is allowed (does not delete).
pub async fn bind_delete<E, A>(
    repo: &Repo<E>,
    authz: &A,
    subject_type: &str,
    id: <E::PrimaryKey as PrimaryKeyTrait>::ValueType,
) -> Result<E::Model, BindError>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel> + Serialize + Send,
    E::ActiveModel: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send + 'static,
    <E::PrimaryKey as PrimaryKeyTrait>::ValueType: Clone + Send + Sync + 'static,
    A: RowAuthz + ?Sized,
{
    if !authz.can("delete", subject_type) {
        return Err(BindError::Denied(format!("delete on {subject_type}")));
    }
    let model = bind_read(repo, authz, subject_type, id).await?;
    let json = serde_json::to_value(&model).map_err(|e| BindError::Denied(e.to_string()))?;
    if !authz.allows_row("delete", subject_type, &json) {
        return Err(BindError::Denied(format!(
            "delete on {subject_type} (row predicate)"
        )));
    }
    Ok(model)
}
