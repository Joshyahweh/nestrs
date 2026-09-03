//! Scaffolding tools (write actions). All return a `ScaffoldReport`
//! with `files_created` + `files_modified` so the model can summarize.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData, Json};
use serde::{Deserialize, Serialize};

use crate::scaffold::crud::{generate_crud, CrudSpec};
use crate::scaffold::dto::{create_dto, DtoFieldSpec};
use crate::scaffold::module::create_module;
use crate::scaffold::project::new_project;
use crate::scaffold::resource::{create_resource, ResourceTransport};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewProjectArgs {
    /// Target directory. Will be created if it doesn't exist.
    pub path: String,
    /// Crate name.
    pub name: String,
    /// Optional list of feature names to enable on `nestrs`.
    #[serde(default)]
    pub transports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateModuleArgs {
    /// Path to the crate root.
    pub path: String,
    /// Module name (lowercase snake_case).
    pub name: String,
    #[serde(default)]
    pub transports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateResourceArgs {
    pub path: String,
    pub name: String,
    pub fields: Vec<DtoFieldSpec>,
    pub transport: ResourceTransport,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateDtoArgs {
    pub path: String,
    pub name: String,
    pub fields: Vec<DtoFieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GenerateCrudArgs {
    pub path: String,
    pub resource: String,
    pub fields: Vec<DtoFieldSpec>,
    pub transports: Vec<ResourceTransport>,
}

fn into_rmcp(e: crate::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

#[derive(Debug)]
pub struct ScaffoldTools;

#[tool_router]
impl ScaffoldTools {
    #[tool(
        name = "new_project",
        description = "Scaffold a new nestrs project (Cargo.toml, src/main.rs, src/app.rs, README, .gitignore).",
        annotations(destructive_hint = true)
    )]
    pub async fn new_project_tool(
        &self,
        Parameters(args): Parameters<NewProjectArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let report = new_project(&PathBuf::from(args.path), &args.name, &args.transports)
            .map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "create_module",
        description = "Scaffold a new module (mod.rs + controller.rs + service.rs) and register it in the parent.",
        annotations(destructive_hint = true)
    )]
    pub async fn create_module_tool(
        &self,
        Parameters(args): Parameters<CreateModuleArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let report = create_module(&PathBuf::from(args.path), &args.name, &args.transports)
            .map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "create_resource",
        description = "Scaffold a DTO + controller + service + module for a single resource.",
        annotations(destructive_hint = true)
    )]
    pub async fn create_resource_tool(
        &self,
        Parameters(args): Parameters<CreateResourceArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let report = create_resource(
            &PathBuf::from(args.path),
            &args.name,
            &args.fields,
            args.transport,
        )
        .map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "create_dto",
        description = "Scaffold a single DTO with the given fields and validators.",
        annotations(destructive_hint = true)
    )]
    pub async fn create_dto_tool(
        &self,
        Parameters(args): Parameters<CreateDtoArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let report =
            create_dto(&PathBuf::from(args.path), &args.name, &args.fields).map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        ))
    }

    #[tool(
        name = "generate_crud",
        description = "Generate a full resource across multiple transports.",
        annotations(destructive_hint = true)
    )]
    pub async fn generate_crud_tool(
        &self,
        Parameters(args): Parameters<GenerateCrudArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let spec = CrudSpec {
            resource: args.resource,
            fields: args.fields,
            transports: args.transports,
        };
        let report = generate_crud(&PathBuf::from(args.path), &spec).map_err(into_rmcp)?;
        Ok(Json(
            serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        ))
    }
}

#[tool_handler]
impl ScaffoldTools {}
