//! `#[tool]` aggregator for all MCP tool surfaces.
//!
//! Each submodule declares a `#[derive(Debug, ...)]` tool-router struct
//! marked with `#[tool_router(server_handler)]`. `server.rs` then wires
//! the routers into the parent `NestrsMcpServer` with a plain
//! `tool_router.merge(...)` chain.

pub mod docs;
pub mod introspection;
pub mod runtime;
pub mod scaffold;
