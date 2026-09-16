//! Authentication helpers and reusable guards (Nest **Passport**-style strategies stay in your app; these wire into [`CanActivate`] and Axum extractors).
//!
//! [`CanActivate`]: nestrs_core::CanActivate

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nestrs_core::{AuthStrategy, CanActivate, GuardError, HandlerKey, MetadataRegistry};
use serde_json::json;
use std::marker::PhantomData;

// ---------------------------------------------------------------------------
// SecurityRejection: the rejection type for [`BearerToken`] (and the only
// rejection type this crate produces). The umbrella `nestrs::HttpException`
// family lives behind `nestrs::authn` and would force a cycle; this local
// type produces the same wire shape (401 + JSON body) without the dep.
// ---------------------------------------------------------------------------

/// Rejection produced by [`BearerToken`] when the `Authorization` header is
/// missing or not a valid `Bearer` token.
#[derive(Debug)]
pub enum SecurityRejection {
    /// 401 — the header was missing, malformed, or carried the wrong scheme.
    Unauthorized(String),
}

impl SecurityRejection {
    /// Build from a message (used by [`BearerToken`]'s extractor).
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
}

impl IntoResponse for SecurityRejection {
    fn into_response(self) -> Response {
        let (status, message, error) = match self {
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m, "Unauthorized"),
        };
        (
            status,
            axum::Json(json!({
                "statusCode": status.as_u16(),
                "message": message,
                "error": error,
            })),
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Bearer scheme parsing
// ---------------------------------------------------------------------------

/// Parses `Authorization: Bearer <token>` (case-insensitive scheme). Returns the token slice without allocating.
pub fn parse_authorization_bearer(auth_header: &str) -> Option<&str> {
    let mut iter = auth_header.splitn(2, char::is_whitespace);
    let scheme = iter.next()?.trim_end_matches(':');
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = iter.next()?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

// ---------------------------------------------------------------------------
// Route metadata helpers
// ---------------------------------------------------------------------------

/// Comma-separated `roles` metadata for the current route (from [`MetadataRegistry`]), if any.
pub fn route_roles_csv(parts: &Parts) -> Option<String> {
    let handler = parts.extensions.get::<HandlerKey>().map(|h| h.0)?;
    MetadataRegistry::get(handler, "roles")
}

/// Generic route metadata CSV (e.g. for `check_policies`). Returns the metadata value
/// stored under `key` on the current handler, if any.
// Only the `policies` module (feature `authz`) calls this; gate it so it does
// not exist as dead code in builds without `authz`.
#[cfg(feature = "authz")]
pub fn route_metadata_csv(parts: &Parts, key: &str) -> Option<String> {
    let handler = parts.extensions.get::<HandlerKey>().map(|h| h.0)?;
    MetadataRegistry::get(handler, key)
}

// ---------------------------------------------------------------------------
// BearerToken / OptionalBearerToken extractors
// ---------------------------------------------------------------------------

/// Requires a non-empty `Authorization: Bearer …` header and exposes the token (UTF-8).
#[derive(Debug, Clone)]
pub struct BearerToken(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for BearerToken
where
    S: Send + Sync,
{
    type Rejection = SecurityRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| SecurityRejection::unauthorized("missing Authorization header"))?;
        let token = parse_authorization_bearer(raw)
            .ok_or_else(|| SecurityRejection::unauthorized("expected Bearer token"))?;
        Ok(BearerToken(token.to_string()))
    }
}

/// Same as [`BearerToken`] but yields `None` when the header is missing or not a Bearer token.
#[derive(Debug, Clone, Default)]
pub struct OptionalBearerToken(pub Option<String>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalBearerToken
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let v = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(parse_authorization_bearer)
            .map(str::to_string);
        Ok(OptionalBearerToken(v))
    }
}

// ---------------------------------------------------------------------------
// AuthStrategyGuard
// ---------------------------------------------------------------------------

/// Runs [`AuthStrategy::validate`] for `S: Default` (JWT/API-key strategies you implement and mark `Default` when stateless).
#[derive(Debug)]
pub struct AuthStrategyGuard<S>(PhantomData<S>);

impl<S> Default for AuthStrategyGuard<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[async_trait]
impl<S> CanActivate for AuthStrategyGuard<S>
where
    S: AuthStrategy + Default + Send + Sync + 'static,
{
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        S::default()
            .validate(parts)
            .await
            .map_err(|e| GuardError::unauthorized(e.message))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DemoXRoleMetadataGuard
// ---------------------------------------------------------------------------

/// **Demo-only** guard showing the Nest metadata + roles pattern: it reads the caller's role
/// from the **client-supplied** `x-role` header and checks it against `#[roles("a,b")]`
/// route metadata.
///
/// ⚠️ **Never use this in production.** The role comes from a request header anyone can set.
/// Real deployments must derive the role from authenticated material (JWT claims, session,
/// API-key lookup) inside your own [`CanActivate`] implementation or an
/// [`AuthStrategy`]. This type exists to demonstrate
/// `set_metadata`/`roles` wiring end-to-end in tests and examples.
#[derive(Debug, Default)]
pub struct DemoXRoleMetadataGuard;

#[async_trait]
impl CanActivate for DemoXRoleMetadataGuard {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        let handler = parts
            .extensions
            .get::<HandlerKey>()
            .map(|h| h.0)
            .ok_or_else(|| GuardError::forbidden("missing handler key"))?;

        let allowed = MetadataRegistry::get(handler, "roles")
            .ok_or_else(|| GuardError::forbidden("missing roles metadata"))?;

        let role = parts
            .headers
            .get("x-role")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let is_allowed = allowed.split(',').any(|r| r.trim() == role);
        if is_allowed {
            Ok(())
        } else {
            Err(GuardError::forbidden("forbidden"))
        }
    }
}

/// Old name of [`DemoXRoleMetadataGuard`]. Kept as a deprecated alias so existing code keeps
/// compiling; the rename makes the client-trusted-header footgun explicit.
#[allow(dead_code)] // re-exported for downstream users even when unused within this crate
#[deprecated(
    since = "0.3.9",
    note = "renamed to DemoXRoleMetadataGuard: this guard trusts the client-supplied `x-role` header and must not be used in production"
)]
pub type XRoleMetadataGuard = DemoXRoleMetadataGuard;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bearer_accepts_canonical_scheme() {
        assert_eq!(parse_authorization_bearer("Bearer abc.def.ghi"), Some("abc.def.ghi"));
        assert_eq!(parse_authorization_bearer("bearer xyz"), Some("xyz"));
        assert_eq!(parse_authorization_bearer("BEARER xyz"), Some("xyz"));
    }

    #[test]
    fn parse_bearer_rejects_other_schemes() {
        assert_eq!(parse_authorization_bearer("Basic dXNlcjpwYXNz"), None);
        assert_eq!(parse_authorization_bearer("Token xyz"), None);
        assert_eq!(parse_authorization_bearer(""), None);
        // Empty token after scheme
        assert_eq!(parse_authorization_bearer("Bearer "), None);
        assert_eq!(parse_authorization_bearer("Bearer"), None);
    }
}
