//! Source-level + live-registry introspection for nestrs projects.
//!
//! Most of the per-route metadata the MCP server reports (guards, interceptors,
//! filters, body/query/path DTO types, response types) is **not** stored in any
//! runtime registry — it lives only in the `impl_routes!` body emitted by
//! `nestrs-macros`. The `source` module walks the workspace's `src/` tree with
//! `syn` to recover that metadata; the `registry` module offers the live
//! `RouteRegistry` / `ProviderRegistry` snapshot for cases where the model is
//! asking "what does this *running* app have right now?".
//!
//! New macros added to `nestrs-macros` show up as `unrecognized` in the parser
//! without failing the parse — see the "Strictly additive" rule in the plan.

pub mod metadata;
pub mod registry;
pub mod source;

pub use metadata::{DtoField, DtoSummary, Validator};
pub use registry::{LiveProviderSummary, LiveRouteSummary, SnapshotError};
pub use source::{
    ControllerSummary, ModuleSummary, ParsedWorkspace, ParserWarning, ProviderSummary,
    RouteSummary, SourceParser, WorkspaceStats,
};

use std::path::Path;

/// Convenience: parse a workspace at `path` (a directory containing
/// `Cargo.toml` and `src/`).
pub fn parse_workspace(path: &Path) -> Result<ParsedWorkspace, crate::Error> {
    let parser = SourceParser::new(path)?;
    Ok(parser.parse()?)
}
