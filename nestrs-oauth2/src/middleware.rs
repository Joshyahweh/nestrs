//! OAuth2 middleware. The Axum middleware that runs *before*
//! `OAuth2Guard` and verifies the bearer token. On success, it
//! stashes an `OAuth2Identity` into `parts.extensions` so the guard
//! (and any downstream extractor) can read it.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;

use crate::resource_server::JwtVerifier;

/// The verified identity stashed into `parts.extensions` by
/// `install_oauth2_middleware`. The `Principal` / `OptionalPrincipal`
/// extractors in the main `nestrs` crate read this when the
/// `authn-bridge` feature is on.
#[derive(Debug, Clone)]
pub struct OAuth2Identity {
    pub subject: String,
    pub claims: serde_json::Value,
}

/// Axum middleware factory. Returns a closure that takes the
/// configured `JwtVerifier` (as state) and the request, and runs
/// the next handler with the verified identity in extensions.
pub async fn install_oauth2_middleware(
    State(verifier): State<Arc<JwtVerifier>>,
    mut req: Request,
    next: Next,
) -> Response {
    // Pull the bearer token from the Authorization header. We don't
    // import `parse_authorization_bearer` from `nestrs::security`
    // (cycle prevention — `nestrs-oauth2` depends only on
    // `nestrs-core`). The logic is small enough to inline.
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| {
            // Case-insensitive Bearer scheme parser.
            let lower = s.to_ascii_lowercase();
            if lower.starts_with("bearer ") {
                Some(s[7..].trim().to_string())
            } else {
                None
            }
        });
    if let Some(t) = token {
        match verifier.verify(&t).await {
            Ok(data) => {
                let subject = data
                    .claims
                    .get("sub")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let identity = OAuth2Identity {
                    subject,
                    claims: data.claims,
                };
                req.extensions_mut().insert(identity);
            }
            Err(e) => {
                // Don't fail the request — the guard / extractor
                // decides whether the route is protected. We just
                // log the verification failure and proceed without
                // an identity. This matches `install_authn_middleware`'s
                // behaviour (see `nestrs/src/authn.rs:405-422`).
                tracing::debug!(?e, "OAuth2 token verification failed");
            }
        }
    }
    next.run(req).await
}
