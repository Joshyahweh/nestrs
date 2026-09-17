//! Phase D extension surface for the throttler.
//!
//! - [`ThrottleKeyGenerator`] turns a [`ThrottlerRequest`] into a per-request
//!   key string. The middleware/guard apply a `{handler}:{key}` scope on top —
//!   this trait only needs to return the per-client identity (`IpKeyGenerator`
//!   returns the IP, `PrincipalKeyGenerator` returns the auth subject id, etc.).
//! - [`ThrottleSkipper`] short-circuits the throttle check before it runs
//!   (e.g. health probes, internal IPs). Default: [`NeverSkip`].
//! - [`ThrottlerRequest`] is the input to both traits; bundles the route
//!   handler id (empty for the global middleware path), the resolved client
//!   IP, and a borrowed `Parts` for header/extension access.
//! - [`PrincipalId`] is the request extension that
//!   [`PrincipalKeyGenerator`] reads; auth middleware (`authn`/`oauth2`)
//!   installs it on the request so authenticated traffic gets per-subject
//!   buckets instead of per-IP ones.

use axum::http::request::Parts;
use axum::http::HeaderName;

/// Per-request view handed to [`ThrottleKeyGenerator`] and [`ThrottleSkipper`].
///
/// `handler` is the route's handler id when known — from the `HandlerKey`
/// extension (guard path) or [`nestrs_core::RouteRegistry::handler_for`]
/// (app-level middleware). Empty when the route isn't registered. `ip` is
/// the rate-limit key string (already the `"unknown"` fallback when
/// unresolvable).
pub struct ThrottlerRequest<'a> {
    pub handler: &'a str,
    pub ip: String,
    pub parts: &'a Parts,
}

/// Request extension installed by auth middleware with the authenticated
/// subject id. Consumed by [`PrincipalKeyGenerator`].
#[derive(Clone, Debug)]
pub struct PrincipalId(pub String);

/// Produces the per-request key passed to the throttle backend.
pub trait ThrottleKeyGenerator: Send + Sync + 'static {
    fn key(&self, req: &ThrottlerRequest<'_>) -> String;
}

/// Pre-check skipper: return `true` to bypass the throttle entirely
/// (e.g. health probes, internal IPs, admin routes).
pub trait ThrottleSkipper: Send + Sync + 'static {
    fn skip(&self, req: &ThrottlerRequest<'_>) -> bool;
}

/// Default skipper: never skip.
pub struct NeverSkip;

impl ThrottleSkipper for NeverSkip {
    fn skip(&self, _req: &ThrottlerRequest<'_>) -> bool {
        false
    }
}

/// Default key generator: the (trusted-aware) client IP. Falls back to the
/// `"unknown"` bucket when unresolvable, coupling the rate limits of
/// unkeyable clients on purpose.
pub struct IpKeyGenerator;

impl ThrottleKeyGenerator for IpKeyGenerator {
    fn key(&self, req: &ThrottlerRequest<'_>) -> String {
        req.ip.clone()
    }
}

/// Per-API-key generator: reads the configured header (default `x-api-key`,
/// overridable via [`ApiKeyHeaderKeyGenerator::new`]). Falls back to the IP
/// key when the header is absent so unkeyed traffic still gets throttled.
pub struct ApiKeyHeaderKeyGenerator {
    header_name: HeaderName,
}

impl ApiKeyHeaderKeyGenerator {
    pub fn new(header_name: HeaderName) -> Self {
        Self { header_name }
    }
}

impl Default for ApiKeyHeaderKeyGenerator {
    fn default() -> Self {
        Self {
            header_name: HeaderName::from_static("x-api-key"),
        }
    }
}

impl ThrottleKeyGenerator for ApiKeyHeaderKeyGenerator {
    fn key(&self, req: &ThrottlerRequest<'_>) -> String {
        req.parts
            .headers
            .get(&self.header_name)
            .and_then(|v| v.to_str().ok())
            .map(|raw| format!("api:{raw}"))
            .unwrap_or_else(|| req.ip.clone())
    }
}

/// Per-principal generator: reads the [`PrincipalId`] extension installed by
/// auth middleware. Falls back to the IP key for anonymous requests.
pub struct PrincipalKeyGenerator;

impl ThrottleKeyGenerator for PrincipalKeyGenerator {
    fn key(&self, req: &ThrottlerRequest<'_>) -> String {
        req.parts
            .extensions
            .get::<PrincipalId>()
            .map(|p| format!("user:{}", p.0))
            .unwrap_or_else(|| req.ip.clone())
    }
}
