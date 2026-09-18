//! Passport-style strategies for nestrs (`@nestjs/passport` analogue).
//!
//! Nest Passport wraps Node `passport` strategies. nestrs already has
//! [`AuthStrategy`] + [`AuthStrategyGuard`]. This crate ships the two
//! strategies apps reach for first: JWT bearer and HTTP Basic (local).

#![doc(html_root_url = "https://docs.rs/nestrs-passport/1.3.0")]

use async_trait::async_trait;
use axum::http::request::Parts;
use nestrs_core::{AuthError, AuthStrategy};
use nestrs_security::parse_authorization_bearer;

pub use nestrs_security::AuthStrategyGuard as PassportGuard;

/// JWT bearer strategy: `Authorization: Bearer <token>`, then `validate`.
pub struct JwtStrategy<F> {
    validate: F,
}

impl<F> JwtStrategy<F> {
    /// `validate` receives the raw bearer token (not a parsed JWT).
    pub fn new(validate: F) -> Self {
        Self { validate }
    }
}

#[async_trait]
impl<F, Fut, T> AuthStrategy for JwtStrategy<F>
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<T, AuthError>> + Send,
    T: Send + Sync + 'static,
{
    type Payload = T;

    async fn validate(&self, parts: &Parts) -> Result<Self::Payload, AuthError> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AuthError::unauthorized("missing Authorization header"))?;
        let token = parse_authorization_bearer(header)
            .ok_or_else(|| AuthError::unauthorized("expected Bearer token"))?;
        (self.validate)(token.to_string()).await
    }
}

/// HTTP Basic strategy (`passport-local` analogue when the password rides
/// in `Authorization` rather than a JSON body — `AuthStrategy` only sees
/// request parts).
pub struct LocalBasicStrategy<F> {
    validate: F,
}

impl<F> LocalBasicStrategy<F> {
    /// `validate(username, password)`.
    pub fn new(validate: F) -> Self {
        Self { validate }
    }
}

#[async_trait]
impl<F, Fut, T> AuthStrategy for LocalBasicStrategy<F>
where
    F: Fn(String, String) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<T, AuthError>> + Send,
    T: Send + Sync + 'static,
{
    type Payload = T;

    async fn validate(&self, parts: &Parts) -> Result<Self::Payload, AuthError> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AuthError::unauthorized("missing Authorization header"))?;
        let (user, pass) = parse_basic(header)
            .ok_or_else(|| AuthError::unauthorized("expected Basic credentials"))?;
        (self.validate)(user, pass).await
    }
}

fn parse_basic(header: &str) -> Option<(String, String)> {
    let mut iter = header.splitn(2, char::is_whitespace);
    let scheme = iter.next()?.trim_end_matches(':');
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let raw = iter.next()?.trim();
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

#[cfg(test)]
mod tests {
    use super::parse_basic;
    use base64::Engine;

    #[test]
    fn basic_header_round_trip() {
        let token = base64::engine::general_purpose::STANDARD.encode("ada:s3cret");
        let header = format!("Basic {token}");
        assert_eq!(
            parse_basic(&header),
            Some(("ada".to_string(), "s3cret".to_string()))
        );
        assert!(parse_basic("Bearer abc").is_none());
    }
}
