//! Bridge from nestrs [`crate::Ability`] to [`nestrs_sea_orm::RowAuthz`].
//!
//! Also installs [`BoundAuthz`] on the request so handlers / custom extractors
//! can resolve path rows under deny-closed policy via
//! [`nestrs_sea_orm::bind_read`].

use crate::policies::{current_principal, Ability, Action, Subject};
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use nestrs_sea_orm::{BoundAuthz, RowAuthz};
use serde_json::Value;
use std::sync::Arc;

/// [`RowAuthz`] adapter over a CASL-style [`Ability`].
///
/// Construct from [`current_ability_authz`] inside a request that already has
/// policies middleware / an ambient ability installed.
#[derive(Clone)]
pub struct AbilityAuthz(pub Arc<Ability>);

impl AbilityAuthz {
    pub fn new(ability: Arc<Ability>) -> Self {
        Self(ability)
    }
}

impl RowAuthz for AbilityAuthz {
    fn can(&self, action: &str, subject_type: &str) -> bool {
        self.0.can(
            &Action::parse(action),
            &Subject::Type(subject_type.to_string()),
        )
    }

    fn allows_row(&self, action: &str, subject_type: &str, row: &Value) -> bool {
        let action = Action::parse(action);
        if !self
            .0
            .can(&action, &Subject::Type(subject_type.to_string()))
        {
            return false;
        }
        let mut instance = row.clone();
        if let Value::Object(map) = &mut instance {
            map.entry("type".to_string())
                .or_insert_with(|| Value::String(subject_type.to_string()));
        }
        let _ = current_principal();
        self.0.can(&action, &Subject::Instance(instance))
    }
}

/// Build [`AbilityAuthz`] from the request-scoped [`crate::current_ability`].
pub fn current_ability_authz() -> Option<AbilityAuthz> {
    crate::current_ability().map(AbilityAuthz::new)
}

/// Axum middleware: copy the ambient [`Ability`] into [`BoundAuthz`] extensions.
///
/// Layer this **inside** policies / ability middleware so
/// [`crate::current_ability`] is already set for the request future:
///
/// ```ignore
/// use axum::middleware::from_fn;
/// use nestrs::attach_row_authz_middleware;
///
/// // axum: last `.layer` is outermost — put policies outside so it wraps attach.
/// router
///     .layer(from_fn(attach_row_authz_middleware))
///     .layer(policies_layer);
/// ```
pub async fn attach_row_authz_middleware(mut req: Request, next: Next) -> Response {
    if let Some(authz) = current_ability_authz() {
        let dyn_authz: Arc<dyn RowAuthz> = Arc::new(authz);
        req.extensions_mut().insert(BoundAuthz::new(dyn_authz));
    }
    next.run(req).await
}
