//! W3C Trace Context middleware (enable with [`crate::NestApplication::use_trace_context`]).
//!
//! Parses the `traceparent` header (validation per the W3C Trace Context spec,
//! see `nestrs::core::parse_traceparent`) and installs an ambient
//! [`nestrs::core::TraceContext`] task-local for the request. Handlers and
//! anything running inside the request task (GraphQL resolvers, guard chains)
//! read it via `nestrs::core::current_trace_context()` or the accessors on
//! [`crate::core::ExecutionContext`].
//!
//! Requests without a valid `traceparent` run without a trace context — the
//! accessors return `None` (no synthetic trace id is generated; producing
//! spans is an `otel`-feature concern, out of scope here).

use crate::core::{parse_traceparent, with_trace_context, TraceContext};
use axum::extract::Request;
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;

static TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");
static TRACESTATE: HeaderName = HeaderName::from_static("tracestate");

pub async fn install_trace_context_middleware(req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let parsed = parts
        .headers
        .get(&TRACEPARENT)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_traceparent);
    let tracestate = parts
        .headers
        .get(&TRACESTATE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let req = Request::from_parts(parts, body);
    match parsed {
        Some(mut ctx) => {
            ctx.tracestate = tracestate;
            with_trace_context(ctx, next.run(req)).await
        }
        // No (valid) traceparent: run without installing a trace context.
        None => next.run(req).await,
    }
}

/// Read of the ambient trace context installed by
/// [`crate::NestApplication::use_trace_context`]. Prefer the
/// `ExecutionContext` accessors or `nestrs::core::current_trace_context()`;
/// this exists for symmetry with `RequestContext` / `HttpExecutionContext`.
pub fn current_trace() -> Option<TraceContext> {
    crate::core::current_trace_context()
}
