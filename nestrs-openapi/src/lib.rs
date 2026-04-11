//! OpenAPI/Swagger integration for `nestrs`.

use axum::extract::State;
use axum::response::Html;
use axum::{Json, Router};
use nestrs_core::RouteRegistry;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct OpenApiOptions {
    pub title: String,
    pub version: String,
    pub json_path: String,
    pub docs_path: String,
    /// Prefix applied to **API routes** (e.g. global prefix + URI versioning).
    pub api_prefix: String,
}

impl Default for OpenApiOptions {
    fn default() -> Self {
        Self {
            title: "nestrs API".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            json_path: "/openapi.json".to_string(),
            docs_path: "/docs".to_string(),
            api_prefix: "".to_string(),
        }
    }
}

pub fn openapi_router(options: OpenApiOptions) -> Router {
    Router::new()
        .route(&options.json_path, axum::routing::get(openapi_json))
        .route(&options.docs_path, axum::routing::get(openapi_docs))
        .with_state(options)
}

async fn openapi_json(State(options): State<OpenApiOptions>) -> Json<serde_json::Value> {
    let routes = RouteRegistry::list();
    let mut paths: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for r in routes {
        let method = r.method.to_ascii_lowercase();
        if method == "all" {
            continue;
        }
        let full_path = join_prefix(&options.api_prefix, r.path);

        let entry = paths.entry(full_path).or_insert_with(|| json!({}));
        let obj = entry.as_object_mut().expect("path entry object");
        obj.insert(
            method,
            json!({
                "operationId": r.handler,
                "responses": {
                    "200": { "description": "OK" }
                }
            }),
        );
    }

    Json(json!({
        "openapi": "3.1.0",
        "info": { "title": options.title, "version": options.version },
        "paths": paths,
    }))
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
