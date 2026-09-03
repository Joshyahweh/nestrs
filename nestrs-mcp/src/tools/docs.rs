//! Local-file docs search tools (read-only). Index is built lazily on
//! first use per `workspace_path`.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData, Json};
use serde::{Deserialize, Serialize};

use crate::docs::{DocKind, DocSearcher, DocStore, SearchScope};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocsArgs {
    /// Absolute path to the nestrs workspace root.
    pub workspace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Absolute path to the nestrs workspace root.
    pub workspace_path: String,
    pub query: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetDocArgs {
    /// Absolute path to the nestrs workspace root.
    pub workspace_path: String,
    /// Workspace-relative path of the doc to fetch.
    pub path: String,
}

fn default_scope() -> String {
    "all".to_string()
}
fn default_limit() -> usize {
    20
}

static STORES: Lazy<DashMap<String, Arc<DocStore>>> = Lazy::new(DashMap::new);

fn store_for(workspace_path: &str) -> Result<Arc<DocStore>, crate::Error> {
    if let Some(s) = STORES.get(workspace_path) {
        return Ok(s.clone());
    }
    let s = Arc::new(DocStore::new());
    s.build(&PathBuf::from(workspace_path))?;
    STORES.insert(workspace_path.to_string(), s.clone());
    Ok(s)
}

fn parse_scope(s: &str) -> SearchScope {
    match s {
        "changelog" => SearchScope::Changelog,
        "book" => SearchScope::Book,
        "readme" => SearchScope::Readme,
        _ => SearchScope::All,
    }
}

fn into_rmcp(e: crate::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

#[derive(Debug)]
pub struct DocsTools;

#[tool_router]
impl DocsTools {
    #[tool(
        name = "search_docs",
        description = "Search CHANGELOG, mdBook, and READMEs for a query. Returns ranked hits with context lines.",
        annotations(read_only_hint = true)
    )]
    pub async fn search_docs(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let store = store_for(&args.workspace_path).map_err(into_rmcp)?;
        let hits = DocSearcher::search(
            &store.sources(),
            &args.query,
            parse_scope(&args.scope),
            args.limit,
        );
        Ok(Json(
            hits.into_iter()
                .map(serde_json::to_value)
                .filter_map(Result::ok)
                .collect(),
        ))
    }

    #[tool(
        name = "get_changelog",
        description = "Read the CHANGELOG.md (returns a list of version/date headings).",
        annotations(read_only_hint = true)
    )]
    pub async fn get_changelog(
        &self,
        Parameters(args): Parameters<DocsArgs>,
    ) -> Result<Json<Vec<serde_json::Value>>, ErrorData> {
        let store = store_for(&args.workspace_path).map_err(into_rmcp)?;
        let source = store
            .sources()
            .into_iter()
            .find(|s| matches!(s.kind, DocKind::Changelog));
        let entries = match source {
            Some(s) => crate::docs::changelog_entries(&s),
            None => Vec::new(),
        };
        Ok(Json(
            entries
                .into_iter()
                .map(serde_json::to_value)
                .filter_map(Result::ok)
                .collect(),
        ))
    }

    #[tool(
        name = "get_doc",
        description = "Get one doc file's content by path (relative to the workspace).",
        annotations(read_only_hint = true)
    )]
    pub async fn get_doc(
        &self,
        Parameters(args): Parameters<GetDocArgs>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let store = store_for(&args.workspace_path).map_err(into_rmcp)?;
        let needle = std::path::Path::new(&args.path);
        let source = store
            .sources()
            .into_iter()
            .find(|s| s.path.ends_with(needle));
        match source {
            Some(s) => Ok(Json(serde_json::json!({
                "path": s.path.to_string_lossy(),
                "kind": format!("{:?}", s.kind).to_ascii_lowercase(),
                "content": s.content,
            }))),
            None => Err(into_rmcp(crate::Error::FileNotFound(format!(
                "doc `{}` not in workspace",
                args.path
            )))),
        }
    }
}

#[tool_handler]
impl DocsTools {}
