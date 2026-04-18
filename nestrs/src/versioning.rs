//! API versioning helpers (Nest [`VersioningType`](https://docs.nestjs.com/techniques/versioning)):
//! URI prefix is handled by [`crate::NestApplication::enable_uri_versioning`]; this module adds
//! **header** and **`Accept`** negotiation plus optional [`NestApiVersion`] request extensions.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::IntoResponse;
use std::collections::HashSet;
use std::sync::Arc;

/// Resolved API version for the current request (e.g. `"v1"`, `"2"`).
#[derive(Clone, Debug)]
pub struct NestApiVersion(pub String);

/// Nest-style versioning modes beyond URI segments (see [`VersioningType`](https://docs.nestjs.com/techniques/versioning)).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersioningType {
    /// Version appears as the first URI segment (`/v1/...`) — use [`crate::NestApplication::enable_uri_versioning`].
    Uri,
    /// Version is read from a header (default `X-API-Version`).
    Header,
    /// Version is parsed from `Accept`, e.g. `application/vnd.api+json;version=2`.
    MediaType,
}

/// Policy for [`crate::NestApplication::enable_api_versioning`].
#[derive(Clone, Debug)]
pub struct ApiVersioningPolicy {
    pub kind: VersioningType,
    /// Header name when [`VersioningType::Header`] (default `X-API-Version`).
    pub header_name: Option<String>,
    /// Used when the client omits a version (header or `Accept`).
    pub default_version: Option<String>,
}

impl Default for ApiVersioningPolicy {
    fn default() -> Self {
        Self {
            kind: VersioningType::Header,
            header_name: None,
            default_version: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ApiVersioningState {
    pub policy: ApiVersioningPolicy,
    pub route_root_prefix: String,
    pub versioned_paths: HashSet<String>,
}

pub(crate) async fn api_version_middleware(
    axum::extract::State(state): axum::extract::State<Arc<ApiVersioningState>>,
    mut req: Request,
    next: Next,
) -> axum::response::Response {
    let resolved = match state.policy.kind {
        VersioningType::Uri => {
            return next.run(req).await;
        }
        VersioningType::Header => {
            let name = state
                .policy
                .header_name
                .as_deref()
                .unwrap_or("x-api-version")
                .parse::<axum::http::HeaderName>()
                .unwrap_or(axum::http::HeaderName::from_static("x-api-version"));
            req.headers()
                .get(&name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .map(|s| s.to_string())
                .or_else(|| state.policy.default_version.clone())
        }
        VersioningType::MediaType => req
            .headers()
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_version_from_accept)
            .or_else(|| state.policy.default_version.clone()),
    };

    if let Some(v) = resolved {
        if !v.is_empty() {
            rewrite_request_path_for_version(&mut req, &state, &v);
            req.extensions_mut().insert(NestApiVersion(v));
        }
    }

    next.run(req).await
}

fn rewrite_request_path_for_version(
    req: &mut Request,
    state: &ApiVersioningState,
    resolved_version: &str,
) {
    let normalized_version = resolved_version.trim_matches('/');
    if normalized_version.is_empty() {
        return;
    }

    let request_path = req.uri().path();
    let Some(app_path) = strip_root_prefix(request_path, &state.route_root_prefix) else {
        return;
    };

    let candidate_app_path = insert_version_segment(app_path, normalized_version);
    if !state.versioned_paths.contains(&candidate_app_path) {
        return;
    }

    let full_path = apply_root_prefix(&state.route_root_prefix, &candidate_app_path);
    let path_and_query = if let Some(query) = req.uri().query() {
        format!("{full_path}?{query}")
    } else {
        full_path
    };

    let mut parts = req.uri().clone().into_parts();
    let parsed = match path_and_query.parse() {
        Ok(v) => v,
        Err(_) => return,
    };
    parts.path_and_query = Some(parsed);
    if let Ok(uri) = axum::http::Uri::from_parts(parts) {
        *req.uri_mut() = uri;
    }
}

fn strip_root_prefix<'a>(path: &'a str, root_prefix: &str) -> Option<&'a str> {
    if root_prefix.is_empty() {
        return Some(path);
    }
    if path == root_prefix {
        return Some("/");
    }
    let rest = path.strip_prefix(root_prefix)?;
    if rest.starts_with('/') {
        Some(rest)
    } else {
        None
    }
}

fn insert_version_segment(path: &str, version: &str) -> String {
    let version_prefix = format!("/{version}");
    if path == version_prefix || path.starts_with(&format!("{version_prefix}/")) {
        return path.to_string();
    }
    if path == "/" {
        return version_prefix;
    }
    format!("{version_prefix}{}", path)
}

fn apply_root_prefix(root_prefix: &str, app_path: &str) -> String {
    if root_prefix.is_empty() {
        return app_path.to_string();
    }
    if app_path == "/" {
        return root_prefix.to_string();
    }
    format!("{root_prefix}{app_path}")
}

fn parse_version_from_accept(accept: &str) -> Option<String> {
    for part in accept.split(',') {
        let part = part.trim();
        if let Some(idx) = part.find(';') {
            let params = &part[idx + 1..];
            for p in params.split(';') {
                let p = p.trim();
                let rest = p.strip_prefix("version=")?;
                let rest = rest.trim_matches(|c| c == '"' || c == '\'');
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}

/// Rejects requests whose `Host` header does not match `expected` (port suffix ignored).
pub async fn host_restriction_middleware(
    axum::extract::State(expected): axum::extract::State<&'static str>,
    req: Request,
    next: Next,
) -> axum::response::Response {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host = strip_port(host);
    if host == expected {
        next.run(req).await
    } else {
        crate::NotFoundException::new(format!(
            "Host `{host}` does not match required host `{expected}`"
        ))
        .into_response()
    }
}

fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return &rest[..end];
        }
    }
    host.split(':').next().unwrap_or(host)
}
