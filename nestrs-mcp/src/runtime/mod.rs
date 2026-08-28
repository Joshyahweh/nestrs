//! Live-runtime HTTP client (talks to a running nestrs app's `admin` port).
//!
//! The admin port is added in the same change set (gated behind the
//! `nestrs::admin` feature) and exposes:
//!
//! - `GET /__nestrs/health` — liveness.
//! - `GET /__nestrs/providers` — `[{ type_name, scope }]`
//! - `GET /__nestrs/routes` — `Vec<LiveRouteSummary>`
//!
//! The MCP tools in `tools/runtime.rs` are thin wrappers over this client.

pub mod client;
pub mod types;

pub use client::AdminClient;
pub use types::{AdminHealth, AdminProviders, AdminRoutes};
