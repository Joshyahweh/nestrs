//! Configuration for the nestrs OAuth2 authorization server.

use std::time::Duration;

use ed25519_dalek::SigningKey;
use pkcs8::DecodePrivateKey;
use thiserror::Error;
use url::Url;

/// Errors from building an [`AuthorizationServerConfig`].
#[derive(Debug, Error)]
pub enum AuthorizationServerConfigError {
    /// The signing key PEM could not be parsed as an Ed25519 PKCS#8 key.
    #[error("invalid signing key PEM (expected an Ed25519 PKCS#8 PEM): {0}")]
    SigningKey(String),
    /// The issuer is not an absolute `http(s)` URL.
    #[error("issuer must be an absolute http(s) URL: {0}")]
    Issuer(String),
    /// A mounted path must start with `/` and not end with `/`.
    #[error("{what} must start with '/' and not end with '/' (got {value:?})")]
    Path {
        what: &'static str,
        value: String,
    },
    /// A TTL must be non-zero.
    #[error("{which} must be non-zero")]
    ZeroTtl { which: &'static str },
    /// The `kid` must be non-empty.
    #[error("kid must be a non-empty string")]
    EmptyKid,
}

/// Configuration for the nestrs OAuth2 authorization server.
///
/// Built with [`AuthorizationServerConfig::new`] (which validates and
/// parses the signing key eagerly — fail fast on misconfiguration) and
/// customized with the chainable builders. Defaults:
///
/// | setting | default |
/// |---|---|
/// | access-token TTL | 15 minutes |
/// | refresh-token TTL | 30 days |
/// | authorization-code TTL | 10 minutes |
/// | endpoint prefix | `/oauth` |
/// | discovery path | `/.well-known/oauth-authorization-server` |
/// | JWKS path | `/.well-known/jwks.json` |
/// | PKCE for confidential clients | **required** (OAuth 2.1 posture) |
///
/// The signing key is a single Ed25519 key; `kid` rotation is a
/// configuration-level change (swap the key + `kid` and restart — clients
/// refresh the JWKS on unknown-`kid`, so the cutover is seamless).
#[derive(Clone)]
pub struct AuthorizationServerConfig {
    pub(crate) issuer: String,
    signing_key_pem: String,
    signing_key: SigningKey,
    pub(crate) kid: String,
    pub(crate) access_token_ttl: Duration,
    pub(crate) refresh_token_ttl: Duration,
    pub(crate) authorization_code_ttl: Duration,
    pub(crate) endpoint_prefix: String,
    pub(crate) discovery_path: String,
    pub(crate) jwks_path: String,
    pub(crate) require_pkce_for_confidential: bool,
    pub(crate) scopes_supported: Vec<String>,
}

impl AuthorizationServerConfig {
    /// New config with the given issuer, Ed25519 PKCS#8 signing-key PEM,
    /// and `kid` (the JWKS key id clients pin verification to).
    ///
    /// The issuer must be an absolute `http(s)` URL — the base every
    /// endpoint URL in the discovery document is derived from (RFC 8414).
    /// The PEM is parsed eagerly so a bad key fails at construction,
    /// not at first token issue.
    pub fn new(
        issuer: impl AsRef<str>,
        signing_key_pem: impl AsRef<str>,
        kid: impl Into<String>,
    ) -> Result<Self, AuthorizationServerConfigError> {
        let signing_key = SigningKey::from_pkcs8_pem(signing_key_pem.as_ref().trim())
            .map_err(|e| AuthorizationServerConfigError::SigningKey(e.to_string()))?;

        let issuer_raw = issuer.as_ref().trim().trim_end_matches('/').to_string();
        let parsed = Url::parse(&issuer_raw)
            .map_err(|_| AuthorizationServerConfigError::Issuer(issuer_raw.clone()))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(AuthorizationServerConfigError::Issuer(issuer_raw));
        }

        let kid = kid.into();
        if kid.is_empty() {
            return Err(AuthorizationServerConfigError::EmptyKid);
        }

        Ok(Self {
            issuer: issuer_raw,
            signing_key_pem: signing_key_pem.as_ref().to_string(),
            signing_key,
            kid,
            access_token_ttl: Duration::from_secs(15 * 60),
            refresh_token_ttl: Duration::from_secs(30 * 24 * 60 * 60),
            authorization_code_ttl: Duration::from_secs(10 * 60),
            endpoint_prefix: "/oauth".to_string(),
            discovery_path: "/.well-known/oauth-authorization-server".to_string(),
            jwks_path: "/.well-known/jwks.json".to_string(),
            require_pkce_for_confidential: true,
            scopes_supported: Vec::new(),
        })
    }

    /// Access-token lifetime (default 15 minutes). Short-lived by design;
    /// refresh tokens are the long-lived credential.
    pub fn access_token_ttl(mut self, ttl: Duration) -> Self {
        self.access_token_ttl = ttl;
        self
    }

    /// Refresh-token lifetime (default 30 days).
    pub fn refresh_token_ttl(mut self, ttl: Duration) -> Self {
        self.refresh_token_ttl = ttl;
        self
    }

    /// Authorization-code lifetime (default 10 minutes). RFC 6749
    /// recommends the maximum of 10 minutes.
    pub fn authorization_code_ttl(mut self, ttl: Duration) -> Self {
        self.authorization_code_ttl = ttl;
        self
    }

    /// Prefix all protocol endpoints live under (default `/oauth` →
    /// `/oauth/authorize`, `/oauth/token`, `/oauth/introspect`,
    /// `/oauth/revoke`). Must start with `/` and not end with `/`.
    /// Discovery (`/.well-known/...`) paths are configured separately.
    pub fn endpoint_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.endpoint_prefix = prefix.into();
        self
    }

    /// Path the RFC 8414 discovery document is served at.
    pub fn discovery_path(mut self, path: impl Into<String>) -> Self {
        self.discovery_path = path.into();
        self
    }

    /// Path the JWKS document is served at.
    pub fn jwks_path(mut self, path: impl Into<String>) -> Self {
        self.jwks_path = path.into();
        self
    }

    /// Whether confidential clients must also use PKCE (S256). Default
    /// `true` — OAuth 2.1 and RFC 8252 posture: every client uses PKCE.
    /// Set `false` for confidential clients that cannot.
    pub fn require_pkce_for_confidential(mut self, require: bool) -> Self {
        self.require_pkce_for_confidential = require;
        self
    }

    /// Scopes advertised in the discovery document
    /// (`scopes_supported`). Purely informational — actual scope
    /// authorization is per-client, in [`crate::authorization_server::model::OAuth2ClientRecord`].
    pub fn scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes_supported = scopes.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    pub(crate) fn signing_key_pem(&self) -> &str {
        &self.signing_key_pem
    }

    /// Final validation, run when the server is constructed (paths +
    /// TTLs that the builders accepted as raw input).
    pub(crate) fn validate(&self) -> Result<(), AuthorizationServerConfigError> {
        validate_path("endpoint_prefix", &self.endpoint_prefix)?;
        validate_path("discovery_path", &self.discovery_path)?;
        validate_path("jwks_path", &self.jwks_path)?;
        if self.access_token_ttl.is_zero() {
            return Err(AuthorizationServerConfigError::ZeroTtl {
                which: "access_token_ttl",
            });
        }
        if self.refresh_token_ttl.is_zero() {
            return Err(AuthorizationServerConfigError::ZeroTtl {
                which: "refresh_token_ttl",
            });
        }
        if self.authorization_code_ttl.is_zero() {
            return Err(AuthorizationServerConfigError::ZeroTtl {
                which: "authorization_code_ttl",
            });
        }
        Ok(())
    }
}

fn validate_path(what: &'static str, value: &str) -> Result<(), AuthorizationServerConfigError> {
    if value.starts_with('/') && !value.ends_with('/') && value.len() >= 2 {
        Ok(())
    } else {
        Err(AuthorizationServerConfigError::Path {
            what,
            value: value.to_string(),
        })
    }
}
