//! Route guards ([`CanActivate`]) — run before the handler (NestJS `UseGuards` analogue).

use async_trait::async_trait;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Failure returned from [`CanActivate::can_activate`]; becomes a JSON error body (401 / 403).
#[derive(Debug, Clone)]
pub enum GuardError {
    Unauthorized(String),
    Forbidden(String),
}

impl GuardError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }
}

impl IntoResponse for GuardError {
    fn into_response(self) -> Response {
        let (status, message, error_label) = match &self {
            GuardError::Unauthorized(m) => (
                axum::http::StatusCode::UNAUTHORIZED,
                m.clone(),
                "Unauthorized",
            ),
            GuardError::Forbidden(m) => (axum::http::StatusCode::FORBIDDEN, m.clone(), "Forbidden"),
        };
        let body = axum::Json(json!({
            "statusCode": status.as_u16(),
            "message": message,
            "error": error_label,
        }));
        (status, body).into_response()
    }
}

/// Authorize the request before the handler runs. Declare per-route guard types in the `impl_routes!`
/// macro: `GET "/x" with (A, B) => MyController::handler,` — use `with ()` when there are no route guards.
/// For a guard on **all** routes of a controller, use `controller_guards (G)` on `impl_routes!` (see the
/// `nestrs` crate); that runs **outside** route-level guards.
///
/// Stateless guards are usually unit structs with [`Default`].
#[async_trait]
pub trait CanActivate: Default + Send + Sync + 'static {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError>;
}
