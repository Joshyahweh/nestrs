//! `OAuth2Guard` — the `CanActivate` implementation that the
//! nestrs framework uses to enforce route-level OAuth2 protection.
//!
//! Mirrors the `AuthnGuard` shape in `nestrs/src/authn.rs:280-312`:
//! the heavy lifting (token verification) happens in the middleware
//! that runs *before* the guard; the guard itself is stateless and
//! just reads the verified identity out of request extensions.

use axum::http::request::Parts;
use nestrs_core::{CanActivate, GuardError, ProviderRegistry};

use crate::middleware::OAuth2Identity;

/// `OAuth2Guard` enforces "request has a verified OAuth2 principal".
/// The actual JWT verification runs in `install_oauth2_middleware`
/// *before* the guard; the guard is the load-bearing gate — it rejects
/// any request whose extensions lack an `OAuth2Identity` with
/// `GuardError::Unauthorized("OAuth2 identity required")`, which the
/// framework maps to **HTTP 401**.
///
/// Derives `Default` because `CanActivate` requires it. The
/// `resolve(registry)` hook lets a future version pull the verifier
/// directly out of DI; the current implementation delegates to the
/// middleware for verification and treats the guard as a stateless
/// "is the principal present" check.
#[derive(Debug, Default, Clone, Copy)]
pub struct OAuth2Guard;

#[async_trait::async_trait]
impl CanActivate for OAuth2Guard {
    fn resolve(_registry: &ProviderRegistry) -> Self {
        // The verifier is in the middleware, not pulled by the guard.
        // Default to a stateless guard; the middleware sets the
        // extension that the guard reads.
        Self
    }

    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        match parts.extensions.get::<OAuth2Identity>() {
            Some(_) => Ok(()),
            None => Err(GuardError::unauthorized("OAuth2 identity required")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::OAuth2Identity;
    use axum::http::{Request, Version};

    fn parts_without_identity() -> Parts {
        let req = Request::builder()
            .method("GET")
            .uri("/")
            .version(Version::HTTP_11)
            .body(())
            .expect("static request");
        req.into_parts().0
    }

    fn parts_with_identity() -> Parts {
        let mut req = Request::builder()
            .method("GET")
            .uri("/")
            .version(Version::HTTP_11)
            .body(())
            .expect("static request");
        req.extensions_mut().insert(OAuth2Identity {
            subject: "u-1".into(),
            claims: serde_json::json!({ "sub": "u-1" }),
        });
        req.into_parts().0
    }

    #[tokio::test]
    async fn oauth2_guard_rejects_missing_identity() {
        let guard = OAuth2Guard;
        let parts = parts_without_identity();
        let err = guard.can_activate(&parts).await.expect_err("must reject");
        match err {
            GuardError::Unauthorized(m) => assert_eq!(m, "OAuth2 identity required"),
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn oauth2_guard_accepts_present_identity() {
        let guard = OAuth2Guard;
        let parts = parts_with_identity();
        assert!(guard.can_activate(&parts).await.is_ok());
    }

    #[test]
    fn guard_error_unauthorized_maps_to_401() {
        // `IntoResponse` for `GuardError` is implemented in `nestrs-core`;
        // we need the axum trait in scope to call `.into_response()`.
        use axum::response::IntoResponse;
        let resp = GuardError::unauthorized("OAuth2 identity required").into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
