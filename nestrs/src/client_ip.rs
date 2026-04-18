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

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Ok(ConnectInfo(addr)) =
            <ConnectInfo<SocketAddr> as axum::extract::FromRequestParts<S>>::from_request_parts(
                parts, state,
            )
            .await
        {
            return Ok(Self(addr.ip()));
        }

        best_effort_client_ip(&parts.headers, &parts.extensions)
            .map(Self)
            .ok_or(ClientIpMissing)
    }
}
