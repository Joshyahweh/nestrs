//! Health probe surface — thin re-export shim over the `nestrs-health`
//! workspace member. Real implementation lives in `nestrs-health`; the
//! umbrella crate keeps the same paths so existing apps do not need to
//! change imports (`nestrs::health_probes::ProbeKind` still works).

pub use nestrs_health::*;
