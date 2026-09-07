//! OAuth2 ↔ main-crate bridge. Re-exports the OAuth2 middleware and
//! identity, and provides an `OAuth2Principal` extractor that reads
//! the verified identity out of request extensions (stashed there by
//! `install_oauth2_middleware`).
//!
//! Mirrors the `Principal` / `OptionalPrincipal` pattern from the
//! `authn` feature: routes that need the OAuth2 principal use
//! `OAuth2Principal`; routes that want 200 with a possibly-anonymous
//! identity use `Option<OAuth2Principal>` (or skip the extractor).

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub use nestrs_oauth2::middleware::OAuth2Identity;
pub use nestrs_oauth2::resource_server::JwtVerifier;

/// Re-export of the OAuth2 middleware factory from `nestrs-oauth2`.
/// `NestApplication::use_oauth2(...)` calls this through
/// `from_fn_with_state` after wiring a `JwtVerifier` into the
/// application state.
pub use nestrs_oauth2::middleware::install_oauth2_middleware as _install_oauth2_middleware_inner;

/// Verified OAuth2 principal. Routes that require an authenticated
/// user extract `OAuth2Principal`; missing identity → 401. Use
/// `Option<OAuth2Principal>` for routes that accept anonymous
/// callers.
#[derive(Debug, Clone)]
pub struct OAuth2Principal {
    pub subject: String,
    pub claims: serde_json::Value,
}

/// 401 response for `OAuth2Principal` extraction failure.
#[derive(Debug, Clone, Copy)]
pub struct OAuth2PrincipalMissing;

impl IntoResponse for OAuth2PrincipalMissing {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, "OAuth2 principal required").into_response()
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for OAuth2Principal
where
    S: Send + Sync,
{
    type Rejection = OAuth2PrincipalMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<OAuth2Identity>()
            .cloned()
            .map(|id| OAuth2Principal {
                subject: id.subject,
                claims: id.claims,
            })
            .ok_or(OAuth2PrincipalMissing)
    }
}

/// Bridge wrapper so the `from_fn_with_state` style works with the
/// `nestrs-oauth2` middleware (which takes `State<Arc<JwtVerifier>>`).
pub async fn install_oauth2_middleware(
    axum::extract::State(verifier): axum::extract::State<Arc<JwtVerifier>>,
    req: Request,
    next: Next,
) -> Response {
    _install_oauth2_middleware_inner(axum::extract::State(verifier), req, next).await
}
