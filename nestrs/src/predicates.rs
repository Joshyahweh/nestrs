//! Pre-built [`RowPredicate`]s for row-level authorization (`authz-row-level`
//! feature).
//!
//! Each predicate is a small struct with defaulted column names plus a
//! [`RowPredicate`](crate::RowPredicate) impl. Two layers:
//!
//! * [`RowPredicate::check`] — the always-on closure check ("is this row
//!   visible to this principal?"). Applied post-load by every authorized
//!   repository path and by `CrudService` under `authz-row-level`.
//! * [`RowPredicate::sql_conditions`](crate::RowPredicate::sql_conditions) —
//!   the per-request pushdown hint, compiled into the `WHERE` clause by
//!   [`Repository::find_many_authorized`](crate::Repository::find_many_authorized)
//!   (and the other authorized finds via the rule's declarative conditions).
//!
//! OR-shaped predicates (`OwnerOrAdmin`, `TenantOrAdmin`, `SelfOrAdmin`,
//! `PublicOrOwner`) cannot be expressed as a single AND-joined conditions map,
//! so their pushdown strategy is: when the principal holds the admin role the
//! pushdown returns `None` (closure allows everything), otherwise the OR
//! collapses to a single equality that *can* be pushed down. `PublicOrOwner`
//! is row-dependent on both branches and is post-filter only.
//!
//! The closure signature `(&Value, &Principal) -> bool` is the
//! leak-prevention guarantee: predicates see the row and the principal, and
//! nothing else (no headers, no environment, no ambient state).

use crate::policies::{Conditions, Principal, RowPredicate};
use serde_json::json;

/// Compare a row field to a principal value with JSON semantics (numbers
/// compare across int/float; `"12"` the string is *not* equal to `12`).
fn row_field_eq(row: &serde_json::Value, field: &str, expected: &serde_json::Value) -> bool {
    row.get(field).map_or(false, |v| v == expected)
}

fn is_admin(principal: &Principal, admin_role: &str) -> bool {
    principal.roles.iter().any(|r| r == admin_role)
}

// ---------------------------------------------------------------------------
// AuthorIsCurrentUser / BelongsToUser
// ---------------------------------------------------------------------------

/// The row's author column equals the principal's subject.
///
/// Pushdown: `{ "author": principal.subject }` → `json_extract(data,
/// '$.author') = $N`.
#[derive(Debug, Clone)]
pub struct AuthorIsCurrentUser {
    pub field: String,
}

impl Default for AuthorIsCurrentUser {
    fn default() -> Self {
        Self {
            field: "author".into(),
        }
    }
}

impl AuthorIsCurrentUser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl RowPredicate for AuthorIsCurrentUser {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        row_field_eq(row, &self.field, &json!(principal.subject))
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), json!(principal.subject));
        Some(conds)
    }
}

/// The row's user column equals the principal's subject (alias of
/// [`AuthorIsCurrentUser`] with a `user_id` default — kept as a distinct name
/// so policy code reads naturally on non-authored resources).
#[derive(Debug, Clone)]
pub struct BelongsToUser {
    pub field: String,
}

impl Default for BelongsToUser {
    fn default() -> Self {
        Self {
            field: "user_id".into(),
        }
    }
}

impl BelongsToUser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl RowPredicate for BelongsToUser {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        row_field_eq(row, &self.field, &json!(principal.subject))
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), json!(principal.subject));
        Some(conds)
    }
}

// ---------------------------------------------------------------------------
// WithinTenant
// ---------------------------------------------------------------------------

/// The row's tenant column equals the principal's tenant claim
/// (`principal.claims[claim_field]`).
///
/// Pushdown: `{ row_field: <claim value> }` when the claim is present;
/// `None` (closure decides) when the principal carries no tenant claim —
/// which the closure then denies for rows with a tenant, matching the
/// deny-conservative convention.
#[derive(Debug, Clone)]
pub struct WithinTenant {
    pub row_field: String,
    pub claim_field: String,
}

impl Default for WithinTenant {
    fn default() -> Self {
        Self {
            row_field: "tenant_id".into(),
            claim_field: "tenant_id".into(),
        }
    }
}

impl WithinTenant {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_row_field(field: impl Into<String>) -> Self {
        Self {
            row_field: field.into(),
            ..Self::default()
        }
    }

    pub fn with_claim_field(field: impl Into<String>) -> Self {
        Self {
            claim_field: field.into(),
            ..Self::default()
        }
    }
}

impl RowPredicate for WithinTenant {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        match principal.claims.get(&self.claim_field) {
            Some(claim) => row_field_eq(row, &self.row_field, claim),
            None => false,
        }
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        let claim = principal.claims.get(&self.claim_field)?;
        let mut conds = Conditions::new();
        conds.insert(self.row_field.clone(), claim.clone());
        Some(conds)
    }
}

// ---------------------------------------------------------------------------
// OwnerOrAdmin / TenantOrAdmin / SelfOrAdmin
// ---------------------------------------------------------------------------

/// The row is the principal's own (owner column == subject) OR the principal
/// holds the admin role.
///
/// Pushdown: admins get `None` (allow-all via closure); everyone else gets
/// the collapsed equality `{ field: principal.subject }`.
#[derive(Debug, Clone)]
pub struct OwnerOrAdmin {
    pub field: String,
    pub admin_role: String,
}

impl Default for OwnerOrAdmin {
    fn default() -> Self {
        Self {
            field: "owner".into(),
            admin_role: "admin".into(),
        }
    }
}

impl OwnerOrAdmin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            ..Self::default()
        }
    }

    pub fn with_admin_role(role: impl Into<String>) -> Self {
        Self {
            admin_role: role.into(),
            ..Self::default()
        }
    }
}

impl RowPredicate for OwnerOrAdmin {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        is_admin(principal, &self.admin_role)
            || row_field_eq(row, &self.field, &json!(principal.subject))
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        if is_admin(principal, &self.admin_role) {
            return None;
        }
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), json!(principal.subject));
        Some(conds)
    }
}

/// The row's tenant column equals the principal's tenant claim OR the
/// principal holds the admin role. Same conditional-pushdown strategy as
/// [`OwnerOrAdmin`].
#[derive(Debug, Clone)]
pub struct TenantOrAdmin {
    pub row_field: String,
    pub claim_field: String,
    pub admin_role: String,
}

impl Default for TenantOrAdmin {
    fn default() -> Self {
        Self {
            row_field: "tenant_id".into(),
            claim_field: "tenant_id".into(),
            admin_role: "admin".into(),
        }
    }
}

impl TenantOrAdmin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_admin_role(role: impl Into<String>) -> Self {
        Self {
            admin_role: role.into(),
            ..Self::default()
        }
    }
}

impl RowPredicate for TenantOrAdmin {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        if is_admin(principal, &self.admin_role) {
            return true;
        }
        match principal.claims.get(&self.claim_field) {
            Some(claim) => row_field_eq(row, &self.row_field, claim),
            None => false,
        }
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        if is_admin(principal, &self.admin_role) {
            return None;
        }
        let claim = principal.claims.get(&self.claim_field)?;
        let mut conds = Conditions::new();
        conds.insert(self.row_field.clone(), claim.clone());
        Some(conds)
    }
}

/// The row IS the principal (its `id` column equals the principal's subject)
/// OR the principal holds the admin role — for `User`-shaped tables.
#[derive(Debug, Clone)]
pub struct SelfOrAdmin {
    pub field: String,
    pub admin_role: String,
}

impl Default for SelfOrAdmin {
    fn default() -> Self {
        Self {
            field: "id".into(),
            admin_role: "admin".into(),
        }
    }
}

impl SelfOrAdmin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_admin_role(role: impl Into<String>) -> Self {
        Self {
            admin_role: role.into(),
            ..Self::default()
        }
    }
}

impl RowPredicate for SelfOrAdmin {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        is_admin(principal, &self.admin_role)
            || row_field_eq(row, &self.field, &json!(principal.subject))
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        if is_admin(principal, &self.admin_role) {
            return None;
        }
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), json!(principal.subject));
        Some(conds)
    }
}

// ---------------------------------------------------------------------------
// PublishedOnly / NotDeleted / PublicOrOwner / HasRole
// ---------------------------------------------------------------------------

/// The row's flag column is `true` (drafts stay invisible).
///
/// Pushdown: static `{ field: true }` — works without a principal.
#[derive(Debug, Clone)]
pub struct PublishedOnly {
    pub field: String,
}

impl Default for PublishedOnly {
    fn default() -> Self {
        Self {
            field: "published".into(),
        }
    }
}

impl PublishedOnly {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl RowPredicate for PublishedOnly {
    fn check(&self, row: &serde_json::Value, _principal: &Principal) -> bool {
        row_field_eq(row, &self.field, &json!(true))
    }

    fn sql_conditions(&self, _principal: &Principal) -> Option<Conditions> {
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), json!(true));
        Some(conds)
    }
}

/// The row is not soft-deleted: the timestamp column is absent or `null`.
///
/// Pushdown: static `{ field: null }` → `IS NULL`.
#[derive(Debug, Clone)]
pub struct NotDeleted {
    pub field: String,
}

impl Default for NotDeleted {
    fn default() -> Self {
        Self {
            field: "deleted_at".into(),
        }
    }
}

impl NotDeleted {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl RowPredicate for NotDeleted {
    fn check(&self, row: &serde_json::Value, _principal: &Principal) -> bool {
        match row.get(&self.field) {
            None | Some(serde_json::Value::Null) => true,
            _ => false,
        }
    }

    fn sql_conditions(&self, _principal: &Principal) -> Option<Conditions> {
        let mut conds = Conditions::new();
        conds.insert(self.field.clone(), serde_json::Value::Null);
        Some(conds)
    }
}

/// The row is public OR owned by the principal. Both branches are
/// row-dependent, so there is **no** pushdown (`sql_conditions` returns
/// `None`) — the predicate is applied post-load only; combine it with a
/// declarative condition or a pre-filter if the table is large.
#[derive(Debug, Clone)]
pub struct PublicOrOwner {
    pub visibility_field: String,
    pub public_value: String,
    pub owner_field: String,
}

impl Default for PublicOrOwner {
    fn default() -> Self {
        Self {
            visibility_field: "visibility".into(),
            public_value: "public".into(),
            owner_field: "owner".into(),
        }
    }
}

impl PublicOrOwner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_public_value(value: impl Into<String>) -> Self {
        Self {
            public_value: value.into(),
            ..Self::default()
        }
    }
}

impl RowPredicate for PublicOrOwner {
    fn check(&self, row: &serde_json::Value, principal: &Principal) -> bool {
        row_field_eq(row, &self.visibility_field, &json!(self.public_value))
            || row_field_eq(row, &self.owner_field, &json!(principal.subject))
    }
}

/// The principal holds a role (the row is irrelevant — this is a role gate
/// expressed as a row predicate so it composes with the others).
///
/// No pushdown in either direction: when the principal has the role the
/// pushdown would be allow-all (modeled as `None`), and when it doesn't the
/// closure denies every row. Note the fetch is wasted for non-admins —
/// prefer a `PoliciesGuard` rule with `{"roles": ...}` conditions for
/// pure role gates.
#[derive(Debug, Clone)]
pub struct HasRole {
    pub role: String,
}

impl HasRole {
    pub fn new(role: impl Into<String>) -> Self {
        Self { role: role.into() }
    }
}

impl RowPredicate for HasRole {
    fn check(&self, _row: &serde_json::Value, principal: &Principal) -> bool {
        is_admin(principal, &self.role)
    }

    fn sql_conditions(&self, principal: &Principal) -> Option<Conditions> {
        // Has the role => allow-all is inexpressible as AND-joined equality
        // conditions; doesn't have it => deny-all is inexpressible too.
        // Post-filter only.
        let _ = principal;
        None
    }
}
