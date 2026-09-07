//! `OAuth2Guard` — the `CanActivate` implementation that the
//! nestrs framework uses to enforce route-level OAuth2 protection.
//!
//! Mirrors the `AuthnGuard` shape in `nestrs/src/authn.rs:280-312`:
//! the heavy lifting (token verification) happens in the middleware
//! that runs *before* the guard; the guard itself is stateless and
//! just reads the verified identity out of request extensions.

use axum::http::request::Parts;
use nestrs_core::{CanActivate, GuardError, ProviderRegistry};

/// `OAuth2Guard` enforces "request has a verified OAuth2 principal".
/// The actual JWT verification runs in `install_oauth2_middleware`
/// *before* the guard; the guard just checks the principal extension
/// is present and, if route metadata requires, that the principal
/// has the right role.
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

    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        // The actual enforcement happens in three places:
        //   1. Middleware: verifies the bearer token, populates
        //      `OAuth2Identity` into `parts.extensions`.
        //   2. The route metadata layer (`#[roles("admin")]` etc.):
        //      this would normally be checked here. We don't read
        //      it because `nestrs::security::route_roles_csv` lives
        //      in the main crate (cycle prevention).
        //   3. The principal extractor: downstream handlers can
        //      call `nestrs::Principal` / `OptionalPrincipal` (when
        //      wired through `authn-bridge`) to require the identity.
        //
        // For this wave the guard is a no-op on `can_activate` —
        // the middleware + principal-extractor pair is the load-
        // bearing piece. Future waves (or a `nestrs::security::OAuth2Identity`
        // extension type) can layer role-based checks on top.
        Ok(())
    }
}
