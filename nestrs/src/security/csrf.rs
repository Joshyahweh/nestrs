//! Double-submit CSRF check for cookie-based sessions (feature **`csrf`**, requires **`cookies`**).
//!
//! For unsafe HTTP methods, compares a cookie value to a header value using a constant-time equality check.
//! Issue the token on first visit (e.g. a GET that sets `Set-Cookie`) and send the same value in `X-CSRF-Token` on mutations.

use axum::extract::Request;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tower_cookies::Cookies;

/// Configuration for [`csrf_double_submit_middleware`].
#[derive(Clone, Debug)]
pub struct CsrfProtectionConfig {
    /// Cookie that holds the CSRF secret (must also be sent by the client as a non-HttpOnly duplicate or mirrored header — typical double-submit).
    pub cookie_name: &'static str,
    /// Header the client must send on unsafe methods (e.g. `x-csrf-token`).
    pub header_name: HeaderName,
}

impl Default for CsrfProtectionConfig {
    fn default() -> Self {
        Self {
            cookie_name: "csrf_token",
            header_name: HeaderName::from_static("x-csrf-token"),
        }
    }
}

fn is_unsafe_method(m: &Method) -> bool {
    matches!(
        *m,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn constant_time_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Axum middleware: rejects unsafe requests when cookie and header tokens are missing or unequal.
pub async fn csrf_double_submit_middleware(
    axum::extract::State(config): axum::extract::State<Arc<CsrfProtectionConfig>>,
    req: Request,
    next: Next,
) -> Response {
    if !is_unsafe_method(req.method()) {
        return next.run(req).await;
    }

    let (parts, body) = req.into_parts();
    let Some(cookies) = parts.extensions.get::<Cookies>() else {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "statusCode": 403,
                "message": "CSRF check requires CookieManagerLayer (use NestApplication::use_cookies)",
                "error": "Forbidden",
            })),
        )
            .into_response();
    };

    let cookie_s = cookies
        .get(config.cookie_name)
        .map(|c| c.value().to_string());
    let header_s = parts
        .headers
        .get(&config.header_name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let ok = match (&cookie_s, &header_s) {
        (Some(a), Some(b)) => constant_time_eq_bytes(a.as_bytes(), b.as_bytes()),
        _ => false,
    };

    if !ok {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "statusCode": 403,
                "message": "CSRF token missing or invalid",
                "error": "Forbidden",
            })),
        )
            .into_response();
    }

    let req = Request::from_parts(parts, body);
    next.run(req).await
}
