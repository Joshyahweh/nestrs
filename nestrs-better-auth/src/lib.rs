//! Better Auth adapter for nestrs.
//!
//! Does **not** reimplement Better Auth. Nest the Axum router from
//! [better-auth.rs](https://github.com/better-auth-rs/better-auth-rs) and
//! protect nestrs handlers with [`BetterAuthGuard`] (session cookie).
//!
//! This crate does not depend on a specific `better-auth` crate version so
//! MSRV 1.88 stays intact; you pass the already-built router in.

#![doc(html_root_url = "https://docs.rs/nestrs-better-auth/1.4.0")]

use async_trait::async_trait;
use axum::http::request::Parts;
use axum::Router;
use nestrs_core::{CanActivate, DynamicModule, GuardError};

/// Default cookie name used by Better Auth / better-auth.rs session cookies.
pub const DEFAULT_SESSION_COOKIE: &str = "better-auth.session_token";

/// Options for [`BetterAuthModule::for_root`].
#[derive(Clone, Debug)]
pub struct BetterAuthOptions {
    /// Mount path for the Better Auth Axum router (Nest `AuthModule` prefix).
    pub path: String,
    /// Session cookie to require in [`BetterAuthGuard`].
    pub session_cookie: String,
}

impl Default for BetterAuthOptions {
    fn default() -> Self {
        Self {
            path: "/api/auth".to_string(),
            session_cookie: DEFAULT_SESSION_COOKIE.to_string(),
        }
    }
}

/// Nest the Better Auth router onto the nestrs module graph.
pub struct BetterAuthModule;

impl BetterAuthModule {
    /// `AuthModule.forRoot` analogue: nest `auth_router` at `options.path`.
    pub fn for_root(auth_router: Router, options: BetterAuthOptions) -> DynamicModule {
        let path = normalize_mount(&options.path);
        DynamicModule::from_router(Router::new().nest(&path, auth_router))
    }
}

fn normalize_mount(path: &str) -> String {
    if path.is_empty() || path == "/" {
        "/api/auth".to_string()
    } else if path.starts_with('/') {
        path.trim_end_matches('/').to_string()
    } else {
        format!("/{}", path.trim_end_matches('/'))
    }
}

/// `AuthGuard` analogue: require the Better Auth session cookie.
///
/// Cookie parsing is header-level (no extra cookie crate) so this works
/// whether or not `NestApplication::use_cookies` is on.
#[derive(Clone, Debug)]
pub struct BetterAuthGuard {
    cookie_name: String,
}

impl Default for BetterAuthGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl BetterAuthGuard {
    /// Guard looking for [`DEFAULT_SESSION_COOKIE`].
    pub fn new() -> Self {
        Self {
            cookie_name: DEFAULT_SESSION_COOKIE.to_string(),
        }
    }

    /// Guard looking for a custom cookie name.
    pub fn with_cookie_name(name: impl Into<String>) -> Self {
        Self {
            cookie_name: name.into(),
        }
    }
}

#[async_trait]
impl CanActivate for BetterAuthGuard {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        if cookie_header_contains(parts, &self.cookie_name) {
            Ok(())
        } else {
            Err(GuardError::unauthorized(
                "missing Better Auth session cookie",
            ))
        }
    }
}

fn cookie_header_contains(parts: &Parts, name: &str) -> bool {
    let Some(value) = parts
        .headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    for pair in value.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name && !v.trim().is_empty() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{cookie_header_contains, normalize_mount};
    use axum::http::request::Parts;
    use axum::http::{header, Request};

    fn parts_with_cookie(cookie: &str) -> Parts {
        Request::builder()
            .header(header::COOKIE, cookie)
            .body(())
            .unwrap()
            .into_parts()
            .0
    }

    #[test]
    fn mount_path_is_absolute() {
        assert_eq!(normalize_mount("api/auth"), "/api/auth");
        assert_eq!(normalize_mount("/api/auth/"), "/api/auth");
    }

    #[test]
    fn session_cookie_is_detected() {
        let parts = parts_with_cookie("better-auth.session_token=abc; other=1");
        assert!(cookie_header_contains(&parts, "better-auth.session_token"));
        assert!(!cookie_header_contains(&parts, "missing"));
    }
}
