//! Runtime support for the `#[nestrs::crud]` proc-macro.
//!
//! The macro emits a `{Pascal}ListQuery` DTO + a hidden `FromRequestParts`
//! extractor that parses it from the request URI's query string using
//! `serde_qs` (which understands `?filter[field]=x&sort=field:DIR` bracket
//! syntax). This module is the *adapter* — the macro references
//! [`__CrudQueryAdapter`] by path when it rewrites the generated handler's
//! `#[param::query]` parameter type.
//!
//! # Why a custom adapter (not just `ValidatedQuery`)?
//!
//! [`ValidatedQuery`](crate::ValidatedQuery) wraps `axum::extract::Query<T>`,
//! which uses `serde_urlencoded` — that deserializer only handles flat
//! `?key=value&key2=value2` pairs and silently drops bracketed keys. The
//! `#[crud]` list endpoint contract follows `@nestjsx/crud`'s convention
//! (`?filter[author]=alice&filter[status]=published&sort=created_at:DESC`),
//! which requires nested-object deserialization. `serde_qs` is the
//! standard crate for that.
//!
//! # Error contract
//!
//! - `serde_qs::Error` → `BadRequestException` (400) with the parse error
//!   message in the body — same shape as `ValidatedQuery`.
//! - `validator::ValidationErrors` → `__nestrs_validation_failed` (422
//!   with field-level detail) — same as `ValidatedQuery` / `ValidatedBody`.
//!
//! Gated on the `database-sqlx` feature because that's where the macro's
//! output DTOs live (the bracketed-filter convention is a SQL-style
//! convention, and the macro itself is the only consumer).

use crate::{BadRequestException, HttpException, UnprocessableEntityException};
use ::axum::http::request::Parts;
use ::axum::http::Uri;
use validator::Validate;

// ---------------------------------------------------------------------------
// Defaults + regex used by the `#[nestrs::crud]` macro's generated
// `ListQuery` DTO. Hoisted to the runtime so the macro can reference
// them by path without each expansion re-declaring them.
// ---------------------------------------------------------------------------

/// `page=1` default (1-indexed, matching `@nestjsx/crud`).
pub fn default_page() -> u32 {
    1
}

/// `per_page=20` default.
pub fn default_per_page() -> u32 {
    20
}

/// Validation regex source for `?sort=field:DIR,field:DIR`. ASCII field
/// names only (alphanumeric + underscore) — anything else fails validation
/// and surfaces as 422.
pub const SORT_REGEX: &str =
    r"^[a-zA-Z_][a-zA-Z0-9_]*(:(ASC|DESC))?(,[a-zA-Z_][a-zA-Z0-9_]*(:(ASC|DESC))?)*$";

/// Custom validator function for the `?sort` query field. Wired to the
/// macro's generated `ListQuery` DTO via
/// `#[validate(custom(function = "validate_sort_string"))]`. Returns
/// `Ok(())` for valid sort strings, `Err(ValidationError)` for malformed
/// ones. Equivalent to a regex check but avoids pulling the `regex`
/// crate as a direct dep — validator already brings it in transitively
/// for `AsRegex`, but we keep our public surface lean.
pub fn validate_sort_string(s: &str) -> ::std::result::Result<(), ::validator::ValidationError> {
    use ::validator::ValidationError;
    // Hand-rolled scan: accept `field(:DIR)?(,field(:DIR)?)*` with
    // ASCII alphanumeric+underscore field names. We could compile a
    // regex here, but the loop is short and saves a transitive type
    // reaching our public surface.
    if s.is_empty() {
        let mut e = ValidationError::new("sort_empty");
        e.message = Some(std::borrow::Cow::Borrowed("sort must not be empty"));
        return Err(e);
    }
    for token in s.split(',') {
        let mut parts = token.split(':');
        let key = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            let mut e = ValidationError::new("sort_field_missing");
            e.message = Some(std::borrow::Cow::Owned(format!(
                "sort token `{token}` is missing a field"
            )));
            return Err(e);
        }
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            let mut e = ValidationError::new("sort_field_invalid");
            e.message = Some(std::borrow::Cow::Owned(format!(
                "sort field `{key}` must be ASCII alphanumeric + underscore"
            )));
            return Err(e);
        }
        if let Some(dir) = parts.next() {
            let dir = dir.trim();
            if !dir.is_empty()
                && !dir.eq_ignore_ascii_case("ASC")
                && !dir.eq_ignore_ascii_case("DESC")
            {
                let mut e = ValidationError::new("sort_dir_invalid");
                e.message = Some(std::borrow::Cow::Owned(format!(
                    "sort direction `{dir}` must be ASC or DESC (or omitted)"
                )));
                return Err(e);
            }
        }
        if parts.next().is_some() {
            let mut e = ValidationError::new("sort_too_many_colons");
            e.message = Some(std::borrow::Cow::Owned(format!(
                "sort token `{token}` has too many `:` segments"
            )));
            return Err(e);
        }
    }
    Ok(())
}

/// Error type returned by `{Service}` methods. The macro's handlers
/// translate this into HTTP responses via [`map_crud_error_to_http`].
#[derive(Debug)]
pub enum CrudError {
    /// An `sqlx` error bubbled up from the underlying repository.
    Sqlx(::sqlx::Error),
    /// A JSON round-trip error (entity → DTO or DTO → entity).
    Json(::serde_json::Error),
    /// Explicit "this row does not exist or is authz-denied" signal. Not
    /// currently emitted by `{Service}` directly (handlers use
    /// `Option::None`); kept for future `find_*_authorized` extensions.
    NotFound,
}

impl ::std::fmt::Display for CrudError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Sqlx(e) => ::std::fmt::Display::fmt(e, f),
            Self::Json(e) => ::std::fmt::Display::fmt(e, f),
            Self::NotFound => f.write_str("not found"),
        }
    }
}

impl ::std::error::Error for CrudError {}

impl From<::sqlx::Error> for CrudError {
    fn from(e: ::sqlx::Error) -> Self {
        Self::Sqlx(e)
    }
}

impl From<::serde_json::Error> for CrudError {
    fn from(e: ::serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Translate a `CrudError` into the `HttpException` the framework's
/// exception filter will serialize. SQL / JSON failures bubble up as
/// 500 (sanitized in production, detailed in dev — see
/// `NestApplication::production_errors`); `NotFound` would have already
/// been converted to a `None` upstream, so this branch is unreachable
/// in v1.
pub fn map_crud_error_to_http(e: CrudError) -> HttpException {
    match e {
        CrudError::NotFound => UnprocessableEntityException::new("not found"),
        other => crate::InternalServerErrorException::new(other.to_string()),
    }
}

/// JSON match helper for the generated `filter[...]=...` logic. The
/// filter side is `serde_json::Value` because `serde_qs` deserializes
/// it that way; this function does loose comparison so `?filter[done]=true`
/// and `?filter[count]=1` work as expected.
pub fn json_matches(got: &serde_json::Value, want: &serde_json::Value) -> bool {
    match (got, want) {
        (serde_json::Value::String(a), serde_json::Value::String(b)) => {
            a.to_lowercase().contains(&b.to_lowercase())
        }
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => a == b,
        (serde_json::Value::Bool(a), serde_json::Value::Bool(b)) => a == b,
        (serde_json::Value::Null, serde_json::Value::Null) => true,
        _ => got == want,
    }
}

/// Substring-match search across every string field of a JSON value.
/// Used by `?search=foo` — recursively walks the value and returns true
/// if any string contains the needle (case-insensitive).
pub fn json_contains_string(v: &serde_json::Value, needle: &str) -> bool {
    match v {
        serde_json::Value::String(s) => s.to_lowercase().contains(needle),
        serde_json::Value::Array(arr) => arr.iter().any(|x| json_contains_string(x, needle)),
        serde_json::Value::Object(map) => map.values().any(|x| json_contains_string(x, needle)),
        _ => false,
    }
}

/// Axum extractor that parses a query DTO with `serde_qs` so bracketed
/// keys (`?filter[author]=x&sort=field:DESC`) survive deserialization.
///
/// The user-facing DTO type is whatever the `#[crud]` macro generated
/// alongside the controller; the macro rewrites the handler parameter to
/// `__CrudQueryAdapter<{Pascal}ListQuery>` at expansion time. The wrapping
/// newtype keeps `FromRequestParts` object-safe per type and matches the
/// shape of [`crate::ValidatedQuery`].
pub struct __CrudQueryAdapter<T>(pub T);

#[::axum::async_trait]
impl<S, T> ::axum::extract::FromRequestParts<S> for __CrudQueryAdapter<T>
where
    S: Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = HttpException;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = raw_query_string(
            parts
                .uri
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or(""),
        );
        // Use non-strict mode to support bracket notation (filter[field]=value)
        let config = serde_qs::Config::new(5, false);
        let value: T = config
            .deserialize_str(&raw)
            .map_err(|e| BadRequestException::new(format!("Invalid crud query string: {e}")))?;
        value
            .validate()
            .map_err(validation_to_http_exception)?;
        Ok(Self(value))
    }
}

/// Strip the path component of a URI, leaving just the raw query string
/// (without the leading `?`). Returns `""` for URIs with no query.
///
/// Why a hand-rolled stripper instead of `Uri::query()`? `Uri::query()`
/// returns the URL-decoded form, which is wrong for `serde_qs` (it
/// expects the raw, percent-encoded form so it can split on `&` / `=`
/// before percent-decoding individual pairs). The path also matters
/// because `Uri::query()` is itself subject to re-encoding in some
/// middlewares; reading straight off `as_str()` keeps the contract
/// deterministic.
fn raw_query_string(uri: &str) -> String {
    match uri.find('?') {
        None => String::new(),
        Some(idx) => uri[idx + 1..].to_string(),
    }
}

/// Resolve the `q.s()` module path for the adapter — exposed so the
/// `#[crud]` macro can name `nestrs::crud_macro::__CrudQueryAdapter` in
/// its generated code without hard-coding the full path.
pub const ADAPTER_PATH: &str = "::nestrs::crud_macro::__CrudQueryAdapter";

/// URI helper re-exported so generated handlers and tests can introspect
/// the query string if needed. Currently unused outside the adapter; kept
/// for symmetry with other transport adapters (`ValidatedQuery::uri`,
/// etc.).
#[allow(dead_code)]
pub(crate) fn raw_query_of(uri: &Uri) -> Option<String> {
    uri.query().map(|q| q.to_string())
}

/// Compare two `serde_json::Value` for sorting. Provides a total ordering
/// that handles Null, Bool, Number, String, Array, Object consistently.
/// This is the comparator used by the generated `list_query` sort logic.
pub fn sort_value_cmp(a: &serde_json::Value, b: &serde_json::Value) -> ::std::cmp::Ordering {
    use ::std::cmp::Ordering;
    match (a, b) {
        (serde_json::Value::Null, serde_json::Value::Null) => Ordering::Equal,
        (serde_json::Value::Null, _) => Ordering::Less,
        (_, serde_json::Value::Null) => Ordering::Greater,
        (serde_json::Value::Bool(a), serde_json::Value::Bool(b)) => a.cmp(b),
        (serde_json::Value::Bool(_), _) => Ordering::Less,
        (_, serde_json::Value::Bool(_)) => Ordering::Greater,
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            // Compare as f64 for a total ordering (handles int/float mix)
            let af = a.as_f64().unwrap_or(f64::NEG_INFINITY);
            let bf = b.as_f64().unwrap_or(f64::NEG_INFINITY);
            af.partial_cmp(&bf).unwrap_or(Ordering::Equal)
        }
        (serde_json::Value::Number(_), _) => Ordering::Less,
        (_, serde_json::Value::Number(_)) => Ordering::Greater,
        (serde_json::Value::String(a), serde_json::Value::String(b)) => a.cmp(b),
        (serde_json::Value::String(_), _) => Ordering::Less,
        (_, serde_json::Value::String(_)) => Ordering::Greater,
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            // Lexicographic compare
            let mut ai = a.iter();
            let mut bi = b.iter();
            loop {
                match (ai.next(), bi.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, _) => return Ordering::Less,
                    (_, None) => return Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let ord = sort_value_cmp(x, y);
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }
        }
        (serde_json::Value::Array(_), _) => Ordering::Less,
        (_, serde_json::Value::Array(_)) => Ordering::Greater,
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            // Compare by sorted keys then values
            let mut a_keys: Vec<_> = a.keys().collect();
            let mut b_keys: Vec<_> = b.keys().collect();
            a_keys.sort();
            b_keys.sort();
            match a_keys.cmp(&b_keys) {
                Ordering::Equal => {
                    for k in a_keys {
                        let ord = sort_value_cmp(&a[k], &b[k]);
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    Ordering::Equal
                }
                other => other,
            }
        }
    }
}

/// Translate `validator::ValidationErrors` into the same
/// `UnprocessableEntityException` shape the framework's `ValidationPipe`
/// uses (see `nestrs::pipes::ValidationPipe::transform`). Duplicated here
/// rather than re-exported from `lib.rs` because the helper is private to
/// the root module; keeping the field-detail JSON in sync is the goal.
fn validation_to_http_exception(errors: validator::ValidationErrors) -> HttpException {
    let mut details: Vec<serde_json::Value> = Vec::new();
    for (field, field_errors) in errors.field_errors() {
        for ve in field_errors {
            let message = ve
                .message
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or_else(|| ve.code.to_string());
            details.push(serde_json::json!({
                "property": field,
                "constraints": { "code": ve.code.to_string(), "message": message },
            }));
        }
    }
    UnprocessableEntityException::new("Validation failed").with_details(serde_json::json!(details))
}
