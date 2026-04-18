//! Client IP extraction (NestJS `@Ip()` analogue).
//!
//! By default, [`ClientIp`] prefers connection metadata from Axum `ConnectInfo<SocketAddr>` when
//! available (enabled by `NestApplication::listen*` in this crate). As a fallback, it will attempt
//! to parse `x-forwarded-for` (first IP) and then `x-real-ip`.

use axum::extract::connect_info::ConnectInfo;
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

impl IntoResponse for ClientIpMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: ClientIp extractor requires ConnectInfo or forwarded headers",
        )
            .into_response()
    }
}

fn parse_forwarded_ip(raw: &str) -> Option<IpAddr> {
    // `x-forwarded-for` can be a comma-separated list. We take the first entry.
    let first = raw.split(',').next()?.trim();
    // Some proxies include a port (e.g. `1.2.3.4:1234`). Try SocketAddr first.
    if let Ok(sa) = first.parse::<SocketAddr>() {
        return Some(sa.ip());
    }
    first.parse::<IpAddr>().ok()
}

pub(crate) fn best_effort_client_ip(
    headers: &HeaderMap,
    extensions: &Extensions,
) -> Option<IpAddr> {
    if let Some(ConnectInfo(addr)) = extensions.get::<ConnectInfo<SocketAddr>>() {
        return Some(addr.ip());
    }

    if let Some(v) = headers
        .get(&X_FORWARDED_FOR)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_forwarded_ip)
    {
        return Some(v);
    }

    headers
        .get(&X_REAL_IP)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<IpAddr>().ok())
}

pub(crate) fn best_effort_client_ip_from_request(req: &axum::extract::Request) -> Option<IpAddr> {
    best_effort_client_ip(req.headers(), req.extensions())
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = ClientIpMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        best_effort_client_ip(&parts.headers, &parts.extensions)
            .map(Self)
            .ok_or(ClientIpMissing)
    }
}

#[cfg(test)]
mod tests {
    use super::{best_effort_client_ip, X_FORWARDED_FOR, X_REAL_IP};
    use axum::extract::connect_info::ConnectInfo;
    use axum::http::{Extensions, HeaderMap, HeaderValue};
    use std::net::{IpAddr, SocketAddr};

    #[test]
    fn connect_info_takes_precedence_over_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_FORWARDED_FOR,
            HeaderValue::from_static("203.0.113.10, 198.51.100.10"),
        );

        let mut extensions = Extensions::new();
        extensions.insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4321))));

        assert_eq!(
            best_effort_client_ip(&headers, &extensions),
            Some(IpAddr::from([127, 0, 0, 1]))
        );
    }

    #[test]
    fn forwarded_headers_are_used_when_connect_info_is_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_FORWARDED_FOR,
            HeaderValue::from_static("203.0.113.10, 198.51.100.10"),
        );
        headers.insert(&X_REAL_IP, HeaderValue::from_static("198.51.100.20"));

        assert_eq!(
            best_effort_client_ip(&headers, &Extensions::new()),
            Some(IpAddr::from([203, 0, 113, 10]))
        );
    }
}
