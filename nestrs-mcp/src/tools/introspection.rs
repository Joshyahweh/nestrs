//! Source-level introspection tools (read-only, no running app required).
//!
//! These all run on top of `introspection::source::SourceParser`, which
//! walks the workspace with `syn` and returns the `ParsedWorkspace` that
//! other tools can filter against.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData, Json};
use serde::{Deserialize, Serialize};

use crate::introspection::source::{ParsedWorkspace, SourceParser};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspacePath {
    /// Absolute path to the nestrs workspace root (the directory that
    /// contains `Cargo.toml`).
    pub workspace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleName {
    /// Absolute path to the nestrs workspace root.
    pub workspace_path: String,
    /// Module name (e.g. `"app"`).
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ControllerName {
    pub workspace_path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProviderName {
    pub workspace_path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DtoName {
    pub workspace_path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RouteRef {
    pub workspace_path: String,
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleFilter {
    pub workspace_path: String,
    /// Optional module name to scope the listing.
    pub module: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ControllerFilter {
    pub workspace_path: String,
    /// Optional module name to scope the listing.
    pub module: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RouteFilter {
    pub workspace_path: String,
    /// Optional module name to scope the listing.
    pub module: Option<String>,
    /// Optional controller name to scope the listing.
    pub controller: Option<String>,
}

fn parse(workspace_path: &str) -> Result<ParsedWorkspace, crate::Error> {
    let parser = SourceParser::new(PathBuf::from(workspace_path))?;
    Ok(parser.parse()?)
}

fn jsonify<T: Serialize>(v: T) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

fn into_rmcp(e: crate::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

fn module_not_found(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("module `{name}` not found"), None)
}
fn controller_not_found(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("controller `{name}` not found"), None)
}
fn provider_not_found(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("provider `{name}` not found"), None)
}
fn dto_not_found(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("DTO `{name}` not found"), None)
}
fn route_not_found(method: &str, path: &str) -> ErrorData {
    ErrorData::invalid_params(format!("route `{method} {path}` not found"), None)
}

#[derive(Debug)]
pub struct IntrospectionTools;

#[tool_router]
impl IntrospectionTools {
    #[tool(
        name = "list_modules",
        description = "List all `#[module]` modules in the workspace.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_modules(
        &self,
        Parameters(args): Parameters<WorkspacePath>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        Ok(Json(parsed.modules.into_iter().map(jsonify).collect()))
    }

    #[tool(
        name = "get_module",
        description = "Get one module by name with its imports/controllers/providers/exports.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_module(
        &self,
        Parameters(args): Parameters<ModuleName>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let module = parsed
            .modules
            .into_iter()
            .find(|m| m.name == args.name)
            .ok_or_else(|| module_not_found(&args.name))?;
        Ok(Json(jsonify(module)))
    }

    #[tool(
        name = "list_controllers",
        description = "List all `#[controller]` controllers.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_controllers(
        &self,
        Parameters(args): Parameters<ControllerFilter>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let controllers: Vec<_> = parsed
            .controllers
            .into_iter()
            .filter(|c| {
                args.module
                    .as_deref()
                    .is_none_or(|m| c.module_path.contains(m))
            })
            .map(jsonify)
            .collect();
        Ok(Json(controllers))
    }

    #[tool(
        name = "get_controller",
        description = "Get one controller by name with its routes, guards, and response type.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_controller(
        &self,
        Parameters(args): Parameters<ControllerName>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let controller = parsed
            .controllers
            .into_iter()
            .find(|c| c.name == args.name)
            .ok_or_else(|| controller_not_found(&args.name))?;
        Ok(Json(jsonify(controller)))
    }

    #[tool(
        name = "list_providers",
        description = "List all `#[injectable]` providers and their scope.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_providers(
        &self,
        Parameters(args): Parameters<ModuleFilter>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let providers: Vec<_> = parsed
            .providers
            .into_iter()
            .filter(|p| args.module.as_deref().is_none_or(|m| p.file.contains(m)))
            .map(jsonify)
            .collect();
        Ok(Json(providers))
    }

    #[tool(
        name = "get_provider",
        description = "Get one provider by name with its scope and constructor signature.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_provider(
        &self,
        Parameters(args): Parameters<ProviderName>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let provider = parsed
            .providers
            .into_iter()
            .find(|p| p.type_name == args.name)
            .ok_or_else(|| provider_not_found(&args.name))?;
        Ok(Json(jsonify(provider)))
    }

    #[tool(
        name = "list_routes",
        description = "List all HTTP routes across all controllers.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_routes(
        &self,
        Parameters(args): Parameters<RouteFilter>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let routes: Vec<_> = parsed
            .controllers
            .into_iter()
            .flat_map(|c| c.routes.into_iter().map(move |r| (c.name.clone(), r)))
            .filter(|(_, r)| {
                args.controller
                    .as_deref()
                    .is_none_or(|c| r.handler.contains(c))
            })
            .map(|(c, r)| {
                serde_json::json!({
                    "controller": c,
                    "method": r.method,
                    "path": r.path,
                    "handler": r.handler,
                })
            })
            .collect();
        Ok(Json(routes))
    }

    #[tool(
        name = "get_route",
        description = "Get one route by method+path with its middlewares, validators, and OpenAPI summary.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_route(
        &self,
        Parameters(args): Parameters<RouteRef>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let (controller, route) = parsed
            .controllers
            .into_iter()
            .flat_map(|c| c.routes.into_iter().map(move |r| (c.name.clone(), r)))
            .find(|(_, r)| r.path == args.path && r.method.eq_ignore_ascii_case(&args.method))
            .ok_or_else(|| route_not_found(&args.method, &args.path))?;
        Ok(Json(serde_json::json!({
            "controller": controller,
            "method": route.method,
            "path": route.path,
            "handler": route.handler,
            "version": route.version,
            "guards": route.guards,
            "interceptors": route.interceptors,
            "pipes": route.pipes,
            "filters": route.filters,
            "metadata": route.metadata,
            "body_type": route.body_type,
            "response_type": route.response_type,
        })))
    }

    #[tool(
        name = "list_dtos",
        description = "List all `#[dto]` DTOs.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_dtos(
        &self,
        Parameters(args): Parameters<WorkspacePath>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        Ok(Json(parsed.dtos.into_iter().map(jsonify).collect()))
    }

    #[tool(
        name = "get_dto",
        description = "Get one DTO by name with its fields and validators.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_dto(
        &self,
        Parameters(args): Parameters<DtoName>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        let dto = parsed
            .dtos
            .into_iter()
            .find(|d| d.name == args.name)
            .ok_or_else(|| dto_not_found(&args.name))?;
        Ok(Json(jsonify(dto)))
    }

    #[tool(
        name = "list_schedules",
        description = "List all `#[interval]` / `#[cron]` scheduled jobs.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_schedules(
        &self,
        Parameters(args): Parameters<WorkspacePath>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        Ok(Json(parsed.schedules.into_iter().map(jsonify).collect()))
    }

    #[tool(
        name = "list_event_handlers",
        description = "List all `#[on_event]` handlers.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_event_handlers(
        &self,
        Parameters(args): Parameters<WorkspacePath>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        Ok(Json(
            parsed.event_handlers.into_iter().map(jsonify).collect(),
        ))
    }

    #[tool(
        name = "list_queue_processors",
        description = "List all `#[process]` queue handlers.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_queue_processors(
        &self,
        Parameters(args): Parameters<WorkspacePath>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let parsed = parse(&args.workspace_path).map_err(into_rmcp)?;
        Ok(Json(
            parsed.queue_processors.into_iter().map(jsonify).collect(),
        ))
    }
}

#[tool_handler]
impl IntrospectionTools {}
