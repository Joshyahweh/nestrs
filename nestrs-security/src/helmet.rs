//! Helmet-style response-header middleware (NestJS [`helmet`](https://docs.nestjs.com/security/helmet) parity).
//!
//! Install globally (before your handlers) to add the conservative
//! browser-security defaults. Each field is optional so callers can
//! override, disable, or extend the set without forking this middleware.
//!
//! **Why a Phase D surface:** NestJS helmet exposes a single fixed default
//! set; we let callers pick what they want per response. The defaults here
//! match `helmet@7.x`:
//!
//! - `X-Frame-Options: DENY`
//! - `X-Content-Type-Options: nosniff`
//! - `Strict-Transport-Security: max-age=15552000; includeSubDomains`
//! - `Referrer-Policy: no-referrer`
//! - `X-DNS-Prefetch-Control: off`
//! - `Cross-Origin-Opener-Policy: same-origin`
//!
//! **Docs:** mdBook **Security** (`docs/src/security.md`).

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

/// Configuration for [`helmet_middleware`]. Each field is the **value to
/// write into the response header**, or `None` to omit that header
/// entirely. Defaults match `helmet@7.x` conservative set.
#[derive(Clone, Debug)]
pub struct HelmetConfig {
    /// Value for `X-Frame-Options`. Default `Some("DENY")`.
    pub x_frame_options: Option<String>,
    /// If `true`, send `X-Content-Type-Options: nosniff`. Default `true`.
    pub x_content_type_options: bool,
    /// Value for `Strict-Transport-Security`. Default
    /// `Some("max-age=15552000; includeSubDomains")`.
    pub strict_transport_security: Option<String>,
    /// Value for `Referrer-Policy`. Default `Some("no-referrer")`.
    pub referrer_policy: Option<String>,
    /// Value for `X-DNS-Prefetch-Control`. Default `Some("off")`.
    pub x_dns_prefetch_control: Option<String>,
    /// Value for `Cross-Origin-Opener-Policy`. Default `Some("same-origin")`.
    pub cross_origin_opener_policy: Option<String>,
}

impl Default for HelmetConfig {
    fn default() -> Self {
        Self {
            x_frame_options: Some("DENY".to_string()),
            x_content_type_options: true,
            strict_transport_security: Some("max-age=15552000; includeSubDomains".to_string()),
            referrer_policy: Some("no-referrer".to_string()),
            x_dns_prefetch_control: Some("off".to_string()),
            cross_origin_opener_policy: Some("same-origin".to_string()),
        }
    }
}

fn insert_if_some(headers: &mut axum::http::HeaderMap, name: &'static str, value: Option<&String>) {
    if let Some(v) = value {
        if let Ok(hv) = HeaderValue::from_str(v) {
            headers.insert(name, hv);
        }
    }
}

fn insert_if_bool(
    headers: &mut axum::http::HeaderMap,
    name: &'static str,
    on: bool,
    value: &'static str,
) {
    if on {
        if let Ok(hv) = HeaderValue::from_str(value) {
            headers.insert(name, hv);
        }
    }
}

/// Axum middleware: apply the configured `HelmetConfig` to every response.
///
/// On a downstream response status >= 400 the headers are still written so
/// callers don't have to remember to apply them in error paths.
pub async fn helmet_middleware(
    State(config): State<Arc<HelmetConfig>>,
    req: Request,
    next: Next,
) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    insert_if_some(headers, "x-frame-options", config.x_frame_options.as_ref());
    insert_if_bool(
        headers,
        "x-content-type-options",
        config.x_content_type_options,
        "nosniff",
    );
    insert_if_some(
        headers,
        "strict-transport-security",
        config.strict_transport_security.as_ref(),
    );
    insert_if_some(headers, "referrer-policy", config.referrer_policy.as_ref());
    insert_if_some(
        headers,
        "x-dns-prefetch-control",
        config.x_dns_prefetch_control.as_ref(),
    );
    insert_if_some(
        headers,
        "cross-origin-opener-policy",
        config.cross_origin_opener_policy.as_ref(),
    );
    response
}

// `IntoResponse` is used by callers that want a single `Response` with the
// headers pre-applied (e.g. in a test that hand-builds a router without
// installing the middleware). Re-exported so users don't have to import
// axum::response::IntoResponse separately for this path.
#[allow(dead_code)]
fn _ensure_into_response_in_scope<T: IntoResponse>(_t: &T) -> Option<StatusCode> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_helmet_7() {
        let cfg = HelmetConfig::default();
        assert_eq!(cfg.x_frame_options.as_deref(), Some("DENY"));
        assert!(cfg.x_content_type_options);
        assert_eq!(
            cfg.strict_transport_security.as_deref(),
            Some("max-age=15552000; includeSubDomains")
        );
        assert_eq!(cfg.referrer_policy.as_deref(), Some("no-referrer"));
        assert_eq!(cfg.x_dns_prefetch_control.as_deref(), Some("off"));
        assert_eq!(
            cfg.cross_origin_opener_policy.as_deref(),
            Some("same-origin")
        );
    }

    #[test]
    fn default_can_be_disabled_field_by_field() {
        let cfg = HelmetConfig {
            x_frame_options: None,
            x_content_type_options: false,
            strict_transport_security: None,
            referrer_policy: None,
            x_dns_prefetch_control: None,
            cross_origin_opener_policy: None,
        };
        assert!(cfg.x_frame_options.is_none());
        assert!(!cfg.x_content_type_options);
    }
}
