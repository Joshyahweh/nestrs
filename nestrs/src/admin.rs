//! `nestrs::admin` — a localhost-only HTTP sidecar port that exposes the
//! live provider / route / metadata registries to tooling (notably
//! `nestrs-mcp`'s live runtime tools).
//!
//! Enable with the `admin` Cargo feature:
//!
//! ```toml
//! nestrs = { version = "1.2.0", features = ["admin"] }
//! ```
//!
//! Then on the application:
//!
//! ```ignore
//! use nestrs::admin::{AdminOptions, AdminHandle};
//!
//! let handle: AdminHandle = app.use_admin(AdminOptions {
//!     addr: "127.0.0.1:7777".parse()?,
//!     token: Some("secret".into()),
//! });
//! tokio::spawn(async move { handle.serve().await });
//! ```
//!
//! Endpoints (all under `/__nestrs/`):
//!
//! - `GET /__nestrs/health`     — liveness, uptime, version
//! - `GET /__nestrs/providers`  — `Vec<{ type_name, scope }>`
//! - `GET /__nestrs/routes`     — `Vec<RouteInfo>` (method, path, handler, openapi summary)
//! - `GET /__nestrs/openapi.json` — placeholder summary (real OpenAPI doc comes from the `openapi` feature)
//!
//! Live dump from the CLI (curl GET, bearer header, `--` before the URL):
//!
//! ```bash
//! nestrs-cli repl live --url http://127.0.0.1:7777 --bearer-token secret
//! ```
//!
//! Auth: when `token` is set, the listener requires `Authorization: Bearer <token>`
//! on every request. The token is deliberately NOT accepted via query string
//! (credentials in URLs leak into access logs, proxy logs, browser history,
//! and `Referer` headers). When unset, the listener refuses to bind to
//! anything but a loopback address and returns 401 to all routes anyway
//! (defense in depth — if you bind it to `0.0.0.0` without a token, every
//! request still gets 401).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use nestrs_core::{AdminSnapshot, ProviderSummary};
use serde::Serialize;
use subtle::ConstantTimeEq;

/// Configuration for the admin sidecar.
#[derive(Debug, Clone)]
pub struct AdminOptions {
    /// Listen address. The listener will refuse to bind to a non-loopback
    /// address if `token` is `None`.
    pub addr: SocketAddr,
    /// Optional bearer token. When set, callers must present it as
    /// `Authorization: Bearer <token>` (header only — query-string tokens
    /// are rejected). When `None`, every request gets 401 (defense in
    /// depth).
    pub token: Option<String>,
}

/// Handle returned by [`crate::NestApplication::use_admin`]. Drop it to
/// shut the listener down; call [`Self::serve`] to block on it.
pub struct AdminHandle {
    pub(crate) addr: SocketAddr,
    pub(crate) token: Option<String>,
    pub(crate) snapshot_provider: Arc<dyn Fn() -> Arc<AdminSnapshot> + Send + Sync>,
}

impl AdminHandle {
    /// Build a router for the admin endpoints and serve it on the
    /// configured address. Returns when the listener stops (e.g. on
    /// drop of a guard held outside this function).
    pub async fn serve(self) -> std::io::Result<()> {
        let token = self.token.clone();
        let provider = self.snapshot_provider.clone();
        let state = AdminState {
            token,
            snapshot_provider: provider,
        };
        let app: Router = Router::new()
            .route("/__nestrs/health", get(get_health))
            .route("/__nestrs/providers", get(get_providers))
            .route("/__nestrs/routes", get(get_routes))
            .route("/__nestrs/openapi.json", get(get_openapi))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        tracing::info!(addr = %self.addr, "nestrs::admin listening");
        axum::serve(listener, app).await
    }

    /// The address the handle is configured to bind to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

#[derive(Clone)]
struct AdminState {
    token: Option<String>,
    snapshot_provider: Arc<dyn Fn() -> Arc<AdminSnapshot> + Send + Sync>,
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "bearer token required").into_response()
}

/// Small error type for [`authed`]. Returning a full [`Response`] would trip
/// `clippy::result_large_err` (≥128 B) without changing any call-site ergonomics.
#[derive(Debug)]
enum AuthError {
    Unauthorized,
}

impl From<AuthError> for Response {
    fn from(_: AuthError) -> Self {
        unauthorized()
    }
}

/// Constant-time bearer-token comparison. A plain `==` short-circuits at
/// the first differing byte, letting a patient attacker recover the token
/// byte-by-byte from response timing; `subtle`'s `ct_eq` compares every
/// byte unconditionally. Token LENGTH is not treated as a secret (a single
/// early length check — standard practice for bearer credentials).
fn token_matches(presented: &str, expected: &str) -> bool {
    presented.len() == expected.len() && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()))
}

/// Header-only token check: the token is never accepted via query string
/// (credentials in URLs leak into access logs, proxy logs, browser history,
/// and `Referer` headers).
fn check_token(state: &AdminState, auth: Option<&str>) -> bool {
    let Some(expected) = state.token.as_deref() else {
        return false; // no token configured → refuse everything
    };
    let Some(presented) = auth.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    token_matches(presented, expected)
}

fn snapshot(state: &AdminState) -> Arc<AdminSnapshot> {
    (state.snapshot_provider)()
}

async fn authed(
    State(state): State<AdminState>,
    headers: axum::http::HeaderMap,
) -> Result<Arc<AdminSnapshot>, AuthError> {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if !check_token(&state, auth) {
        return Err(AuthError::Unauthorized);
    }
    Ok(snapshot(&state))
}

async fn get_health(State(state): State<AdminState>, headers: axum::http::HeaderMap) -> Response {
    match authed(State(state), headers).await {
        Ok(snap) => {
            let body = serde_json::json!({
                "status": "ok",
                "uptime_ms": snap.uptime_ms,
                "version": snap.version,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => e.into(),
    }
}

async fn get_providers(
    State(state): State<AdminState>,
    headers: axum::http::HeaderMap,
) -> Response {
    match authed(State(state), headers).await {
        Ok(snap) => {
            let providers: Vec<ProviderSummaryJson> = snap
                .providers
                .iter()
                .map(ProviderSummaryJson::from)
                .collect();
            (StatusCode::OK, Json(providers)).into_response()
        }
        Err(e) => e.into(),
    }
}

async fn get_routes(State(state): State<AdminState>, headers: axum::http::HeaderMap) -> Response {
    match authed(State(state), headers).await {
        Ok(snap) => {
            let routes: Vec<RouteInfoJson> = snap.routes.iter().map(RouteInfoJson::from).collect();
            (StatusCode::OK, Json(routes)).into_response()
        }
        Err(e) => e.into(),
    }
}

async fn get_openapi(State(state): State<AdminState>, headers: axum::http::HeaderMap) -> Response {
    match authed(State(state), headers).await {
        Ok(_snap) => {
            // The real OpenAPI doc comes from the `openapi` feature. When
            // that's not enabled, return a minimal summary so callers can
            // still detect the app and version.
            let body = serde_json::json!({
                "openapi": "3.0.0",
                "info": { "title": "nestrs app", "version": env!("CARGO_PKG_VERSION") },
                "note": "full OpenAPI doc requires the `openapi` feature on `nestrs`"
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => e.into(),
    }
}

#[derive(Debug, Serialize)]
struct ProviderSummaryJson {
    type_name: String,
    scope: String,
}

impl From<&ProviderSummary> for ProviderSummaryJson {
    fn from(p: &ProviderSummary) -> Self {
        let scope = match p.scope {
            nestrs_core::ProviderScope::Singleton => "singleton",
            nestrs_core::ProviderScope::Transient => "transient",
            nestrs_core::ProviderScope::Request => "request",
        };
        Self {
            type_name: p.type_name.to_string(),
            scope: scope.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RouteInfoJson {
    method: String,
    path: String,
    handler: String,
    openapi_summary: Option<String>,
}

impl From<&nestrs_core::RouteInfo> for RouteInfoJson {
    fn from(r: &nestrs_core::RouteInfo) -> Self {
        Self {
            method: r.method.to_string(),
            path: r.path.to_string(),
            handler: r.handler.to_string(),
            openapi_summary: r.openapi.and_then(|o| o.summary).map(|s| s.to_string()),
        }
    }
}

/// Validate that `addr` is acceptable. Refuses non-loopback addresses
/// when no token is configured.
pub(crate) fn validate_addr(addr: SocketAddr, token: Option<&str>) -> Result<(), String> {
    if token.is_none() && !addr.ip().is_loopback() {
        return Err(format!(
            "refusing to bind nestrs::admin to non-loopback address {} without a bearer token",
            addr
        ));
    }
    let _ = IpAddr::is_loopback; // keep import used
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn rejects_non_loopback_without_token() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 7777);
        assert!(validate_addr(addr, None).is_err());
    }

    #[test]
    fn allows_loopback_without_token() {
        let addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();
        assert!(validate_addr(addr, None).is_ok());
    }

    #[test]
    fn allows_non_loopback_with_token() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 7777);
        assert!(validate_addr(addr, Some("secret")).is_ok());
    }

    fn state_with(token: Option<&str>) -> AdminState {
        AdminState {
            token: token.map(str::to_string),
            snapshot_provider: Arc::new(|| unreachable!("no snapshot in token tests")),
        }
    }

    #[test]
    fn no_configured_token_refuses_everything() {
        let state = state_with(None);
        assert!(!check_token(&state, Some("Bearer anything")));
        assert!(!check_token(&state, None));
    }

    #[test]
    fn correct_bearer_header_passes() {
        let state = state_with(Some("secret"));
        assert!(check_token(&state, Some("Bearer secret")));
    }

    #[test]
    fn wrong_token_same_length_fails() {
        // Same length exercises the FULL byte comparison — the exact case
        // where a short-circuiting `==` would leak a timing difference at
        // the first differing byte.
        let state = state_with(Some("secret"));
        assert!(!check_token(&state, Some("Bearer secXet")));
        assert!(!check_token(&state, Some("Bearer xeXXXX")));
    }

    #[test]
    fn prefix_correct_but_longer_fails() {
        let state = state_with(Some("secret"));
        assert!(!check_token(&state, Some("Bearer secret-with-more")));
        assert!(!check_token(&state, Some("Bearer secr")));
    }

    #[test]
    fn missing_or_malformed_authorization_fails() {
        let state = state_with(Some("secret"));
        assert!(!check_token(&state, None));
        assert!(!check_token(&state, Some("secret"))); // no Bearer scheme
        assert!(!check_token(&state, Some("Basic c2VjcmV0"))); // wrong scheme
        assert!(!check_token(&state, Some("Bearer"))); // no space, no token
        assert!(!check_token(&state, Some(""))); // empty header
    }

    #[test]
    fn token_matches_is_length_insensitive_at_prefix() {
        // Direct coverage of the constant-time helper, including the
        // length-mismatch arm.
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("secret", "Secret"));
        assert!(!token_matches("secret", "secret2"));
        assert!(!token_matches("", "s"));
        assert!(token_matches("", ""));
    }
}
