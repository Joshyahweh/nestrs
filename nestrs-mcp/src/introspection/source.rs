//! `syn`-based source parser for nestrs projects.
//!
//! Walks `<workspace>/src/**/*.rs`, finds `#[module(...)]`, `#[controller(...)]`,
//! `#[routes]` impls, `#[dto(...)]` structs, `#[injectable(...)]` providers, and
//! the various per-fn attributes (`#[get/post/...]`, `#[use_guards(...)]`, etc.)
//! without depending on `nestrs-macros` (which is `proc-macro` only and would
//! create a build-time circular dep).
//!
//! Unknown attributes are surfaced as `ParserWarning`s rather than failing the
//! parse — this keeps the parser robust as new macros are added.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

use super::metadata::DtoSummary;

#[derive(Debug, Error)]
pub enum SourceParserError {
    #[error("workspace path does not exist: {0}")]
    WorkspaceNotFound(PathBuf),
    #[error("workspace path is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("`Cargo.toml` not found in {0} — is this a Cargo workspace?")]
    NoCargoToml(PathBuf),
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("syn parse error in {file}: {message}")]
    Syn { file: String, message: String },
}

/// A non-fatal diagnostic from the parser. New macros added to
/// `nestrs-macros` show up here until the parser learns them.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ParserWarning {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub message: String,
}

/// One route (handler) inside a controller. Mirrors the per-fn attributes
/// `nestrs-macros` recognizes on items inside a `#[routes]` impl.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RouteSummary {
    pub method: String,
    pub path: String,
    pub handler: String,
    pub version: Option<String>,
    pub guards: Vec<String>,
    pub interceptors: Vec<String>,
    pub pipes: Vec<String>,
    pub filters: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    /// Body extractor type, if any (e.g. `ValidatedBody<CreateUserDto>`).
    pub body_type: Option<String>,
    /// Response type, inferred from the handler's return type.
    pub response_type: Option<String>,
}

/// One controller struct. Mirrors `#[controller("/path"[, version, host])]`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ControllerSummary {
    pub name: String,
    /// Module path inferred from the file (e.g. `"myapp::users"`).
    pub module_path: String,
    pub file: String,
    /// Controller prefix from `#[controller("/users")]`.
    pub prefix: Option<String>,
    pub version: Option<String>,
    pub host: Option<String>,
    /// Routes parsed from the `#[routes]` impl on this struct, if any.
    pub routes: Vec<RouteSummary>,
    /// Guards declared on the `#[routes(controller_guards = (...))]` arg.
    pub controller_guards: Vec<String>,
    /// State type from `#[routes(state = T)]`, if any.
    pub state: Option<String>,
}

/// One provider (anything with `#[injectable]` or registered via
/// `#[module(providers = [...])]`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderSummary {
    pub type_name: String,
    pub file: String,
    /// `singleton` / `transient` / `request` from `#[injectable(scope = ...)]`.
    pub scope: Option<String>,
    /// True if the struct is annotated with `#[injectable]`.
    pub is_injectable: bool,
}

/// One `#[module(...)]` impl.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSummary {
    pub name: String,
    pub file: String,
    pub imports: Vec<String>,
    pub controllers: Vec<String>,
    pub providers: Vec<String>,
    pub microservices: Vec<String>,
    pub exports: Vec<String>,
    pub re_exports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStats {
    pub files_scanned: usize,
    pub modules: usize,
    pub controllers: usize,
    pub providers: usize,
    pub routes: usize,
    pub dtos: usize,
    pub warnings: usize,
}

/// The full parsed workspace. This is the unit returned by every
/// introspection tool (individual tools extract a subset).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ParsedWorkspace {
    pub root: PathBuf,
    pub modules: Vec<ModuleSummary>,
    pub controllers: Vec<ControllerSummary>,
    pub providers: Vec<ProviderSummary>,
    pub dtos: Vec<DtoSummary>,
    pub schedules: Vec<String>,
    pub event_handlers: Vec<String>,
    pub queue_processors: Vec<String>,
    pub stats: WorkspaceStats,
    pub warnings: Vec<ParserWarning>,
}

/// The parser itself. Constructed with a workspace root, then `.parse()` walks
/// the `src/` tree.
#[derive(Debug)]
pub struct SourceParser {
    root: PathBuf,
}

impl SourceParser {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, SourceParserError> {
        let root = root.as_ref().to_path_buf();
        if !root.exists() {
            return Err(SourceParserError::WorkspaceNotFound(root));
        }
        if !root.is_dir() {
            return Err(SourceParserError::NotADirectory(root));
        }
        if !root.join("Cargo.toml").exists() {
            return Err(SourceParserError::NoCargoToml(root));
        }
        Ok(Self { root })
    }

    /// Walk the workspace and return every recognized item.
    pub fn parse(&self) -> Result<ParsedWorkspace, SourceParserError> {
        let src = self.root.join("src");
        let mut files_scanned = 0usize;
        let mut modules = Vec::new();
        let mut controllers = Vec::new();
        let mut providers = Vec::new();
        let mut dtos = Vec::new();
        let mut schedules = Vec::new();
        let mut event_handlers = Vec::new();
        let mut queue_processors = Vec::new();
        let mut warnings = Vec::new();
        let mut route_count = 0usize;

        // If `src/` doesn't exist (e.g. workspace with no library/binary yet)
        // we still want a valid (empty) parse — scaffolding tools rely on
        // this for "new project" previews.
        let walker = if src.is_dir() {
            WalkDir::new(&src).into_iter()
        } else {
            WalkDir::new(&self.root).max_depth(1).into_iter()
        };

        for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            files_scanned += 1;
            let file = entry.path();
            let rel = file
                .strip_prefix(&self.root)
                .unwrap_or(file)
                .to_string_lossy()
                .into_owned();

            let content = match fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    warnings.push(ParserWarning {
                        file: rel.clone(),
                        line: 0,
                        kind: "io".into(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };

            let ast = match syn::parse_file(&content) {
                Ok(ast) => ast,
                Err(e) => {
                    warnings.push(ParserWarning {
                        file: rel.clone(),
                        line: 0,
                        kind: "syn".into(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };

            for item in &ast.items {
                self.visit_item(
                    item,
                    &rel,
                    &mut modules,
                    &mut controllers,
                    &mut providers,
                    &mut dtos,
                    &mut schedules,
                    &mut event_handlers,
                    &mut queue_processors,
                    &mut warnings,
                    &mut route_count,
                );
            }
        }

        let stats = WorkspaceStats {
            files_scanned,
            modules: modules.len(),
            controllers: controllers.len(),
            providers: providers.len(),
            routes: route_count,
            dtos: dtos.len(),
            warnings: warnings.len(),
        };

        Ok(ParsedWorkspace {
            root: self.root.clone(),
            modules,
            controllers,
            providers,
            dtos,
            schedules,
            event_handlers,
            queue_processors,
            stats,
            warnings,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_item(
        &self,
        item: &syn::Item,
        file: &str,
        modules: &mut Vec<ModuleSummary>,
        controllers: &mut Vec<ControllerSummary>,
        providers: &mut Vec<ProviderSummary>,
        dtos: &mut Vec<DtoSummary>,
        schedules: &mut Vec<String>,
        event_handlers: &mut Vec<String>,
        queue_processors: &mut Vec<String>,
        warnings: &mut Vec<ParserWarning>,
        route_count: &mut usize,
    ) {
        match item {
            syn::Item::Impl(item_impl) => {
                if let Some(module) =
                    parser::parse_module_impl(item_impl, file, warnings)
                {
                    modules.push(module);
                }
                // `#[routes(X)] impl X { ... }` — attach routes to the
                // existing controller named `X`, if we saw one earlier.
                // Also back-fill `state` and `controller_guards` from the
                // `#[routes(...)]` attr list onto the struct-form controller
                // (which has no `#[routes]` attrs of its own). Falls through
                // to the legacy `parse_controller_impl` path when no
                // struct-level controller was found.
                if let Some(target) = parser::parse_routes_target(item_impl) {
                    let (state, controller_guards) =
                        parser::parse_routes_args(item_impl);
                    for method in &item_impl.items {
                        if let syn::ImplItem::Fn(m) = method {
                            if let Some(r) = parser::parse_route_method(m, warnings) {
                                if let Some(c) = controllers
                                    .iter_mut()
                                    .find(|c| c.name == target)
                                {
                                    c.routes.push(r);
                                    *route_count += 1;
                                    // Back-fill from `#[routes(...)]` so the
                                    // source-level view matches the runtime
                                    // view. Only fill if the struct-form
                                    // controller didn't already declare them
                                    // (it never does, but be explicit).
                                    if c.state.is_none() {
                                        c.state = state.clone();
                                    }
                                    if c.controller_guards.is_empty() {
                                        c.controller_guards =
                                            controller_guards.clone();
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(controller) =
                    parser::parse_controller_impl(item_impl, file, warnings)
                {
                    *route_count += controller.routes.len();
                    controllers.push(controller);
                }
            }
            syn::Item::Struct(item_struct) => {
                if let Some(module) =
                    parser::parse_module_struct(item_struct, file, warnings)
                {
                    modules.push(module);
                }
                if let Some(controller) =
                    parser::parse_controller_struct(item_struct, file, warnings)
                {
                    controllers.push(controller);
                }
                if let Some(dto) =
                    parser::parse_dto_struct(item_struct, file, warnings)
                {
                    dtos.push(dto);
                }
                if parser::is_injectable_struct(item_struct) {
                    let scope = parser::parse_injectable_scope(item_struct);
                    providers.push(ProviderSummary {
                        type_name: parser::struct_name(item_struct),
                        file: file.to_string(),
                        scope,
                        is_injectable: true,
                    });
                }
            }
            syn::Item::Fn(item_fn) => {
                if let Some(kind) = parser::parse_schedule_attr(&item_fn.attrs) {
                    schedules.push(format!("{kind}::{}", parser::fn_name(item_fn)));
                }
                if let Some(kind) = parser::parse_event_attr(&item_fn.attrs) {
                    event_handlers.push(format!("{kind}::{}", parser::fn_name(item_fn)));
                }
                if let Some(kind) = parser::parse_queue_attr(&item_fn.attrs) {
                    queue_processors.push(format!("{kind}::{}", parser::fn_name(item_fn)));
                }
            }
            _ => {}
        }
    }
}

pub(super) mod parser;

// Used by the `parse_workspace` convenience in `mod.rs`.
//
// (The `From<SourceParserError> for crate::Error` impl lives in
// `crate::error` to avoid an orphan rule violation.)
