//! API versioning helpers (Nest [`VersioningType`](https://docs.nestjs.com/techniques/versioning)):
//! URI prefix is handled by [`crate::NestApplication::enable_uri_versioning`]; this module adds
//! **header** and **`Accept`** negotiation plus optional [`NestApiVersion`] request extensions.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::IntoResponse;
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

pub(crate) async fn api_version_middleware(
    axum::extract::State(state): axum::extract::State<Arc<ApiVersioningPolicy>>,
    mut req: Request,
    next: Next,
) -> axum::response::Response {
    let resolved = match state.kind {
        VersioningType::Uri => {
            return next.run(req).await;
        }
        VersioningType::Header => {
            let name = state
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
                .or_else(|| state.default_version.clone())
        }
        VersioningType::MediaType => req
            .headers()
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_version_from_accept)
            .or_else(|| state.default_version.clone()),
    };

    if let Some(v) = resolved {
        if !v.is_empty() {
            req.extensions_mut().insert(NestApiVersion(v));
        }
    }

    next.run(req).await
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
