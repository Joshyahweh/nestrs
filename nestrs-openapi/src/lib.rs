//! OpenAPI 3.1 + Swagger UI for `nestrs`.
//!
//! ## What is generated
//!
//! - **`paths`** from [`nestrs_core::RouteRegistry`] (filled by `impl_routes!` / `#[routes]`).
//! - Each operation includes **`operationId`** (module path + handler), a short **`summary`** derived
//!   from the handler name (overridable with `#[openapi(summary = \"...\")]`), and a **`tags`** entry
//!   inferred from the URL or overridden with `#[openapi(tag = \"...\")]`.
//! - Responses default to **`200 OK`** only unless you set `#[openapi(responses = ((404, \"...\"), ...))]`;
//!   request/response **schemas are not** derived from Rust types (unlike Nest `@ApiProperty` / class-validator reflection).
//!
//! ## Nest `@nestjs/swagger` parity (practical)
//!
//! | Nest / Swagger idea | nestrs-openapi |
//! |---------------------|----------------|
//! | `@ApiTags` / controller grouping | Inferred **`tags`** from path; optional document-level [`OpenApiOptions::document_tags`]. |
//! | `@ApiOperation` summary | Auto **`summary`** from handler name; override with **`#[openapi(summary = \"...\")]`**. |
//! | `@ApiResponse` / status codes | Default **200** only; per-route **`#[openapi(responses = ((404, \"...\"), ...))]`** or manual `components`. |
//! | DTO / schema generation | **Not built-in** — hand-author [`OpenApiOptions::components`].**schemas** or merge output from **`utoipa`** / **`okapi`** / other generators; see the repo mdBook **OpenAPI & HTTP** (`docs/src/openapi-http.md`). |
//! | `@ApiBearerAuth` / route security | Global [`OpenApiOptions::security`] + [`OpenApiOptions::components`].**securitySchemes**; optional **per-route** [`OpenApiOptions::infer_route_security_from_roles`] (uses [`nestrs_core::MetadataRegistry`] **`roles`** from `#[roles]`). |
//! | Swagger UI | **Yes** — bundled HTML page at [`OpenApiOptions::docs_path`]. |
//! | Plugins (CLI, extra decorators) | **No** — keep this crate small; compose with other OpenAPI tools if needed. |

use axum::extract::State;
use axum::response::Html;
use axum::{Json, Router};
use nestrs_core::{MetadataRegistry, OpenApiRouteSpec, RouteInfo, RouteRegistry};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct OpenApiOptions {
    pub title: String,
    pub version: String,
    pub json_path: String,
    pub docs_path: String,
    /// Prefix applied to **API routes** (e.g. global prefix + URI versioning).
    pub api_prefix: String,
    /// Optional OpenAPI [`servers`](https://spec.openapis.org/oas/v3.1.0#server-object) array.
    pub servers: Option<Vec<Value>>,
    /// Optional top-level [`tags`](https://spec.openapis.org/oas/v3.1.0#tag-object) (name + description for Swagger UI).
    pub document_tags: Option<Vec<Value>>,
    /// Optional [`components`](https://spec.openapis.org/oas/v3.1.0#components-object) (e.g. `securitySchemes`, `schemas`).
    pub components: Option<Value>,
    /// Optional root [`security`](https://spec.openapis.org/oas/v3.1.0#openapi-security) requirements.
    pub security: Option<Vec<Value>>,
    /// When **true**, any route whose handler has **`roles`** metadata (set by `#[roles(...)]` on the
    /// handler) gets an operation-level **`security`** entry referencing [`Self::roles_security_scheme`].
    ///
    /// You must still declare that scheme under [`Self::components`].**`securitySchemes`** (for example
    /// `bearerAuth`). This is a **heuristic** bridge to Swagger “lock” icons — it does not inspect
    /// guard types or `CanActivate` implementations.
    pub infer_route_security_from_roles: bool,
    /// Scheme name used when [`Self::infer_route_security_from_roles`] is enabled (OpenAPI object key,
    /// e.g. `"bearerAuth"` matching `components.securitySchemes.bearerAuth`).
    pub roles_security_scheme: String,
}

impl Default for OpenApiOptions {
    fn default() -> Self {
        Self {
            title: "nestrs API".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            json_path: "/openapi.json".to_string(),
            docs_path: "/docs".to_string(),
            api_prefix: "".to_string(),
            servers: None,
            document_tags: None,
            components: None,
            security: None,
            infer_route_security_from_roles: false,
            roles_security_scheme: "bearerAuth".to_string(),
        }
    }
}

pub fn openapi_router(options: OpenApiOptions) -> Router {
    Router::new()
        .route(&options.json_path, axum::routing::get(openapi_json))
        .route(&options.docs_path, axum::routing::get(openapi_docs))
        .with_state(options)
}

async fn openapi_json(State(options): State<OpenApiOptions>) -> Json<Value> {
    let routes = RouteRegistry::list();
    let mut paths: BTreeMap<String, Value> = BTreeMap::new();

    for r in routes {
        let method = r.method.to_ascii_lowercase();
        if method == "all" {
            continue;
        }
        let full_path = join_prefix(&options.api_prefix, r.path);

        let entry = paths.entry(full_path.clone()).or_insert_with(|| json!({}));
        let obj = entry.as_object_mut().expect("path entry object");
        obj.insert(method, build_operation(&full_path, &r, &options));
    }

    let mut root = Map::new();
    root.insert("openapi".into(), json!("3.1.0"));
    root.insert(
        "info".into(),
        json!({ "title": options.title, "version": options.version }),
    );
    root.insert("paths".into(), json!(paths));

    if let Some(ref servers) = options.servers {
        if !servers.is_empty() {
            root.insert("servers".into(), json!(servers));
        }
    }
    if let Some(ref tags) = options.document_tags {
        if !tags.is_empty() {
            root.insert("tags".into(), json!(tags));
        }
    }
    if let Some(ref security) = options.security {
        if !security.is_empty() {
            root.insert("security".into(), json!(security));
        }
    }
    if let Some(components) = options.components.clone() {
        root.insert("components".into(), components);
    }

    Json(Value::Object(root))
}

fn build_operation(full_path: &str, route: &RouteInfo, options: &OpenApiOptions) -> Value {
    let handler = route.handler;
    let spec = route.openapi;
    let summary = spec
        .and_then(|s| s.summary)
        .map(str::to_string)
        .unwrap_or_else(|| humanize_handler(handler));
    let tag = spec
        .and_then(|s| s.tag)
        .map(str::to_string)
        .unwrap_or_else(|| infer_tag_from_path(full_path));
    let responses = build_responses(spec);
    let mut op = Map::new();
    op.insert("operationId".into(), json!(handler));
    op.insert("summary".into(), json!(summary));
    op.insert("tags".into(), json!([tag]));
    op.insert("responses".into(), responses);
    if options.infer_route_security_from_roles && MetadataRegistry::get(handler, "roles").is_some()
    {
        let mut req = Map::new();
        req.insert(
            options.roles_security_scheme.clone(),
            Value::Array(Vec::new()),
        );
        op.insert("security".into(), Value::Array(vec![Value::Object(req)]));
    }
    Value::Object(op)
}

fn build_responses(spec: Option<&OpenApiRouteSpec>) -> Value {
    let Some(s) = spec else {
        return json!({
            "200": { "description": "OK" }
        });
    };
    if s.responses.is_empty() {
        return json!({
            "200": { "description": "OK" }
        });
    }
    let mut map = Map::new();
    for d in s.responses {
        map.insert(
            d.status.to_string(),
            json!({ "description": d.description }),
        );
    }
    Value::Object(map)
}

fn humanize_handler(handler: &str) -> String {
    let base = handler.rsplit("::").next().unwrap_or(handler).trim();
    let words = base.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// First meaningful path segment after an optional `v123` version segment.
fn infer_tag_from_path(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut idx = 0;
    if let Some(s) = segs.get(idx) {
        let is_ver =
            s.len() > 1 && s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit());
        if is_ver {
            idx += 1;
        }
    }
    segs.get(idx)
        .map(|s| (*s).to_string())
        .unwrap_or_else(|| "default".to_string())
}

async fn openapi_docs(State(options): State<OpenApiOptions>) -> Html<String> {
    let url = options.json_path;
    Html(format!(
        r##"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Swagger UI</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist/swagger-ui.css" />
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist/swagger-ui-bundle.js"></script>
    <script>
      window.onload = () => {{
        SwaggerUIBundle({{
          url: "{url}",
          dom_id: "#swagger-ui"
        }});
      }};
    </script>
  </body>
</html>"##
    ))
}

fn join_prefix(prefix: &str, path: &str) -> String {
    let p = prefix.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return path.to_string();
    }
    if path == "/" {
        return p.to_string();
    }
    format!("{}/{}", p, path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_strips_module_path() {
        assert_eq!(
            humanize_handler("my_app::controllers::UserController::list"),
            "List"
        );
        assert_eq!(humanize_handler("mod:: ping"), "Ping");
    }

    #[test]
    fn infer_tag_skips_version_segment() {
        assert_eq!(infer_tag_from_path("/v1/o/ping"), "o");
        assert_eq!(infer_tag_from_path("/api/users"), "api");
    }
}
