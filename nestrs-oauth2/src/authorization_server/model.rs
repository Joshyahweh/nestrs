//! Data model for the authorization server: registered client records
//! and the stored (hashed) credential material the stores persist.

use std::fmt;

use super::{constant_time_eq, sha256_hex};
use url::Url;

/// The grants a client (or request) may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GrantType {
    /// RFC 6749 §4.1 — `/authorize` → code → `/token`.
    AuthorizationCode,
    /// RFC 6749 §6 — rotate refresh tokens at `/token`.
    RefreshToken,
    /// RFC 6749 §4.4 — machine-to-machine, confidential clients only.
    ClientCredentials,
}

impl GrantType {
    /// The wire name (the `grant_type` form value).
    pub fn as_str(&self) -> &'static str {
        match self {
            GrantType::AuthorizationCode => "authorization_code",
            GrantType::RefreshToken => "refresh_token",
            GrantType::ClientCredentials => "client_credentials",
        }
    }

    /// Parse the `grant_type` form value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "authorization_code" => Some(GrantType::AuthorizationCode),
            "refresh_token" => Some(GrantType::RefreshToken),
            "client_credentials" => Some(GrantType::ClientCredentials),
            _ => None,
        }
    }
}

/// Errors from building an [`OAuth2ClientRecord`].
#[derive(Debug, thiserror::Error)]
pub enum ClientRecordError {
    #[error("client_id must be a non-empty string")]
    EmptyClientId,
    #[error("a confidential client requires a non-empty client_secret")]
    EmptySecret,
    #[error("redirect URI {uri:?} must be an absolute http(s) URL without a fragment")]
    InvalidRedirectUri { uri: String },
}

/// A registered OAuth2 client, as the [`crate::authorization_server::stores::ClientStore`]
/// hands back on lookup.
///
/// The client secret is stored **hashed** (SHA-256 of the secret) — like a
/// password hash, registration can validate without being able to recover
/// the secret. Present secrets are compared constant-time over the
/// digests. Generate client secrets with a CSPRNG (32+ random bytes);
/// a fast hash is appropriate for high-entropy secrets but not for
/// human-chosen passwords.
///
/// Redirect URIs are an **exact-match allow-list**: `https://a/cb` and
/// `https://a/cb?x=1` are different registrations, and `/authorize`
/// only ever redirects to a byte-for-byte registered URI. This (plus
/// never redirecting before the client + redirect URI are validated) is
/// the open-redirect protection.
#[derive(Clone)]
pub struct OAuth2ClientRecord {
    pub client_id: String,
    /// SHA-256 hex of the secret; `None` for public clients.
    client_secret_hash: Option<String>,
    pub is_public: bool,
    redirect_uris: Vec<String>,
    allowed_grants: Vec<GrantType>,
    allowed_scopes: Vec<String>,
}

impl OAuth2ClientRecord {
    /// A confidential client (one that can keep a secret server-side).
    ///
    /// `redirect_uris` may be empty for a client using only the
    /// `client_credentials` grant.
    pub fn confidential(
        client_id: impl Into<String>,
        client_secret: &str,
        redirect_uris: Vec<String>,
        allowed_grants: Vec<GrantType>,
        allowed_scopes: Vec<String>,
    ) -> Result<Self, ClientRecordError> {
        let client_id = client_id.into();
        if client_id.is_empty() {
            return Err(ClientRecordError::EmptyClientId);
        }
        if client_secret.is_empty() {
            return Err(ClientRecordError::EmptySecret);
        }
        Ok(Self {
            client_secret_hash: Some(sha256_hex(client_secret.as_bytes())),
            is_public: false,
            redirect_uris: validate_redirect_uris(redirect_uris)?,
            allowed_grants,
            allowed_scopes: dedup(allowed_scopes),
            client_id,
        })
    }

    /// A public client (browser SPA / native app — no secret can be kept).
    /// PKCE (S256) is mandatory for these at `/authorize`.
    pub fn public(
        client_id: impl Into<String>,
        redirect_uris: Vec<String>,
        allowed_grants: Vec<GrantType>,
        allowed_scopes: Vec<String>,
    ) -> Result<Self, ClientRecordError> {
        let client_id = client_id.into();
        if client_id.is_empty() {
            return Err(ClientRecordError::EmptyClientId);
        }
        Ok(Self {
            client_secret_hash: None,
            is_public: true,
            redirect_uris: validate_redirect_uris(redirect_uris)?,
            allowed_grants,
            allowed_scopes: dedup(allowed_scopes),
            client_id,
        })
    }

    /// Constant-time check of a presented client secret against the
    /// stored digest. `false` for public clients (no secret exists).
    pub(crate) fn check_secret(&self, presented: &str) -> bool {
        match &self.client_secret_hash {
            Some(stored) => {
                let presented_hash = sha256_hex(presented.as_bytes());
                constant_time_eq(presented_hash.as_bytes(), stored.as_bytes())
            }
            None => false,
        }
    }

    /// Exact byte-for-byte redirect-URI allow-list check.
    pub fn has_redirect(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|r| r == uri)
    }

    pub fn redirect_uris(&self) -> &[String] {
        &self.redirect_uris
    }

    pub fn allowed_grants(&self) -> &[GrantType] {
        &self.allowed_grants
    }

    pub fn allows_grant(&self, grant: GrantType) -> bool {
        self.allowed_grants.contains(&grant)
    }

    /// Whether every requested scope is in the client's allow-list.
    /// An empty allow-list authorizes nothing (scopes are opt-in).
    pub fn allows_scopes(&self, scopes: &[String]) -> bool {
        scopes.iter().all(|s| self.allowed_scopes.contains(s))
    }

    pub fn allowed_scopes(&self) -> &[String] {
        &self.allowed_scopes
    }
}

// Manual Debug: never print the secret digest — even a hash is
// fingerprinting material that does not belong in logs.
impl fmt::Debug for OAuth2ClientRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2ClientRecord")
            .field("client_id", &self.client_id)
            .field(
                "client_secret_hash",
                &self.client_secret_hash.as_ref().map(|_| "<redacted>"),
            )
            .field("is_public", &self.is_public)
            .field("redirect_uris", &self.redirect_uris)
            .field("allowed_grants", &self.allowed_grants)
            .field("allowed_scopes", &self.allowed_scopes)
            .finish()
    }
}

/// A consumed (single-use) authorization code, stored **keyed by the
/// SHA-256 hex of the code** — the plaintext code only ever exists in
/// the redirect to the client and the one `/token` request that redeems
/// it. No store ever sees the raw value, so a leaked store snapshot
/// cannot redeem codes.
///
/// Lifecycle contract for [`crate::authorization_server::stores::CodeStore`]
/// implementors: `save` is called with `used: false`; `consume` flips
/// `used` (atomically, single-row).
#[derive(Clone, Debug)]
pub struct StoredAuthorizationCode {
    /// The client the code was issued to (checked at redemption).
    pub client_id: String,
    /// The exact redirect URI from the authorize request (checked at
    /// redemption).
    pub redirect_uri: String,
    /// The authenticated resource owner (the user).
    pub subject: String,
    /// Space-delimited granted scope.
    pub scope: String,
    /// Epoch seconds. Codes are short-lived (default 10 minutes).
    pub expires_at: i64,
    /// The S256 PKCE challenge from the authorize request, if PKCE
    /// was used. `None` only for confidential clients with PKCE
    /// explicitly not required.
    pub code_challenge: Option<String>,
    /// The refresh-token family this code seeds. Replay of the code
    /// revokes this whole family.
    pub family_id: String,
    /// Single-use flag; flipped by `consume`.
    pub used: bool,
}

/// A refresh token, stored **keyed by the SHA-256 hex of the token** —
/// same store-compromise posture as codes.
///
/// Refresh tokens **rotate** (RFC 6749 §6 + OAuth 2.1): every refresh
/// grant consumes the presented token and issues a successor in the
/// same `family_id`. Presenting an already-rotated (or revoked) token
/// is theft with high probability — the standard response is to revoke
/// the entire family, which is what this server does.
#[derive(Clone, Debug)]
pub struct StoredRefreshToken {
    pub client_id: String,
    /// The resource owner; empty for machine grants (refresh tokens
    /// are not issued for `client_credentials` anyway).
    pub subject: String,
    /// The scope this refresh token carries (originating grant's
    /// scope; refresh may narrow but never widen it).
    pub scope: String,
    /// The rotation family this token belongs to. A family = one
    /// authorization (one code) and its whole refresh chain.
    pub family_id: String,
    /// Epoch seconds.
    pub expires_at: i64,
    /// Flipped by `consume`; a `true` value presented again is the
    /// reuse signal.
    pub rotated: bool,
    /// Flipped by `revoke_family` (or a targeted revocation).
    pub revoked: bool,
    /// The `jti` of the access token this refresh token was issued
    /// alongside, so family revocation can also kill outstanding
    /// access JWTs (see [`crate::authorization_server::stores::RefreshTokenStore::revoke_family`]).
    pub access_jti: String,
    /// That access token's `exp` — bounds the revocation-list entry.
    pub access_exp: i64,
}

fn validate_redirect_uris(uris: Vec<String>) -> Result<Vec<String>, ClientRecordError> {
    for uri in &uris {
        let parsed = Url::parse(uri)
            .map_err(|_| ClientRecordError::InvalidRedirectUri { uri: uri.clone() })?;
        let ok = matches!(parsed.scheme(), "http" | "https")
            && parsed.host_str().is_some()
            && parsed.fragment().is_none();
        if !ok {
            return Err(ClientRecordError::InvalidRedirectUri { uri: uri.clone() });
        }
    }
    Ok(uris)
}

fn dedup(scopes: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(scopes.len());
    for s in scopes {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_secret_never_appears_in_debug() {
        let record = OAuth2ClientRecord::confidential(
            "web-app",
            "top-secret",
            vec!["https://a/cb".into()],
            vec![GrantType::ClientCredentials],
            vec!["read".into()],
        )
        .unwrap();
        let debug = format!("{record:?}");
        assert!(
            !debug.contains("top-secret"),
            "the secret must never appear"
        );
        assert!(
            !debug.contains(&sha256_hex(b"top-secret")),
            "the secret's digest must never appear either"
        );
    }

    #[test]
    fn check_secret_is_exact_and_not_membership() {
        let record = OAuth2ClientRecord::confidential(
            "web-app",
            "top-secret",
            vec![],
            vec![GrantType::ClientCredentials],
            vec![],
        )
        .unwrap();
        assert!(record.check_secret("top-secret"));
        assert!(!record.check_secret("top-secret2"));
        assert!(!record.check_secret(""));
        assert!(!record.check_secret("top-"));
    }

    #[test]
    fn redirect_uri_must_be_absolute_http_without_fragment() {
        assert!(OAuth2ClientRecord::public("spa", vec![], vec![], vec![]).is_ok());
        let err = OAuth2ClientRecord::public(
            "spa",
            vec!["javascript:alert(1)//example.com".into()],
            vec![],
            vec![],
        )
        .unwrap_err();
        assert!(matches!(err, ClientRecordError::InvalidRedirectUri { .. }));
        let err =
            OAuth2ClientRecord::public("spa", vec!["https://a/cb#frag".into()], vec![], vec![])
                .unwrap_err();
        assert!(matches!(err, ClientRecordError::InvalidRedirectUri { .. }));
    }

    #[test]
    fn scope_allowlist_is_membership() {
        let record = OAuth2ClientRecord::public(
            "spa",
            vec![],
            vec![],
            vec!["read".into(), "profile".into()],
        )
        .unwrap();
        assert!(record.allows_scopes(&[])); // an empty request is vacuously allowed
        assert!(record.allows_scopes(&["read".to_string()]));
        assert!(record.allows_scopes(&["read".to_string(), "profile".to_string()]));
        assert!(!record.allows_scopes(&["admin".to_string()]));
    }
}
