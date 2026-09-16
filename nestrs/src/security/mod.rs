//! Security building blocks — thin re-export shim over the `nestrs-security`
//! workspace member. Real implementation lives in `nestrs-security`; the
//! umbrella crate keeps the same paths so existing apps do not need to
//! change imports.

pub use nestrs_security::*;
