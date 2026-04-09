//! Lightweight request metadata in [`axum::http::Extensions`] (Nest-style CLS analogue).
//!
//! Enable with [`crate::NestApplication::use_request_context`]. Handlers read it with the
//! [`RequestContext`] extractor.

use axum::extract::Request;
use axum::http::request::Parts;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Snapshot of the inbound request for use inside handlers (clone is cheap: three small fields).
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub method: Method,
    /// Path and query only (no scheme/host), e.g. `/v1/api/items?q=1`.
    pub path_and_query: String,
    /// Value of `x-request-id` after tower-http request-id layers, if any.
    pub request_id: Option<String>,
}

/// Returned when [`RequestContext`] is used but [`crate::NestApplication::use_request_context`] was not enabled.
#[derive(Debug)]
pub struct RequestContextMissing;

impl IntoResponse for RequestContextMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: RequestContext extractor requires NestApplication::use_request_context()",
        )
            .into_response()
    }
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
{
    type Rejection = RequestContextMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .ok_or(RequestContextMissing)
    }
}

pub(crate) async fn install_request_context_middleware(req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    let request_id = parts
        .headers
        .get(&X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| parts.uri.path().to_owned());
    parts.extensions.insert(RequestContext {
        method: parts.method.clone(),
        path_and_query,
        request_id,
    });
    let req = Request::from_parts(parts, body);
    next.run(req).await
}
