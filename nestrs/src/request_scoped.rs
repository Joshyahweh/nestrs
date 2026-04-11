//! Request-scoped provider extraction (NestJS request scope analogue).
//!
//! Enable with [`crate::NestApplication::use_request_scope`]. Handlers can then access request-scoped
//! providers via [`RequestScoped<T>`].

use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

/// Extracts an `Arc<T>` from the request-scoped DI cache.
///
/// This is intended for providers registered with `#[injectable(scope = "request")]`, but it also works
/// for singleton/transient providers (it simply resolves them through the same provider registry).
pub struct RequestScoped<T>(pub Arc<T>);

/// Returned when request scope is not enabled on the app.
#[derive(Debug)]
pub struct RequestScopedMissing;

impl IntoResponse for RequestScopedMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: RequestScoped extractor requires NestApplication::use_request_scope()",
        )
            .into_response()
    }
}

#[async_trait::async_trait]
impl<S, T> axum::extract::FromRequestParts<S> for RequestScoped<T>
where
    S: Send + Sync,
    T: Send + Sync + 'static,
{
    type Rejection = RequestScopedMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let registry = parts
            .extensions
            .get::<Arc<crate::core::ProviderRegistry>>()
            .cloned()
            .ok_or(RequestScopedMissing)?;
        Ok(Self(registry.get::<T>()))
    }
}

