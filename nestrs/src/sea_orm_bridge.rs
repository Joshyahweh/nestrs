//! Bridge from nestrs [`crate::Ability`] to [`nestrs_sea_orm::RowAuthz`].

use crate::policies::{current_principal, Ability, Action, Subject};
use nestrs_sea_orm::RowAuthz;
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
        // Type-level grant first.
        let action = Action::parse(action);
        if !self
            .0
            .can(&action, &Subject::Type(subject_type.to_string()))
        {
            return false;
        }
        // If the rule carries a predicate / conditions, evaluate against the row.
        // `Ability::can` on `Subject::Instance` denies closed when a predicate
        // exists but no principal is installed.
        let mut instance = row.clone();
        if let Value::Object(map) = &mut instance {
            map.entry("type".to_string())
                .or_insert_with(|| Value::String(subject_type.to_string()));
        }
        let _ = current_principal(); // document ambient dependency
        self.0.can(&action, &Subject::Instance(instance))
    }
}

/// Build [`AbilityAuthz`] from the request-scoped [`crate::current_ability`].
pub fn current_ability_authz() -> Option<AbilityAuthz> {
    crate::current_ability().map(AbilityAuthz::new)
}
