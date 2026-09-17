//! `#[tool]` aggregator for all MCP tool surfaces.
//!
//! Each submodule declares a tool-router struct. `server.rs` cannot
//! `merge` those routers into `NestrsMcpServer` (rmcp's `ToolRouter<S>`
//! is invariant in `S`); instead the stock server dispatches
//! `list_tools` / `get_tool` / `call_tool` to the four area handlers.

pub mod docs;
pub mod introspection;
pub mod runtime;
pub mod scaffold;
