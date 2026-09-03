//! Live runtime tools (require a running nestrs app with the `admin`
//! feature enabled). All tools take the admin port URL and an optional
//! bearer token.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData, Json};
use serde::{Deserialize, Serialize};

use crate::runtime::AdminClient;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AdminEndpoint {
    /// Base URL of the admin port (e.g. `"http://127.0.0.1:7777"`).
    pub base_url: String,
    /// Optional bearer token.
    pub token: Option<String>,
}

fn client(args: &AdminEndpoint) -> Result<AdminClient, crate::Error> {
    AdminClient::new(args.base_url.clone(), args.token.clone())
}

fn into_rmcp(e: crate::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

#[derive(Debug)]
pub struct RuntimeTools;

#[tool_router]
impl RuntimeTools {
    #[tool(
        name = "get_app_health",
        description = "Fetch liveness + readiness + uptime from a running nestrs app's admin port.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_app_health(
        &self,
        Parameters(args): Parameters<AdminEndpoint>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let c = client(&args).map_err(into_rmcp)?;
        let h: crate::runtime::AdminHealth = c.health().await.map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(h).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "get_app_routes",
        description = "Fetch the live route table from a running nestrs app's admin port.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_app_routes(
        &self,
        Parameters(args): Parameters<AdminEndpoint>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let c = client(&args).map_err(into_rmcp)?;
        let r: crate::runtime::AdminRoutes = c.routes().await.map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(r).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "get_app_providers",
        description = "Fetch the live DI provider list from a running nestrs app's admin port.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_app_providers(
        &self,
        Parameters(args): Parameters<AdminEndpoint>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let c = client(&args).map_err(into_rmcp)?;
        let p: crate::runtime::AdminProviders = c.providers().await.map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(p).unwrap_or(serde_json::Value::Null),
        ))
    }
}

#[tool_handler]
impl RuntimeTools {}
