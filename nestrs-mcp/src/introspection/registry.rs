//! Live-registry snapshot (requires a running app to query).
//!
//! `nestrs_core` exposes `RouteRegistry::list()` and
//! `ProviderRegistry::registered_type_names()`. The MCP runtime tools
//! (`get_app_routes`, `get_app_providers`) call into a running app's
//! `__nestrs/admin/...` endpoints; the types here are the shapes those
//! endpoints return. They are intentionally minimal — the admin port is
//! a small surface, not a re-implementation of the source parser.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotError {
    /// The admin endpoint returned a non-success status.
    #[error("admin endpoint returned status {status}: {body}")]
    Http { status: u16, body: String },
    /// The response body didn't deserialize.
    #[error("admin response parse error: {0}")]
    Parse(#[from] serde_json::Error),
    /// A required field was missing.
    #[error("admin response missing field: {0}")]
    MissingField(&'static str),
}

/// One route entry as returned by the live admin endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LiveRouteSummary {
    pub method: String,
    pub path: String,
    /// `module_path::handler` — the same string the macro emits into
    /// `RouteInfo::handler`.
    pub handler: String,
    /// Optional OpenAPI summary if the route was annotated with
    /// `#[openapi(summary = "...")]`.
    pub openapi_summary: Option<String>,
}

/// One provider entry as returned by the live admin endpoint.
///
/// `scope` is exposed via a new `AdminSnapshot` trait on
/// `ProviderRegistry`; the MCP server never sees the factory or hook fn
/// pointers (those are private to `nestrs-core`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LiveProviderSummary {
    /// Static type name from `ProviderEntry::type_name`.
    pub type_name: String,
    /// `singleton` / `transient` / `request`.
    pub scope: String,
}
