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
static TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");
static TRACESTATE: HeaderName = HeaderName::from_static("tracestate");

/// Longest client-supplied `x-request-id` we honor. Everything about the
/// value (length included) is attacker-chosen; a bound keeps it out of
/// logs, response headers, and tracing spans at a reasonable size.
const MAX_REQUEST_ID_LEN: usize = 128;

/// A client-supplied `x-request-id` is honored only when it is plain
/// identifier-ish ASCII: `[A-Za-z0-9._-]`, non-empty, at most
/// [`MAX_REQUEST_ID_LEN`] bytes. This covers UUID/ULID/hex/base64url-style
/// ids (legitimate upstream propagation) and rejects the rest — control
/// characters, spaces, unicode, and oversized values — which would
/// otherwise be echoed verbatim into logs, tracing, and the response
/// header by tower-http's request-id layers.
fn acceptable_request_id(raw: &[u8]) -> bool {
    !raw.is_empty()
        && raw.len() <= MAX_REQUEST_ID_LEN
        && raw.iter().all(|&b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
        })
}

/// Strips an unacceptable client-supplied `x-request-id` BEFORE the
/// tower-http request-id layers run (they only fill in a MISSING header —
/// whatever the client sent is honored as-is). The next layer then assigns
/// a fresh UUID, so unparseable traffic can never forge correlation ids or
/// smuggle log-forging payloads through the request-id channel.
pub(crate) async fn sanitize_request_id_middleware(req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    if let Some(v) = parts.headers.get(&X_REQUEST_ID) {
        if !acceptable_request_id(v.as_bytes()) {
            tracing::warn!(
                target: "nestrs::request_id",
                "client-supplied x-request-id rejected (invalid charset or length); \
                 a fresh UUID will be assigned"
            );
            parts.headers.remove(&X_REQUEST_ID);
        }
    }
    next.run(Request::from_parts(parts, body)).await
}

/// Snapshot of the inbound request for use inside handlers (clone is cheap: three small fields).
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub method: Method,
    /// Path and query only (no scheme/host), e.g. `/v1/api/items?q=1`.
    pub path_and_query: String,
    /// Value of `x-request-id` after tower-http request-id layers, if any.
    pub request_id: Option<String>,
    /// Raw W3C `traceparent` header value, when present (pair with
    /// [`crate::NestApplication::use_trace_context`] for the parsed ambient form).
    pub traceparent: Option<String>,
    /// Raw `tracestate` header value, when present.
    pub tracestate: Option<String>,
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
    let traceparent = parts
        .headers
        .get(&TRACEPARENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let tracestate = parts
        .headers
        .get(&TRACESTATE)
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
        traceparent,
        tracestate,
    });
    let req = Request::from_parts(parts, body);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    // --- request-id sanitization (audit #46) ---------------------------------
    //
    // tower-http's `SetRequestIdLayer` only fills in a MISSING
    // `x-request-id`; whatever the client sent is honored verbatim and
    // echoed into logs, tracing, and the response header. Only plain
    // identifier-ish ASCII survives (UUID/ULID/hex style); the rest is
    // stripped so a fresh UUID is assigned downstream.

    #[test]
    fn acceptable_request_id_ids_survive() {
        assert!(super::acceptable_request_id(b"incoming-rid"));
        assert!(super::acceptable_request_id(
            b"01890604-1d6f-7a46-9b0b-1f0b4ef7b9e3"
        ));
        assert!(super::acceptable_request_id(b"01H8ZX9JGK6QNBVCWDRT24A5B6Z"));
        assert!(super::acceptable_request_id(b"app.42_worker-3"));
        let max = "a".repeat(super::MAX_REQUEST_ID_LEN);
        assert!(super::acceptable_request_id(max.as_bytes()));
    }

    #[test]
    fn unacceptable_request_id_values_are_rejected() {
        assert!(!super::acceptable_request_id(b""), "empty");
        let too_long = "a".repeat(super::MAX_REQUEST_ID_LEN + 1);
        assert!(!super::acceptable_request_id(too_long.as_bytes()));
        assert!(!super::acceptable_request_id(b"with spaces"), "space");
        assert!(!super::acceptable_request_id(b"tab\tseparated"), "tab");
        // obs-text (high bytes) is legal in header values — reject it.
        assert!(!super::acceptable_request_id(b"caf\xc3\xa9"), "utf-8");
        // HeaderValue can carry 0x7F and most control bytes via
        // from_maybe_shared; the charset whitelist must not pass them.
        assert!(!super::acceptable_request_id(&[0x7f]), "DEL byte");
        assert!(!super::acceptable_request_id(&[0x1b, b'[']), "ANSI escape");
        assert!(
            !super::acceptable_request_id(b"../../../etc/passwd"),
            "path traversal-looking id"
        );
    }
}
