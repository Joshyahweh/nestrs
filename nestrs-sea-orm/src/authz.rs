//! Pluggable row-level authorization for [`crate::Repo`].
//!
//! The trait stays in this crate so `nestrs-sea-orm` does not depend on the
//! umbrella `nestrs` crate. Downstream, `nestrs` (feature `sea-orm` + `authz`)
//! implements [`RowAuthz`] for its CASL-style [`Ability`](https://docs.rs/nestrs)
//! via `nestrs::sea_orm::AbilityAuthz`.

use serde_json::Value;

/// Minimal authz surface the SeaORM [`crate::Repo`] needs.
///
/// Actions use Nest/CASL-style lowercase names: `create`, `read`, `update`,
/// `delete`. Subject types are application strings (usually the entity /
/// table name).
pub trait RowAuthz: Send + Sync {
    /// Type-level check (no concrete row).
    fn can(&self, action: &str, subject_type: &str) -> bool;

    /// Row-level check. Return `false` to deny. Implementations that need an
    /// ambient principal should deny closed when none is installed.
    fn allows_row(&self, action: &str, subject_type: &str, row: &Value) -> bool;
}
