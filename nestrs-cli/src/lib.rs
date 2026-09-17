//! Library surface for `nestrs-scaffold` (binary: `nestrs-cli`).
//!
//! crates.io owns the `nestrs-cli` crate name, so this package is
//! `nestrs-scaffold`. Integration tests and the binary both import
//! these modules through this lib.

pub mod graphql_federation;
pub mod graphql_sdl;
pub mod repl;

/// Optional MCP server mount. Enable the `mcp` feature to re-export
/// [`nestrs_mcp::server::NestrsMcpServer`] so the CLI can serve stdio/HTTP
/// without spawning a subprocess.
#[cfg(feature = "mcp")]
pub use nestrs_mcp::server::NestrsMcpServer;
