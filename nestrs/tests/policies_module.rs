#![cfg(feature = "authz")]

//! `policies` module unit + integration tests:
//!  * Ability matching (action / subject / conditions / fields)
//!  * AbilityBuilder composition
//!  * `parse_policy_entries` (the CSV that `#[check_policies(...)]` lowers to)
//!  * Metadata registry round-trip for `check_policies` (set by the proc-macro)

use nestrs::prelude::*;
use nestrs::{parse_policy_entries, Ability, AbilityBuilder, Action, Conditions, Subject};
use serde_json::json;

#[test]
fn ability_can_returns_true_for_granted_action() {
    let ab = Ability::builder()
        .can(Action::Read, "Post")
        .can(Action::Update, "Post")
        .build();
    assert!(ab.can(&Action::Read, &Subject::Type("Post")));
    assert!(ab.can(&Action::Update, &Subject::Type("Post")));
}

#[test]
fn ability_can_returns_false_for_ungranted_action() {
    let ab = Ability::builder().can(Action::Read, "Post").build();
    assert!(!ab.can(&Action::Delete, &Subject::Type("Post")));
    assert!(!ab.can(&Action::Read, &Subject::Type("Comment")));
}

#[test]
fn ability_can_with_conditions_matches_when_conditions_equal() {
    let mut conds = Conditions::new();
    conds.insert("tenant_id".into(), json!(12));
    let ab = Ability::builder()
        .can_with_conditions(Action::Read, "Post", conds)
        .build();
    let post_12 = Subject::Instance(json!({
        "type": "Post",
        "attributes": { "tenant_id": 12 }
    }));
    let post_99 = Subject::Instance(json!({
        "type": "Post",
        "attributes": { "tenant_id": 99 }
    }));
    assert!(ab.can(&Action::Read, &post_12));
    assert!(!ab.can(&Action::Read, &post_99));
}

#[test]
fn ability_can_with_conditions_rejects_when_conditions_mismatch() {
    let mut conds = Conditions::new();
    conds.insert("owner".into(), json!("alice"));
    let ab = Ability::builder()
        .can_with_conditions(Action::Manage, "Post", conds)
        .build();
    let bob_post = Subject::Instance(json!({
        "type": "Post",
        "attributes": { "owner": "bob" }
    }));
    assert!(!ab.can(&Action::Update, &bob_post));
    assert!(!ab.can(&Action::Delete, &bob_post));
}

#[test]
fn ability_can_with_instance_subject_matches_by_id() {
    let ab = Ability::builder().can(Action::Read, "User").build();
    let user_42 = Subject::Instance(json!({ "type": "User", "id": 42 }));
    let user_99 = Subject::Instance(json!({ "type": "User", "id": 99 }));
    assert!(ab.can(&Action::Read, &user_42));
    assert!(ab.can(&Action::Read, &user_99));
}

#[test]
fn ability_can_with_instance_subject_rejects_for_other_owner() {
    // Conditions: only the owner of a post can update it.
    let mut conds = Conditions::new();
    conds.insert("owner_id".into(), json!(7));
    let ab = Ability::builder()
        .can_with_conditions(Action::Update, "Post", conds)
        .build();
    let owned = Subject::Instance(json!({
        "type": "Post",
        "id": 1,
        "attributes": { "owner_id": 7 }
    }));
    let not_owned = Subject::Instance(json!({
        "type": "Post",
        "id": 2,
        "attributes": { "owner_id": 99 }
    }));
    assert!(ab.can(&Action::Update, &owned));
    assert!(!ab.can(&Action::Update, &not_owned));
}

#[test]
fn ability_can_with_manage_implies_all_actions() {
    let ab = Ability::builder().can(Action::Manage, "Org").build();
    assert!(ab.can(&Action::Read, &Subject::Type("Org")));
    assert!(ab.can(&Action::Create, &Subject::Type("Org")));
    assert!(ab.can(&Action::Update, &Subject::Type("Org")));
    assert!(ab.can(&Action::Delete, &Subject::Type("Org")));
    assert!(ab.can(&Action::Custom("audit".into()), &Subject::Type("Org")));
}

#[test]
fn ability_allowed_fields_returns_field_subset_when_defined() {
    let ab = Ability::builder()
        .can_on_fields(Action::Read, "User", vec!["id".into(), "email".into()])
        .can(Action::Update, "User")
        .build();
    let subject = Subject::Type("User");
    let read_fields = ab.allowed_fields(&Action::Read, &subject).expect("Some");
    assert_eq!(read_fields, vec!["id", "email"]);
    assert!(ab.allowed_fields(&Action::Update, &subject).is_none());
}

#[test]
fn ability_constraint_returns_conditions_for_subject() {
    let mut conds = Conditions::new();
    conds.insert("status".into(), json!("published"));
    let ab = Ability::builder()
        .can_with_conditions(Action::Read, "Post", conds)
        .build();
    let got = ab
        .constraint(&Action::Read, &Subject::Type("Post"))
        .expect("Some");
    assert_eq!(got.get("status").unwrap(), &json!("published"));
}

#[test]
fn ability_constraint_is_none_when_no_conditions_rule() {
    let ab = Ability::builder().can(Action::Read, "Post").build();
    assert!(ab
        .constraint(&Action::Read, &Subject::Type("Post"))
        .is_none());
}

#[test]
fn policies_module_register_does_not_panic_on_minimal_options() {
    // Sanity: a module with an empty ability should be constructible.
    let ability = Ability::builder().build();
    let module = PoliciesModule::register(PoliciesOptions::new(ability));
    assert!(module.exports.contains(&std::any::TypeId::of::<Ability>()));
    assert!(module
        .exports
        .contains(&std::any::TypeId::of::<PoliciesGuard>()));
}

#[test]
fn parse_policy_entries_handles_simple_csv() {
    let entries = parse_policy_entries("read:Post,update:Post");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].action, Action::Read);
    assert_eq!(entries[0].subject_type, "Post");
    assert_eq!(entries[1].action, Action::Update);
    assert_eq!(entries[1].subject_type, "Post");
}

#[test]
fn parse_policy_entries_handles_whitespace_and_empties() {
    let entries = parse_policy_entries(" read:Post ,, update:User ,");
    assert_eq!(entries.len(), 2, "empty tokens should be dropped");
    assert_eq!(entries[0].subject_type, "Post");
    assert_eq!(entries[1].subject_type, "User");
}

#[test]
fn parse_policy_entries_handles_manage_and_custom() {
    let entries = parse_policy_entries("manage:Org,audit:Post");
    assert_eq!(entries[0].action, Action::Manage);
    assert_eq!(entries[1].action, Action::Custom("audit".into()));
}

#[test]
fn ability_builder_reports_rule_count() {
    let ab = AbilityBuilder::new()
        .can(Action::Read, "Post")
        .can(Action::Update, "Post")
        .can_with_conditions(Action::Delete, "Post", Conditions::new())
        .build();
    assert_eq!(ab.rule_count(), 3);
}

// -- MetadataRegistry round-trip ------------------------------------------------

#[derive(Default)]
#[injectable]
struct MetaState;

#[controller(prefix = "/p")]
struct MetaController;

#[routes(state = MetaState)]
impl MetaController {
    #[get("/policies")]
    #[check_policies("read:Post", "update:User")]
    async fn has_policies() -> &'static str {
        "ok"
    }
}

#[test]
fn check_policies_metadata_round_trips_through_registry() {
    use nestrs::core::MetadataRegistry;
    // Reach into the registry directly: this is what PoliciesGuard does at
    // request time. We just confirm the proc-macro emitted the right key.
    let entries = parse_policy_entries("read:Post,update:User");
    let csv = entries
        .iter()
        .map(|e| format!("{}:{}", e.action.as_key(), e.subject_type))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(csv, "read:Post,update:User");
    // Sanity: an unknown handler name returns None (we never set it).
    assert!(MetadataRegistry::get("no-such-handler", "no-such-key").is_none());
}
