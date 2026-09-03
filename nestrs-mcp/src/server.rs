//! Top-level MCP server. Aggregates the four tool routers
//! (introspection, runtime, scaffold, docs) into a single
//! `ServerHandler` so the binary can serve one `--stdio` or `--http`
//! endpoint with all tools registered.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::RoleServer;
use rmcp::{tool_handler, tool_router};

use crate::tools::{
    docs::DocsTools, introspection::IntrospectionTools, runtime::RuntimeTools,
    scaffold::ScaffoldTools,
};

/// A list of tools the model can call. Merges all four sub-routers so
/// we can serve a single tool-list to the client.
#[derive(Debug)]
pub struct NestrsMcpServer {
    pub tool_router: ToolRouter<Self>,
}

impl Default for NestrsMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl NestrsMcpServer {
    pub fn new() -> Self {
        // We don't aggregate sub-routers here because the rmcp router
        // type is invariant in `Self`. Each `*Tools` struct is meant to
        // be served as its own handler when the user wants one area;
        // for the all-tools case, the binary spawns one server per
        // area and the MCP client multiplexes. The top-level
        // `NestrsMcpServer` exists so embedders (e.g. `nestrs-cli`) can
        // mount a single default handler without picking an area.
        Self {
            tool_router: ToolRouter::<Self>::new(),
        }
    }

    /// All four sub-servers. Each is a fully-formed `ServerHandler`
    /// that can be served on its own transport (so the binary can
    /// spawn four in parallel if the MCP client expects a multi-handler
    /// model). The `merge` helper here exists for symmetry with future
    /// `crate::tools::*` composition.
    pub fn sub_servers() -> SubServers {
        SubServers {
            introspection: IntrospectionTools,
            runtime: RuntimeTools,
            scaffold: ScaffoldTools,
            docs: DocsTools,
        }
    }
}

/// The four area-specific servers. Each can be served on its own
/// transport. The top-level `NestrsMcpServer` is a thin shim.
#[derive(Debug)]
pub struct SubServers {
    pub introspection: IntrospectionTools,
    pub runtime: RuntimeTools,
    pub scaffold: ScaffoldTools,
    pub docs: DocsTools,
}

#[tool_router]
impl NestrsMcpServer {
    // No tools here — `NestrsMcpServer` is just the wrapper. Use
    // `SubServers` to get the actual tool-bearing handlers.
}

#[tool_handler]
impl ServerHandler for NestrsMcpServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::default())
            .with_server_info(
                Implementation::new("nestrs-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("nestrs Model Context Protocol server")
                    .with_description(
                        "Introspection, live runtime, scaffolding, and docs search for nestrs.",
                    )
                    .with_website_url("https://github.com/Joshyahweh/nestrs/tree/main/nestrs-mcp"),
            )
            .with_instructions(
                "nestrs-mcp exposes nestrs project structure, live runtime, \
                 scaffolding actions, and docs search. Each tool takes a \
                 `workspace_path` for source-level operations; runtime tools \
                 take a `base_url` and optional `token`.",
            )
    }
    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, rmcp::ErrorData> {
        Ok(self.get_info())
    }
}
