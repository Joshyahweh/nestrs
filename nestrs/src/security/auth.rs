//! Authentication helpers and reusable guards (Nest **Passport**-style strategies stay in your app; these wire into [`CanActivate`](crate::core::CanActivate) and Axum extractors).

use crate::core::{AuthStrategy, CanActivate, GuardError, HandlerKey, MetadataRegistry};
use crate::UnauthorizedException;
use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use std::marker::PhantomData;

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

/// Comma-separated `roles` metadata for the current route (from [`MetadataRegistry`]), if any.
pub fn route_roles_csv(parts: &Parts) -> Option<String> {
    let handler = parts.extensions.get::<HandlerKey>().map(|h| h.0)?;
    MetadataRegistry::get(handler, "roles")
}

/// Requires a non-empty `Authorization: Bearer …` header and exposes the token (UTF-8).
#[derive(Debug, Clone)]
pub struct BearerToken(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for BearerToken
where
    S: Send + Sync,
{
    type Rejection = crate::HttpException;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| UnauthorizedException::new("missing Authorization header"))?;
        let token = parse_authorization_bearer(raw)
            .ok_or_else(|| UnauthorizedException::new("expected Bearer token"))?;
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

/// Reads the caller role from the `x-role` header and checks it against `#[roles("a,b")]` metadata (same pattern as Nest metadata + guard).
#[derive(Debug, Default)]
pub struct XRoleMetadataGuard;

#[async_trait]
impl CanActivate for XRoleMetadataGuard {
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
