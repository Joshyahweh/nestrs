//! Bridge from nestrs [`crate::Ability`] to [`nestrs_sea_orm::RowAuthz`].
//!
//! Also installs [`BoundAuthz`] on the request so handlers / custom extractors
//! can resolve path rows under deny-closed policy via
//! [`nestrs_sea_orm::bind_read`].

use crate::policies::{Ability, Action, Subject};
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
        let Some(instance) = row_as_subject(subject_type, row) else {
            return false;
        };
        self.0.can(&action, &instance)
    }
}

/// Build the instance [`Subject`] for a stored row.
///
/// The outer `type` is the caller-supplied subject, so a column named `type`
/// cannot switch the check onto another grant. Columns are copied under
/// `attributes` (what conditions and predicates read). A nested `attributes`
/// object is dropped so it cannot hide those columns. The stored `type`
/// column is left as data: replacing it with `subject_type` made
/// `{ "type": "<subject>" }` true for every row, and a predicate that
/// rejected a discriminator never saw the stored value.
fn row_as_subject(subject_type: &str, row: &Value) -> Option<Subject> {
    let Value::Object(src) = row else {
        return None;
    };
    let mut columns = src.clone();
    columns.remove("attributes");
    Some(Subject::Instance(serde_json::json!({
        "type": subject_type,
        "attributes": columns,
    })))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::Conditions;
    use serde_json::json;

    fn post_reader_and_user_reader() -> AbilityAuthz {
        let mut conds = Conditions::new();
        conds.insert("author".into(), json!("alice"));
        let ability = Ability::builder()
            .can_with_conditions(Action::Read, "Post", conds)
            .can(Action::Read, "User")
            .build();
        AbilityAuthz::new(Arc::new(ability))
    }

    #[test]
    fn allows_row_ignores_a_forged_type_and_attributes() {
        let authz = post_reader_and_user_reader();
        let forged = json!({
            "type": "User",
            "author": "bob",
            "attributes": { "author": "alice", "type": "User" }
        });
        assert!(
            !authz.allows_row("read", "Post", &forged),
            "row type and attributes must not select the User grant or spoof author"
        );
    }

    #[test]
    fn allows_row_uses_real_columns() {
        let authz = post_reader_and_user_reader();
        let alice = json!({ "author": "alice", "title": "hi" });
        let bob = json!({ "author": "bob", "title": "hi" });
        assert!(authz.allows_row("read", "Post", &alice));
        assert!(!authz.allows_row("read", "Post", &bob));
    }

    #[test]
    fn allows_row_rejects_non_object_rows() {
        let authz = post_reader_and_user_reader();
        assert!(!authz.allows_row("read", "Post", &json!("alice")));
        assert!(!authz.allows_row("read", "Post", &Value::Null));
    }

    #[test]
    fn allows_row_type_condition_sees_the_stored_column() {
        let mut conds = Conditions::new();
        conds.insert("type".into(), json!("Document"));
        let ability = Ability::builder()
            .can_with_conditions(Action::Read, "Document", conds)
            .build();
        let authz = AbilityAuthz::new(Arc::new(ability));
        let document = json!({ "type": "Document", "body": "ok" });
        let secret = json!({ "type": "secret", "body": "hidden" });
        assert!(authz.allows_row("read", "Document", &document));
        assert!(
            !authz.allows_row("read", "Document", &secret),
            "the subject name must not replace the stored type column"
        );
    }

    #[tokio::test]
    async fn allows_row_predicate_sees_the_stored_type_column() {
        use crate::policies::{with_principal, Principal};

        let ability = Ability::builder()
            .can_with_predicate(
                Action::Read,
                "Document",
                vec![],
                |row: &Value, _: &Principal| {
                    row.get("type").and_then(|v| v.as_str()) != Some("secret")
                },
            )
            .build();
        let authz = AbilityAuthz::new(Arc::new(ability));
        let principal = Arc::new(Principal {
            subject: "alice".into(),
            roles: vec![],
            claims: json!({}),
        });
        with_principal(principal, async move {
            let note = json!({ "type": "note", "body": "hi" });
            let secret = json!({ "type": "secret", "body": "hidden" });
            assert!(authz.allows_row("read", "Document", &note));
            assert!(
                !authz.allows_row("read", "Document", &secret),
                "a predicate must observe the stored type, not the subject name"
            );
        })
        .await;
    }
}
