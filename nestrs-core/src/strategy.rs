//! Authentication strategy primitives (NestJS Passport-style analogue).

use axum::http::request::Parts;

/// Error produced by [`AuthStrategy::validate`].
#[derive(Debug, Clone)]
pub struct AuthError {
    pub message: String,
}

impl AuthError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Auth strategy contract similar to Nest's Passport strategy adapters.
///
/// A guard can delegate token/header parsing + claim validation to a strategy and map failures
/// to `GuardError::Unauthorized`.
#[async_trait::async_trait]
pub trait AuthStrategy: Send + Sync + 'static {
    type Payload: Send + Sync + 'static;

    async fn validate(&self, parts: &Parts) -> Result<Self::Payload, AuthError>;
}
