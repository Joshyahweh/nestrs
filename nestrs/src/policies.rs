//! First-class **authorization** — CASL-style ability/policy engine with a [`PoliciesGuard`]
//! and per-route `#[check_policies(...)]` metadata.
//!
//! Build an [`Ability`] with the [`AbilityBuilder`], register it with
//! [`PoliciesModule::register`], then declare routes like:
//!
//! ```ignore
//! #[get("/posts/:id")]
//! #[use_guards(AuthnGuard, PoliciesGuard)]
//! #[check_policies("read:Post", "update:Post")]
//! async fn edit_post(...) -> ... { ... }
//! ```
//!
//! The [`PoliciesGuard`] reads `PrincipalIdentity` (set by `install_authn_middleware`) and
//! the `check_policies` metadata, then asks the resolved [`Ability`] whether the principal
//! is allowed to perform each `action:Subject` token. On miss the guard fails with 403.
//!
//! Symbols are gated behind the `authz` Cargo feature.

#[cfg(feature = "authn")]
use crate::authn::PrincipalIdentity;
use crate::core::{CanActivate, DynamicModule, GuardError, Injectable, ProviderRegistry};
use crate::module;
use crate::security::route_metadata_csv;
use async_trait::async_trait;
use axum::http::request::Parts;
use std::any::TypeId;
use std::collections::HashSet;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Action / Subject / Rule
// ---------------------------------------------------------------------------

/// Authorization action. `Manage` is a wildcard that satisfies every other action
/// for the same subject (CASL convention).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
    /// Wildcard — implies every other action on the same subject.
    Manage,
    Custom(String),
}

impl Action {
    /// Wildcard match: `Manage` implies everything; otherwise strict equality.
    pub fn matches(&self, other: &Action) -> bool {
        match (self, other) {
            (Action::Manage, _) | (_, Action::Manage) => true,
            (a, b) => a == b,
        }
    }

    /// Stable string form used for diagnostics and the metadata CSV.
    pub fn as_key(&self) -> String {
        match self {
            Action::Read => "read".into(),
            Action::Create => "create".into(),
            Action::Update => "update".into(),
            Action::Delete => "delete".into(),
            Action::Manage => "manage".into(),
            Action::Custom(s) => s.clone(),
        }
    }
}

/// Subject of a policy check. `Type("Post")` matches the *class*; `Instance(json)`
/// matches a specific document and is consulted against `Rule::conditions`.
#[derive(Clone, Debug)]
pub enum Subject {
    Type(&'static str),
    Instance(serde_json::Value),
}

impl Subject {
    /// The string type name (used for rule matching). For instances, looks at the
    /// `"type"` field, falling back to the JSON type tag.
    pub fn type_name<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Subject::Type(s) => s,
            Subject::Instance(v) => v.get("type").and_then(|x| x.as_str()).unwrap_or(fallback),
        }
    }
}

/// Convenience alias: condition predicates are plain JSON for portability with
/// `serde_json::Value` request payloads. Use a `serde_json::Map` so callers
/// never have to remember the import.
pub type Conditions = serde_json::Map<String, serde_json::Value>;

/// A single grant. `fields = None` means "all fields"; `conditions = None` means
/// "any instance of the subject type".
#[derive(Clone, Debug)]
pub struct Rule {
    pub action: Action,
    pub subject_type: &'static str,
    pub fields: Option<Vec<String>>,
    pub conditions: Option<Conditions>,
}

// ---------------------------------------------------------------------------
// Ability
// ---------------------------------------------------------------------------

/// Resolved set of grants. Construct via [`Ability::builder`] / [`AbilityBuilder`].
#[derive(Clone, Debug)]
pub struct Ability {
    rules: Vec<Rule>,
}

impl Ability {
    pub fn builder() -> AbilityBuilder {
        AbilityBuilder::new()
    }

    /// Returns `true` when any rule grants `(action, subject)` to the principal.
    /// For `Subject::Instance(v)`, the rule's `conditions` must also be satisfied.
    pub fn can(&self, action: &Action, subject: &Subject) -> bool {
        self.rules.iter().any(|r| rule_matches(r, action, subject))
    }

    /// Returns the field allow-list for `(action, subject)` — `Some(fields)` when
    /// the matching rule has explicit `fields`, `None` when no field restriction
    /// (i.e. all fields are allowed). Returns `None` when no rule matches at all
    /// (caller decides whether to deny or pass through).
    pub fn allowed_fields(&self, action: &Action, subject: &Subject) -> Option<Vec<String>> {
        self.rules
            .iter()
            .find(|r| rule_matches(r, action, subject))
            .map(|r| r.fields.clone())
            .and_then(|f| f)
    }

    /// Returns the row-level constraint for `(action, subject)`, if any rule
    /// declares one. Used by `Repository::find_*_authorized` to compile
    /// conditions into a `WHERE` clause.
    pub fn constraint(&self, action: &Action, subject: &Subject) -> Option<Conditions> {
        self.rules
            .iter()
            .find(|r| rule_matches(r, action, subject))
            .and_then(|r| r.conditions.clone())
    }

    /// Number of registered rules. Useful in tests + diagnostics.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

fn rule_matches(rule: &Rule, action: &Action, subject: &Subject) -> bool {
    if !rule.action.matches(action) {
        return false;
    }
    let sname = subject.type_name("");
    if sname != rule.subject_type {
        return false;
    }
    if let (Some(conds), Subject::Instance(instance)) = (&rule.conditions, subject) {
        if !conditions_satisfied(conds, instance) {
            return false;
        }
    }
    true
}

/// Returns `true` when every `column: value` constraint in `conds` is present
/// in `instance` with an equal value. The instance's column may be nested under
/// the `attributes` key (CASL wire-format) for convenience.
fn conditions_satisfied(conds: &Conditions, instance: &serde_json::Value) -> bool {
    let attrs = instance.get("attributes").unwrap_or(instance);
    for (k, v) in conds.iter() {
        let actual = attrs.get(k);
        if !json_values_equal(actual, v) {
            return false;
        }
    }
    true
}

fn json_values_equal(a: Option<&serde_json::Value>, b: &serde_json::Value) -> bool {
    match a {
        Some(x) => x == b,
        None => b.is_null(),
    }
}

// ---------------------------------------------------------------------------
// AbilityBuilder
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AbilityBuilder {
    rules: Vec<Rule>,
}

impl AbilityBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant `action` on `Subject::Type(name)` unconditionally.
    pub fn can(mut self, action: Action, subject_type: &'static str) -> Self {
        self.rules.push(Rule {
            action,
            subject_type,
            fields: None,
            conditions: None,
        });
        self
    }

    /// Grant `action` on `subject_type` only when the instance satisfies every
    /// `column: value` pair in `conditions` (e.g. `{ "tenant_id": 12 }`).
    pub fn can_with_conditions(
        mut self,
        action: Action,
        subject_type: &'static str,
        conditions: Conditions,
    ) -> Self {
        self.rules.push(Rule {
            action,
            subject_type,
            fields: None,
            conditions: Some(conditions),
        });
        self
    }

    /// Grant `action` on `subject_type` but only over the listed field names
    /// (used by [`PolicyMaskingInterceptor`](crate::masking) to strip responses).
    pub fn can_on_fields(
        mut self,
        action: Action,
        subject_type: &'static str,
        fields: Vec<String>,
    ) -> Self {
        self.rules.push(Rule {
            action,
            subject_type,
            fields: Some(fields),
            conditions: None,
        });
        self
    }

    /// Build the [`Ability`]. Consumes the builder.
    pub fn build(self) -> Ability {
        Ability { rules: self.rules }
    }
}

// ---------------------------------------------------------------------------
// route_policies_csv / PolicyEntry
// ---------------------------------------------------------------------------

/// Parsed form of a single `action:Subject` token from `#[check_policies(...)]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyEntry {
    pub action: Action,
    pub subject_type: String,
}

/// Parse a `check_policies` metadata CSV like `"read:Post,update:Post"` into
/// structured entries. Tokens that don't contain a colon are returned as
/// `Action::Custom(token)` against an empty subject (rare but tolerated).
pub fn parse_policy_entries(csv: &str) -> Vec<PolicyEntry> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|tok| match tok.split_once(':') {
            Some((a, s)) => PolicyEntry {
                action: parse_action(a.trim()),
                subject_type: s.trim().to_string(),
            },
            None => PolicyEntry {
                action: Action::Custom(tok.to_string()),
                subject_type: String::new(),
            },
        })
        .collect()
}

fn parse_action(s: &str) -> Action {
    match s {
        "read" | "Read" | "READ" => Action::Read,
        "create" | "Create" | "CREATE" => Action::Create,
        "update" | "Update" | "UPDATE" => Action::Update,
        "delete" | "Delete" | "DELETE" => Action::Delete,
        "manage" | "Manage" | "MANAGE" => Action::Manage,
        other => Action::Custom(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// PoliciesGuard
// ---------------------------------------------------------------------------

/// Guard that reads the `check_policies` route metadata and asks the resolved
/// [`Ability`] whether the principal is allowed each `action:Subject` pair.
///
/// Chain behind `AuthnGuard` (which produces the `PrincipalIdentity` this guard
/// consults). On miss: 403.
#[derive(Debug, Default)]
pub struct PoliciesGuard;

#[async_trait]
impl CanActivate for PoliciesGuard {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        let ability = parts
            .extensions
            .get::<Arc<Ability>>()
            .cloned()
            .ok_or_else(|| {
                GuardError::forbidden(
                    "PoliciesGuard used without install_policies_middleware — no Ability on request",
                )
            })?;

        // The principal is required so we can build an instance subject for
        // rules with `conditions`. The PrincipalIdentity is set by
        // `install_authn_middleware`; if it didn't run, we have no caller to
        // evaluate conditions against.
        #[cfg(feature = "authn")]
        let principal_subject: Option<Subject> = parts
            .extensions
            .get::<PrincipalIdentity>()
            .cloned()
            .map(|p| {
                Subject::Instance(serde_json::json!({
                    "type": "User",
                    "attributes": {
                        "sub": p.subject,
                        "roles": p.roles,
                    }
                }))
            });
        #[cfg(not(feature = "authn"))]
        let principal_subject: Option<Subject> = None;

        let csv = route_metadata_csv(parts, "check_policies")
            .ok_or_else(|| GuardError::forbidden("no #[check_policies(...)] on route"))?;
        let entries = parse_policy_entries(&csv);
        if entries.is_empty() {
            return Err(GuardError::forbidden("empty check_policies metadata"));
        }

        for entry in &entries {
            // The subject is the **type** declared in the route metadata. A
            // matching rule must (a) grant the action on the type, and (b) if
            // the rule carries `conditions`, satisfy them against the
            // *principal* (so `{ "roles": "admin" }` can gate admin routes).
            let subject = Subject::Type(leak_static(&entry.subject_type));
            if !ability.can(&entry.action, &subject) {
                return Err(GuardError::forbidden(
                    format!(
                        "policy denied: {} on {}",
                        entry.action.as_key(),
                        entry.subject_type
                    )
                    .as_str(),
                ));
            }
            // Conditions pass: re-evaluate specifically against the principal
            // (the CASL convention is conditions describe the principal).
            if ability.constraint(&entry.action, &subject).is_some() {
                if let Some(ref ps) = principal_subject {
                    if !ability.can(&entry.action, ps) {
                        return Err(GuardError::forbidden(
                            format!(
                                "policy denied: principal does not satisfy conditions for {} on {}",
                                entry.action.as_key(),
                                entry.subject_type
                            )
                            .as_str(),
                        ));
                    }
                } else {
                    // No principal available (e.g. `authn` not enabled) but a
                    // rule has conditions — deny conservatively.
                    return Err(GuardError::forbidden(
                        format!(
                            "policy denied: rule has conditions but no principal is present ({} on {})",
                            entry.action.as_key(),
                            entry.subject_type
                        )
                        .as_str(),
                    ));
                }
            }
        }
        Ok(())
    }
}

// `Subject::Type` requires a `&'static str`. For route-metadata subject names
// (which are dynamic strings) we leak them — they're long-lived and bounded
// by the route table, so the cost is negligible. The unique values are
// deduplicated to keep the leak set small.
fn leak_static(s: &str) -> &'static str {
    use std::sync::Mutex;
    static LEAKED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut guard = LEAKED.lock().expect("leak_static poisoned");
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(existing) = set.get(s) {
        existing
    } else {
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        set.insert(leaked);
        leaked
    }
}

// ---------------------------------------------------------------------------
// PoliciesModule
// ---------------------------------------------------------------------------

/// Configuration for [`PoliciesModule::register`].
#[derive(Clone)]
pub struct PoliciesOptions {
    /// The single ability the application enforces. Wrap a builder with `.build()`.
    pub ability: Ability,
}

impl PoliciesOptions {
    pub fn new(ability: Ability) -> Self {
        Self { ability }
    }
}

/// Build a [`PoliciesModule`] that registers a configured [`Ability`]. The
/// `PoliciesGuard` resolves the ability from this module via the DI registry.
#[module(
    providers = [Ability, PoliciesGuard],
    exports = [Ability, PoliciesGuard],
)]
pub struct PoliciesModule;

impl PoliciesModule {
    pub fn register(options: PoliciesOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        registry.override_provider::<Ability>(Arc::new(options.ability));
        // PoliciesGuard has no state — register as a singleton factory that
        // returns a unit `Default` instance. (CanActivate::resolve on the trait
        // does the heavy lifting; the provider exists so the type is reachable
        // from `use_guards(PoliciesGuard)`.)
        registry.override_provider::<PoliciesGuard>(Arc::new(PoliciesGuard));
        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![TypeId::of::<Ability>(), TypeId::of::<PoliciesGuard>()],
        )
    }
}

// Per-task stash for the active `Ability`. Set by `install_policies_middleware`
// and read by `Repository::find_*_authorized`. We use a dedicated `task_local!`
// rather than `REQUEST_SCOPE_CACHE` because the latter stores `Arc<dyn Any>`
// (no downcast helper), and we need a concrete `Arc<Ability>` back.
tokio::task_local! {
    static ABILITY_SLOT: std::cell::RefCell<Option<Arc<Ability>>>;
}

/// Pull the current request's [`Ability`] from the task-local. Returns
/// `None` when `install_policies_middleware` did not run.
pub fn current_ability() -> Option<Arc<Ability>> {
    ABILITY_SLOT.try_with(|c| c.borrow().clone()).ok().flatten()
}

/// Run `future` with `ability` stashed in the per-task ability slot. Test
/// helper — production code should rely on `install_policies_middleware`.
pub async fn with_ability<F, T>(ability: Arc<Ability>, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    ABILITY_SLOT
        .scope(std::cell::RefCell::new(Some(ability)), future)
        .await
}

/// Axum middleware that takes a configured [`Ability`] via state and stashes
/// it into both `parts.extensions` (so [`PoliciesGuard`] and
/// [`PolicyMaskingInterceptor`](crate::masking) can read it) and a
/// per-task local (so [`Repository::find_*_authorized`] can read it).
///
/// `install_authn_middleware` is independent; run this *after* authn so the
/// guard chain can read both the principal and the ability.
pub async fn install_policies_middleware(
    axum::extract::State(ability): axum::extract::State<Arc<Ability>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut parts, body) = req.into_parts();
    parts.extensions.insert(ability.clone());
    let req = axum::extract::Request::from_parts(parts, body);
    ABILITY_SLOT
        .scope(std::cell::RefCell::new(Some(ability)), next.run(req))
        .await
}

// ---------------------------------------------------------------------------
// Request-scope lookup (for Repository::find_*_authorized)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Conditions → SQL compiler (Feature D)
// ---------------------------------------------------------------------------

/// Compile a `Conditions` map into a parameterized SQL fragment + the bind
/// values in order. Supports three shapes:
///
/// * `{"col": scalar}`     → `<col> = $N`
/// * `{"col": [a, b, c]}`  → `<col> IN ($N, $M, $L)`
/// * `{"col": null}`       → `<col> IS NULL`
///
/// When `json_column` is provided, each `<col>` is rendered as
/// `json_extract(<json_column>, '$.<col>')` so the predicate operates on the
/// entity's JSON blob. Pass `None` to treat the keys as raw SQL column names.
/// Multiple conditions are joined with `AND`.
///
/// The first bind index is `start_at` (typically the count of preceding
/// `?` placeholders already in the surrounding query).
pub fn conditions_to_sql(
    conds: &Conditions,
    start_at: usize,
    json_column: Option<&str>,
) -> (String, Vec<serde_json::Value>) {
    let mut parts: Vec<String> = Vec::new();
    let mut binds: Vec<serde_json::Value> = Vec::new();
    let mut idx = start_at;
    for (col, val) in conds.iter() {
        let col_ref = match json_column {
            Some(jc) => format!("json_extract({}, '$.{}')", jc, col),
            None => col.clone(),
        };
        match val {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                let mut placeholders: Vec<String> = Vec::with_capacity(arr.len());
                for v in arr {
                    placeholders.push(format!("${}", idx));
                    binds.push(v.clone());
                    idx += 1;
                }
                parts.push(format!("{} IN ({})", col_ref, placeholders.join(", ")));
            }
            serde_json::Value::Null => {
                parts.push(format!("{} IS NULL", col_ref));
            }
            other => {
                parts.push(format!("{} = ${}", col_ref, idx));
                binds.push(other.clone());
                idx += 1;
            }
        }
    }
    (parts.join(" AND "), binds)
}

// ---------------------------------------------------------------------------
// Injectable impls
// ---------------------------------------------------------------------------
//
// `override_provider` requires `Injectable`; we provide a no-config default
// that is *replaced* by `PoliciesModule::register(PoliciesOptions)`.

impl Injectable for Ability {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        // Empty ability: denies everything. Real configuration comes through
        // `PoliciesModule::register` which `override_provider`s this entry.
        Arc::new(Ability::builder().build())
    }
}

impl Injectable for PoliciesGuard {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(PoliciesGuard)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn action_manage_implies_all_other_actions() {
        let m = Action::Manage;
        assert!(m.matches(&Action::Read));
        assert!(m.matches(&Action::Update));
        assert!(m.matches(&Action::Delete));
        assert!(m.matches(&Action::Custom("audit".into())));
    }

    #[test]
    fn action_read_does_not_imply_update() {
        assert!(!Action::Read.matches(&Action::Update));
        assert!(!Action::Update.matches(&Action::Delete));
    }

    #[test]
    fn ability_can_grants_when_rule_present() {
        let ab = Ability::builder()
            .can(Action::Read, "Post")
            .can(Action::Update, "Post")
            .build();
        assert!(ab.can(&Action::Read, &Subject::Type("Post")));
        assert!(ab.can(&Action::Update, &Subject::Type("Post")));
        assert!(!ab.can(&Action::Delete, &Subject::Type("Post")));
    }

    #[test]
    fn ability_can_conditions_match_when_value_equal() {
        let mut conds = Conditions::new();
        conds.insert("tenant_id".into(), json!(12));
        let ab = Ability::builder()
            .can_with_conditions(Action::Read, "Post", conds)
            .build();
        let owned_by_12 =
            Subject::Instance(json!({ "type": "Post", "attributes": { "tenant_id": 12 } }));
        let owned_by_99 =
            Subject::Instance(json!({ "type": "Post", "attributes": { "tenant_id": 99 } }));
        assert!(ab.can(&Action::Read, &owned_by_12));
        assert!(!ab.can(&Action::Read, &owned_by_99));
    }

    #[test]
    fn ability_manage_with_conditions_still_filters_by_conditions() {
        let mut conds = Conditions::new();
        conds.insert("owner".into(), json!("alice"));
        let ab = Ability::builder()
            .can_with_conditions(Action::Manage, "Post", conds)
            .build();
        let alice_post =
            Subject::Instance(json!({ "type": "Post", "attributes": { "owner": "alice" } }));
        let bob_post =
            Subject::Instance(json!({ "type": "Post", "attributes": { "owner": "bob" } }));
        assert!(ab.can(&Action::Update, &alice_post));
        assert!(!ab.can(&Action::Update, &bob_post));
    }

    #[test]
    fn ability_allowed_fields_returns_subset() {
        let ab = Ability::builder()
            .can_on_fields(Action::Read, "User", vec!["id".into(), "email".into()])
            .can(Action::Update, "User")
            .build();
        let read_user = Subject::Type("User");
        let fields = ab.allowed_fields(&Action::Read, &read_user).expect("Some");
        assert_eq!(fields, vec!["id", "email"]);
        let update_fields = ab.allowed_fields(&Action::Update, &read_user);
        assert!(
            update_fields.is_none(),
            "no field restriction => all fields"
        );
    }

    #[test]
    fn ability_constraint_returns_conditions_clone() {
        let mut conds = Conditions::new();
        conds.insert("status".into(), json!("published"));
        let ab = Ability::builder()
            .can_with_conditions(Action::Read, "Post", conds.clone())
            .build();
        let got = ab
            .constraint(&Action::Read, &Subject::Type("Post"))
            .expect("Some");
        assert_eq!(got.get("status").unwrap(), &json!("published"));
    }

    #[test]
    fn parse_policy_entries_handles_csv_with_mixed_cases() {
        let entries = parse_policy_entries("read:Post, update:User, manage:Org");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].action, Action::Read);
        assert_eq!(entries[0].subject_type, "Post");
        assert_eq!(entries[1].action, Action::Update);
        assert_eq!(entries[1].subject_type, "User");
        assert_eq!(entries[2].action, Action::Manage);
        assert_eq!(entries[2].subject_type, "Org");
    }
}
