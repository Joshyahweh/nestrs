//! Global throttler middleware. Installed by `NestApplication::use_throttler`
//! via `axum::middleware::from_fn_with_state`.
//!
//! Runs before auth guards; attaches `Retry-After` / `X-RateLimit-*` headers
//! on 429. Per-route decorators are evaluated by [`crate::ThrottlerGuard`]
//! after the handler resolves — the middleware only sees the *global* spec.

use crate::keys::ThrottlerRequest;
use crate::module::{ThrottleSpecFor, ThrottlerState};
use crate::spec::{ThrottleOutcome, ThrottleSpec};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::Response;
use axum::middleware::Next;
use nestrs_core::client_ip::{rate_limit_key_ip_or_unknown, trusted_hops_from_parts};
use std::sync::Arc;

pub async fn throttler_middleware(
    State(state): State<Arc<ThrottlerState>>,
    req: Request,
    next: Next,
) -> Response {
    let (parts, body) = req.into_parts();
    let trusted_hops = trusted_hops_from_parts(&parts, state.trusted_proxy_hops);
    let ip = rate_limit_key_ip_or_unknown(&parts.headers, &parts.extensions, trusted_hops);

    // Global middleware path: no handler known yet, no per-route decorator
    // looked up here (that's `ThrottlerGuard`'s job). The only spec the
    // middleware can apply is the module's `global` fallback.
    let spec = ThrottleSpecFor::resolve(false, None, state.global);

    if let Some(spec) = spec {
        let tr_req = ThrottlerRequest {
            handler: "",
            ip: ip.clone(),
            parts: &parts,
        };
        if !state.skipper.skip(&tr_req) {
            let key = state.key_generator.key(&tr_req);
            let outcome = state.service.check("", &key, &spec).await;
            if let ThrottleOutcome::Limited {
                retry_after_secs,
            } = outcome
            {
                return too_many_response(retry_after_secs, &spec);
            }
        }
    }

    let req = Request::from_parts(parts, body);
    next.run(req).await
}

fn too_many_response(retry_after_secs: u64, spec: &ThrottleSpec) -> Response {
    let body = Body::from(
        serde_json::json!({"statusCode": 429, "message": "Too Many Requests"}).to_string(),
    );
    Response::builder()
        .status(axum::http::StatusCode::TOO_MANY_REQUESTS)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header("retry-after", retry_after_secs.to_string())
        .header("x-ratelimit-limit", spec.limit.to_string())
        .header("x-ratelimit-remaining", "0")
        .header("x-ratelimit-reset", retry_after_secs.to_string())
        .body(body)
        .expect("static 429 response builds")
}
