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
//! Best-effort: looks for a `"type": "Post"` string on each JSON object.
//! When present, the principal must be able to `read` **that object**.
//! Conditions and row predicates are checked against the object's own
//! fields. A nested `"attributes"` object is ignored for that check, so it
//! cannot spoof a condition. `Ability::allowed_fields` then supplies the
//! field allow-list and any other field is removed. `None` from
//! `allowed_fields` means two different things: no rule matched, or a
//! matching grant has no field list. Only the second case leaves the
//! object intact. An object the principal cannot `read` — no grant, a
//! condition miss, or a predicate rejection — keeps the `type` marker and
//! drops its other scalar fields. Arrays are kept only when they hold a
//! nested subject, so a list wrapper the caller cannot read still
//! field-masks the subjects inside it. Scalar elements of those arrays
//! are removed, and an object in that array with no `type` marker loses
//! its scalar fields too. A scalar array on the denied object is removed.
//! Responses with no `"type"` marker are passed through unchanged —
//! a no-op rather than a hard fail.
//!
//! ## Body size cap
//!
//! Bodies larger than [`MaskingConfig::max_body_bytes`] (default 1 MiB) are
//! not returned unmasked. Buffering stops at the cap and the response body
//! is replaced with an empty one, so a payload sized to the cap cannot skip
//! field stripping. The same withhold applies when the bytes are not JSON
//! `serde_json` can parse (trailing data, or nesting deeper than the
//! parser accepts): returning them would skip field stripping, and a client
//! with a deeper parser would still read the fields. The original status
//! is kept. `Content-Length` is dropped so the empty body is not framed
//! with the withheld payload's length.
//!
//! ## Test
//!
//! See `tests/masking_module.rs` for end-to-end coverage.

use crate::interceptor::Interceptor;
use crate::policies::{Ability, Action, Subject};
use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::response::Parts;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};

/// Hard cap for the response body we'll read into memory before masking.
/// A larger body is replaced with an empty one so it cannot skip field
/// stripping.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Tunables for [`PolicyMaskingInterceptor`].
#[derive(Clone, Debug)]
pub struct MaskingConfig {
    /// Maximum body size (in bytes) buffered for masking. A larger body is
    /// replaced with an empty one so it cannot skip field stripping.
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
    // Only mask JSON responses. The media type is compared case-insensitively
    // so `Application/JSON` cannot skip field stripping. Parameters such as
    // `charset` are ignored. Suffix types (`application/problem+json`) are
    // not JSON responses of this interceptor.
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(is_json_content_type);
    if !is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, cfg.max_body_bytes).await {
        Ok(b) => b,
        Err(_) => {
            // `to_bytes` errors when the body is larger than the limit.
            // Returning that prefix, or the original body, would skip
            // field stripping. Empty keeps the status and withholds the
            // payload.
            tracing::warn!(
                target: "nestrs::masking",
                "response body exceeds cap; withholding body instead of skipping masking"
            );
            return withheld(parts);
        }
    };
    // A body that fills the cap was fully read (`to_bytes` only returns
    // `Ok` at or under the limit). Mask it. The previous `>= cap` branch
    // sent that exact-sized body through with every field intact.

    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            // Trailing bytes and nesting past serde_json's depth limit
            // fail here. The original bytes are still valid for a parser
            // with a higher limit, so returning them skips field stripping.
            tracing::warn!(
                target: "nestrs::masking",
                "response body is not JSON this parser accepts; withholding body instead of skipping masking"
            );
            return withheld(parts);
        }
    };
    mask_value(&mut value, ability);
    let new_bytes = match serde_json::to_vec(&value) {
        Ok(b) => b,
        Err(_) => return withheld(parts),
    };
    let mut new_parts = parts;
    new_parts.headers.remove(header::CONTENT_LENGTH);
    new_parts.headers.remove(header::TRANSFER_ENCODING);
    new_parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Response::from_parts(new_parts, Body::from(new_bytes))
}

/// `application/json`, ignoring parameters and ASCII case.
fn is_json_content_type(value: &HeaderValue) -> bool {
    let Ok(raw) = value.to_str() else {
        return false;
    };
    let media_type = raw.split(';').next().unwrap_or(raw).trim();
    media_type.eq_ignore_ascii_case("application/json")
}

/// Replace the body with an empty one and drop framing headers that still
/// describe the withheld payload. A stale `Content-Length` would make a
/// keep-alive peer read the next response as the rest of this body.
fn withheld(mut parts: Parts) -> Response {
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::TRANSFER_ENCODING);
    Response::from_parts(parts, Body::empty())
}

/// Recursive masker. Top-level value: detect `type`; recurse on objects and
/// arrays. For each detected subject, drop fields the principal cannot read.
///
/// The object is judged as a [`Subject::Instance`], not a bare type. A
/// grant with conditions or a row predicate matches only when that object
/// satisfies them. A matching `read` grant with no field list leaves the
/// object intact. A matching grant with a field list keeps only those
/// fields. No matching `read` grant keeps `type` and drops every other
/// scalar field. Arrays stay only when they contain a subject the walk can
/// still mask; scalar elements, scalar arrays, and scalar fields of an
/// untyped object inside those arrays are removed. Leaving them in place
/// returned emails, tokens, and untyped blobs from a subject the principal
/// cannot read. `allowed_fields` returns `None` in both the "no rule" and
/// "no field list" cases, and treating a type-level match as "show
/// everything" returned instances the conditions rejected.
///
/// `pub` so transport crates (WS, GraphQL, MCP) can re-use the same walker
/// on `serde_json::Value` payloads they produce directly, without going
/// through the HTTP [`mask_response`] path.
pub fn mask_value(value: &mut Value, ability: &Ability) {
    mask_value_in(value, ability, false);
}

/// `sealed` is set when this value was kept only so a nested subject can be
/// judged. The denied ancestor's own scalars must not survive that walk.
fn mask_value_in(value: &mut Value, ability: &Ability, sealed: bool) {
    match value {
        Value::Object(map) => mask_object(map, ability, sealed),
        Value::Array(items) => {
            if sealed {
                retain_subject_containers(items);
            }
            for item in items {
                mask_value_in(item, ability, sealed);
            }
        }
        _ => {}
    }
}

fn mask_object(map: &mut Map<String, Value>, ability: &Ability, sealed: bool) {
    // Own the name before mutating `map`. The response's type field is
    // untrusted and potentially unique per object.
    let type_name = map
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let child_sealed = if let Some(type_name) = type_name {
        let subject = read_subject(&type_name, map);
        if !ability.can(&Action::Read, &subject) {
            // Scalars are this instance's own fields. Arrays stay only when
            // they hold a subject: a `PostList` the caller cannot read still
            // contains `Post` objects that have their own grants. A scalar
            // array (`emails`, `recovery_codes`) is the denied object's data.
            seal_unreadable(map);
            true
        } else if let Some(allowed) = ability.allowed_fields(&Action::Read, &subject) {
            let allowed: std::collections::HashSet<&str> =
                allowed.iter().map(String::as_str).collect();
            map.retain(|k, _| allowed.contains(k.as_str()));
            // This object is readable. Nested values are judged on their own
            // markers, including scalar arrays the grant named.
            false
        } else {
            false
        }
    } else if sealed {
        // No type marker. This object rode along inside a denied subject's
        // array, so its scalars are that subject's data.
        seal_untyped(map);
        true
    } else {
        false
    };
    for child in map.values_mut() {
        mask_value_in(child, ability, child_sealed);
    }
}

/// Denied typed object: keep `type` and arrays that lead to a subject.
fn seal_unreadable(map: &mut Map<String, Value>) {
    map.retain(|k, v| k == "type" || array_holds_subject(v));
    scrub_sealed_arrays(map);
}

/// Untyped object reached through a denied subject. Keep nested containers
/// so a typed descendant can still be judged, and drop scalars.
fn seal_untyped(map: &mut Map<String, Value>) {
    map.retain(|_, v| v.is_object() || array_holds_subject(v));
    scrub_sealed_arrays(map);
}

fn scrub_sealed_arrays(map: &mut Map<String, Value>) {
    for value in map.values_mut() {
        if let Value::Array(items) = value {
            retain_subject_containers(items);
        }
    }
}

fn array_holds_subject(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(is_subject_container),
        _ => false,
    }
}

/// An object, or an array that eventually holds an object. Scalars and
/// arrays of only scalars cannot be field-masked.
fn is_subject_container(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::Array(items) => items.iter().any(is_subject_container),
        _ => false,
    }
}

fn retain_subject_containers(items: &mut Vec<Value>) {
    items.retain(is_subject_container);
}

/// Instance view of a response object for `can` / `allowed_fields`.
///
/// Conditions and predicates read `attributes` (see [`crate::policies`]).
/// The object's own fields are copied there, and a nested `attributes`
/// value is dropped first so it cannot hide those fields or satisfy a
/// condition on its own. The outer `type` is the object's marker, which
/// selects the grant; the stored `type` field stays in the columns so a
/// predicate can still see it.
fn read_subject(type_name: &str, map: &Map<String, Value>) -> Subject {
    let mut columns = map.clone();
    columns.remove("attributes");
    Subject::Instance(serde_json::json!({
        "type": type_name,
        "attributes": columns,
    }))
}

/// Convenience: a 200 response with a JSON body. Used by tests and the
/// "pass-through" docs to construct sample responses.
pub fn json_response<T: serde::Serialize>(status: StatusCode, body: T) -> Response {
    (status, axum::Json(body)).into_response()
}
