//! `nestrs-mcp` — Model Context Protocol server for nestrs.
//!
//! Crate root. The binary entry point lives in `main.rs`; this `lib.rs`
//! exposes the server so embedders (e.g. a future `nestrs-cli` `mcp`
//! subcommand) can mount it without spawning a subprocess.
//!
//! ## Module layout
//!
//! - [`error`] — crate-level `Error` type and `Result` alias.
//! - [`introspection`] — source-level + live-registry snapshot.
//! - [`docs`] — local-file docs search (CHANGELOG, mdBook, READMEs).
//! - [`scaffold`] — `new_project` / `create_module` / `create_resource` /
//!   `create_dto` / `generate_crud`.
//! - [`runtime`] — HTTP client for the live admin port on a running app.
//! - [`server`] — the `ServerHandler` + `#[tool_router]` aggregator.
//! - [`tools`] — one module per surface, each implementing a group of MCP
//!   `#[tool]` methods.

#![warn(missing_debug_implementations)]
#![warn(rust_2018_idioms)]

pub mod docs;
pub mod error;
pub mod introspection;
pub mod runtime;
pub mod scaffold;
pub mod server;
pub mod tools;
pub mod wizard;

pub use error::{Error, Result};

#[cfg(feature = "admin")]
pub use nestrs::admin::{AdminHandle, AdminOptions};
