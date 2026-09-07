//! Response **masking** interceptor — strips fields from JSON response bodies
//! when the request's resolved [`Ability`] says the principal can't read them.
//!
//! ## When it runs
//!
//! The interceptor is applied via [`crate::interceptor_layer!`] on a router or
//! a single route. It must run *after* `install_policies_middleware` so the
//! `Arc<Ability>` is available in `parts.extensions`. If no ability is found
//! (anonymous request, or authz middleware not installed), the response is
//! passed through untouched.
//!
//! ## Subject detection
//!
//! Best-effort: looks for a top-level `"type": "Post"` field in the JSON
//! response. When present, `Ability::allowed_fields(Read, Subject::Type("Post"))`
//! returns the field allow-list; any top-level field not in that list is
//! removed. Nested objects are recursed one level. Arrays of objects are
//! recursed. Responses with no `"type"` marker are passed through unchanged —
//! a no-op rather than a hard fail.
//!
//! ## Body size cap
//!
//! Bodies larger than 1 MiB are passed through unmasked (with a `tracing`
//! `warn!`) so a malicious or accidentally large response cannot OOM the
//! server. The cap is conservative and configurable via
//! [`MaskingConfig::max_body_bytes`].
//!
//! ## Test
//!
//! See `tests/masking_module.rs` for end-to-end coverage.

use crate::interceptor::Interceptor;
use crate::policies::{Ability, Action, Subject};
use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::Value;

/// Hard cap for the response body we'll read into memory before masking.
/// Beyond this, the body is passed through unmasked.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Tunables for [`PolicyMaskingInterceptor`].
#[derive(Clone, Debug)]
pub struct MaskingConfig {
    /// Maximum body size (in bytes) we'll buffer for masking. Larger bodies
    /// pass through unmasked.
    pub max_body_bytes: usize,
}

impl Default for MaskingConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

/// Interceptor that strips fields from JSON responses when the request's
/// `Ability` doesn't grant the principal `read` on the corresponding subject.
///
/// See module docs for behavior, body cap, and subject detection rules.
#[derive(Default)]
pub struct PolicyMaskingInterceptor;

#[async_trait::async_trait]
impl Interceptor for PolicyMaskingInterceptor {
    async fn intercept(&self, req: Request, next: Next) -> Response {
        let ability = req.extensions().get::<std::sync::Arc<Ability>>().cloned();
        let response = next.run(req).await;
        let Some(ability) = ability else {
            return response;
        };
        mask_response(response, &ability, MaskingConfig::default()).await
    }
}

/// Public helper: apply masking to an already-constructed response using the
/// given ability. Pulled out so tests can exercise the logic without a full
/// router. Body cap and the pass-through-when-no-`type`-marker rules from the
/// module docs apply here too.
pub async fn mask_response(response: Response, ability: &Ability, cfg: MaskingConfig) -> Response {
    // Only mask JSON responses.
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false);
    if !is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, cfg.max_body_bytes).await {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!(
                target: "nestrs::masking",
                "response body exceeds cap; passing through unmasked"
            );
            return Response::from_parts(parts, Body::empty());
        }
    };
    if bytes.len() >= cfg.max_body_bytes {
        tracing::warn!(
            target: "nestrs::masking",
            "response body at or above cap; passing through unmasked"
        );
        return Response::from_parts(parts, Body::from(bytes));
    }

    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };
    mask_value(&mut value, ability);
    let new_bytes = match serde_json::to_vec(&value) {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };
    let mut new_parts = parts;
    new_parts.headers.remove(header::CONTENT_LENGTH);
    new_parts.headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    Response::from_parts(new_parts, Body::from(new_bytes))
}

/// Recursive masker. Top-level value: detect `type`; recurse on objects and
/// arrays. For each detected subject, drop fields not in the allow-list.
///
/// `pub` so transport crates (WS, GraphQL, MCP) can re-use the same walker
/// on `serde_json::Value` payloads they produce directly, without going
/// through the HTTP [`mask_response`] path.
pub fn mask_value(value: &mut Value, ability: &Ability) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(type_name)) = map.get("type") {
                let subject = Subject::Type(Box::leak(type_name.clone().into_boxed_str()));
                if let Some(allowed) = ability.allowed_fields(&Action::Read, &subject) {
                    let allowed: std::collections::HashSet<&str> =
                        allowed.iter().map(|s| s.as_str()).collect();
                    map.retain(|k, _| allowed.contains(k.as_str()));
                }
            }
            for v in map.values_mut() {
                mask_value(v, ability);
            }
        }
        Value::Array(items) => {
            for v in items {
                mask_value(v, ability);
            }
        }
        _ => {}
    }
}

/// Convenience: a 200 response with a JSON body. Used by tests and the
/// "pass-through" docs to construct sample responses.
pub fn json_response<T: serde::Serialize>(status: StatusCode, body: T) -> Response {
    (status, axum::Json(body)).into_response()
}
