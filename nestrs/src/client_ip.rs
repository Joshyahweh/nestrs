//! Client IP extraction (NestJS `@Ip()` analogue).
//!
//! Resolution order:
//!
//! 1. Forwarded headers (`x-forwarded-for`, then `x-real-ip`) — **only** when a trusted-proxy
//!    hop count has been configured via [`crate::NestApplication::use_trusted_proxy_headers`].
//!    Behind a reverse proxy, connection metadata is the *proxy's* address, so a declared
//!    trusted topology takes precedence over it.
//! 2. Connection metadata from Axum `ConnectInfo<SocketAddr>` when available (enabled by
//!    `NestApplication::listen*`) — used whenever no hop count is configured.
//!
//! Forwarded headers are client-controlled. Trusting them without knowing your proxy topology
//! lets callers spoof their IP (bypassing IP-based rate limits, poisoning other clients'
//! windows). When `hops` is configured, each of the `hops` trusted proxies appends exactly one
//! entry (its peer) to `x-forwarded-for`, so the client IP as seen by your outermost proxy is
//! the entry `hops` positions from the end of the list; entries further left are
//! client-controlled and are never selected.

use axum::extract::connect_info::{ConnectInfo, MockConnectInfo};
use axum::http::request::Parts;
use axum::http::Extensions;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};

static X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
static X_REAL_IP: HeaderName = HeaderName::from_static("x-real-ip");

/// Extracts the best-effort client IP address for the current request.
pub struct ClientIp(pub IpAddr);

/// Returned when an IP address cannot be determined.
#[derive(Debug)]
pub struct ClientIpMissing;

/// Extension carrying the configured trusted-proxy hop count (installed per request by
/// [`crate::NestApplication::use_trusted_proxy_headers`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrustedProxyHops(pub u16);

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

fn parse_forwarded_ip(raw: &str) -> Option<IpAddr> {
    // Some proxies include a port (e.g. `1.2.3.4:1234`). Try SocketAddr first.
    if let Ok(sa) = raw.parse::<SocketAddr>() {
        return Some(sa.ip());
    }
    raw.parse::<IpAddr>().ok()
}

/// Resolves the client IP given the optional trusted-proxy hop count.
///
/// - `None` / `Some(0)` — connection metadata only; forwarded headers are ignored.
/// - `Some(n)` with `n >= 1` — take the `x-forwarded-for` entry `n` positions from the *end*
///   of the chain (the value appended by the outermost trusted proxy); fall back to
///   `x-real-ip` (set by the outermost proxy) only when the XFF chain is shorter than the
///   hop count.
pub(crate) fn best_effort_client_ip(
    headers: &HeaderMap,
    extensions: &Extensions,
    trusted_hops: Option<u16>,
) -> Option<IpAddr> {
    // `MockConnectInfo` is stored as its own extension type until Axum's `ConnectInfo` extractor
    // maps it (see axum `ConnectInfo::from_request_parts`). It represents the *actual* peer in
    // test setups (no proxy in front), so it keeps precedence over forwarded headers.
    if let Some(MockConnectInfo(addr)) = extensions.get::<MockConnectInfo<SocketAddr>>() {
        return Some(addr.ip());
    }

    let hops = trusted_hops.unwrap_or(0);

    // When a trusted-proxy topology is declared, connection metadata is the *proxy's* address,
    // not the client's — forwarded headers must take precedence.
    if hops == 0 {
        if let Some(ConnectInfo(addr)) = extensions.get::<ConnectInfo<SocketAddr>>() {
            return Some(addr.ip());
        }
        return None;
    }

    // Duplicate XFF headers are legal; proxies may emit several. Join them so hop indexing
    // covers the full chain rather than a truncated list.
    let xff_values: Vec<&str> = headers
        .get_all(&X_FORWARDED_FOR)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if !xff_values.is_empty() {
        let v = xff_values.join(",");
        let entries: Vec<&str> = v
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        // Each of the `hops` trusted proxies appended exactly one entry (its peer), so the
        // client IP as seen by the outermost trusted proxy sits at `len - hops`. Entries to
        // its left are client-controlled and must never be selected.
        if let Some(idx) = entries.len().checked_sub(hops as usize) {
            if let Some(ip) = parse_forwarded_ip(entries[idx]) {
                return Some(ip);
            }
        }
    }

    headers
        .get(&X_REAL_IP)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_forwarded_ip)
}

pub(crate) fn best_effort_client_ip_from_request(
    headers: &HeaderMap,
    extensions: &Extensions,
    trusted_hops: Option<u16>,
) -> Option<IpAddr> {
    best_effort_client_ip(headers, extensions, trusted_hops)
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = ClientIpMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let hops = parts.extensions.get::<TrustedProxyHops>().map(|t| t.0);
        best_effort_client_ip(&parts.headers, &parts.extensions, hops)
            .map(Self)
            .ok_or(ClientIpMissing)
    }
}

#[cfg(test)]
mod tests {
    use super::{best_effort_client_ip, TrustedProxyHops, X_FORWARDED_FOR, X_REAL_IP};
    use axum::extract::connect_info::{ConnectInfo, MockConnectInfo};
    use axum::http::{Extensions, HeaderMap, HeaderValue};
    use std::net::{IpAddr, SocketAddr};

    fn xff() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_FORWARDED_FOR,
            HeaderValue::from_static("203.0.113.10, 198.51.100.10"),
        );
        headers.insert(&X_REAL_IP, HeaderValue::from_static("198.51.100.20"));
        headers
    }

    #[test]
    fn configured_hops_override_connect_info_behind_proxy() {
        // Behind a reverse proxy, ConnectInfo is the *proxy's* address; with a declared
        // trusted-proxy topology the forwarded chain must win.
        let headers = xff();
        let mut extensions = Extensions::new();
        extensions.insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4321))));
        extensions.insert(TrustedProxyHops(2));

        assert_eq!(
            best_effort_client_ip(&headers, &extensions, Some(2)),
            Some(IpAddr::from([203, 0, 113, 10]))
        );
    }

    #[test]
    fn forwarded_headers_are_ignored_without_trusted_proxies() {
        // Secure default: spoofable forwarded headers are not consulted unless configured.
        let mut headers = HeaderMap::new();
        headers.insert(&X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.10"));
        headers.insert(&X_REAL_IP, HeaderValue::from_static("198.51.100.20"));

        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), None),
            None
        );
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(0)),
            None
        );
    }

    #[test]
    fn one_trusted_proxy_uses_rightmost_xff_entry() {
        // Client -> our LB (appends "198.51.100.10") -> app.
        let headers = xff();
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(1)),
            Some(IpAddr::from([198, 51, 100, 10]))
        );
    }

    #[test]
    fn two_trusted_proxies_use_second_from_right() {
        let headers = xff();
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(2)),
            Some(IpAddr::from([203, 0, 113, 10]))
        );
    }

    #[test]
    fn real_ip_fallback_when_xff_shorter_than_hop_count() {
        let mut headers = HeaderMap::new();
        headers.insert(&X_REAL_IP, HeaderValue::from_static("198.51.100.20"));
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(1)),
            Some(IpAddr::from([198, 51, 100, 20]))
        );
    }

    #[test]
    fn spoofed_left_entries_cannot_override_trusted_resolution() {
        // Attacker prepends a fake IP; with one trusted hop it must be ignored.
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_FORWARDED_FOR,
            HeaderValue::from_static("6.6.6.6, 203.0.113.10"),
        );
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(1)),
            Some(IpAddr::from([203, 0, 113, 10]))
        );
    }

    #[test]
    fn honest_single_hop_traffic_resolves_the_appended_entry() {
        // Client -> our LB (appends the client IP) -> app: the only XFF entry is the client's
        // and must be selected (regression: `len - 1 - hops` underflowed here and fell through
        // to `x-real-ip`, or panicked on `len == hops` arithmetic).
        let mut headers = HeaderMap::new();
        headers.insert(&X_FORWARDED_FOR, HeaderValue::from_static("198.51.100.7"));
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(1)),
            Some(IpAddr::from([198, 51, 100, 7]))
        );
    }

    #[test]
    fn duplicate_xff_headers_are_joined_before_hop_indexing() {
        // Two separate XFF header lines (legal per RFC 7239 predecessors): hop indexing must
        // see the full chain, not just the first line.
        let mut headers = HeaderMap::new();
        headers.append(&X_FORWARDED_FOR, HeaderValue::from_static("6.6.6.6"));
        headers.append(&X_FORWARDED_FOR, HeaderValue::from_static("198.51.100.10"));
        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new(), Some(1)),
            Some(IpAddr::from([198, 51, 100, 10]))
        );
    }

    #[test]
    fn mock_connect_info_is_visible_to_best_effort() {
        let mut extensions = Extensions::new();
        extensions.insert(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4321))));

        assert_eq!(
            best_effort_client_ip(&HeaderMap::new(), &extensions, None),
            Some(IpAddr::from([127, 0, 0, 1]))
        );
    }
}
