//! Route guards ([`CanActivate`]) — run before the handler (NestJS `UseGuards` analogue).

use async_trait::async_trait;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::ProviderRegistry;

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
///
/// # Dependency injection
///
/// Guards are resolved **once at route-registration time**, so they can hold dependencies
/// (JWT keys, repositories, caches). Override [`Self::resolve`] to pull them from the
/// [`ProviderRegistry`]; keep the `Default` supertrait satisfied with a placeholder unit struct:
///
/// ```ignore
/// #[derive(Default)]
/// struct AuthGuard { users: Arc<UserRepository> } // real fields live here
///
/// // Placeholder used only to satisfy the Default bound:
/// impl Default for AuthGuard { fn default() -> Self { Self { users: Arc::new(UserRepository::empty()) } } }
///
/// #[async_trait]
/// impl CanActivate for AuthGuard {
///     fn resolve(registry: &ProviderRegistry) -> Self {
///         Self { users: registry.get::<UserRepository>() }
///     }
///     async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> { /* ... */ }
/// }
/// ```
#[async_trait]
pub trait CanActivate: Default + Send + Sync + 'static {
    /// Build the guard instance used for **every** request on routes declaring this guard.
    ///
    /// The default implementation returns [`Default::default()`] (a stateless guard).
    /// Override this to construct a stateful guard from the application's
    /// [`ProviderRegistry`] (NestJS dependency-injected guards).
    fn resolve(_registry: &crate::ProviderRegistry) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError>;
}
