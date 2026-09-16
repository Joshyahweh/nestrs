//! Client IP extraction (NestJS `@Ip()` analogue).
//!
//! This file holds the **platform-specific** shape of the client IP — the
//! [`ClientIp`] extractor and its [`ClientIpMissing`] rejection, both of which
//! depend on `axum::extract::FromRequestParts`. The actual resolution logic
//! (forwarded-header trust, hop indexing, the rate-limit `"unknown"` key
//! fallback) lives in [`nestrs_core::client_ip`] so the rate limiter and the
//! throttler crate can both reach it without depending on `nestrs`.
//!
//! Resolution order:
//!
//! 1. Forwarded headers (`x-forwarded-for`, then `x-real-ip`) — **only** when a trusted-proxy
//!    hop count has been configured via [`crate::NestApplication::use_trusted_proxy_headers`].
//!    Behind a reverse proxy, connection metadata is the *proxy's* address, so a declared
//!    trusted topology takes precedence over it.
//! 2. Connection metadata from Axum `ConnectInfo<SocketAddr>` when available (enabled by
//!    `NestApplication::listen*`) — used whenever no hop count is configured.

use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nestrs_core::client_ip::{best_effort_client_ip, trusted_hops_from_parts, RateLimitKey};
use std::net::IpAddr;

// Re-export the shared pieces so `crate::client_ip::*` keeps working for
// the rate limiter, the throttler, and any user code that imported them.
pub use nestrs_core::client_ip::{
    rate_limit_key_ip_or_unknown, TrustedProxyHops, X_FORWARDED_FOR, X_REAL_IP,
};

/// Extracts the best-effort client IP address for the current request.
pub struct ClientIp(pub IpAddr);

/// Returned when an IP address cannot be determined.
#[derive(Debug)]
pub struct ClientIpMissing;

impl IntoResponse for ClientIpMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: ClientIp extractor requires ConnectInfo or forwarded headers \
             (configure NestApplication::use_trusted_proxy_headers to trust forwarded headers)",
        )
            .into_response()
    }
}

/// Client IP for a rate-limit / throttle key, with the shared `"unknown"`
/// fallback and an operator-visible warning when the fallback fires.
///
/// `nestrs-core` returns [`RateLimitKey`]; this wrapper logs the warn that
/// `nestrs-core` cannot (it stays free of `tracing`). This is the function
/// the rate limiter, the global throttler middleware, and the
/// `ThrottlerGuard` all share — one consistent fallback everywhere.
pub fn rate_limit_key_ip(
    headers: &axum::http::HeaderMap,
    extensions: &axum::http::Extensions,
    trusted_hops: Option<u16>,
) -> String {
    let rk = RateLimitKey::resolve(headers, extensions, trusted_hops);
    if rk.fell_back {
        tracing::warn!(
            target: "nestrs::client_ip",
            "no client IP could be resolved; rate limiting into the shared \"unknown\" \
             bucket (check trusted-proxy hops and x-forwarded-for health)"
        );
    }
    rk.key
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = ClientIpMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let hops = trusted_hops_from_parts(parts, None);
        best_effort_client_ip(&parts.headers, &parts.extensions, hops)
            .map(Self)
            .ok_or(ClientIpMissing)
    }
}