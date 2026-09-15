//! The authorization-server engine: issues tokens, enforces the grant
//! rules, runs the security-critical transitions (code single-use,
//! refresh rotation + reuse detection, family revocation).

use std::sync::Arc;

use axum::http::request::Parts;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use super::config::{AuthorizationServerConfig, AuthorizationServerConfigError};
use super::model::{GrantType, OAuth2ClientRecord, StoredAuthorizationCode, StoredRefreshToken};
use super::stores::{
    AccessTokenRevocationList, AuthorizationServerStores, ClientStore, CodeStore, ConsumeCode,
    ConsumeRefresh, RefreshTokenStore,
};
use super::{constant_time_eq, sha256_hex};

/// Resolves the authenticated resource owner (the user) for an
/// `/authorize` request: their subject identifier, or `None` when the
/// request is unauthenticated (the endpoint then responds 401 instead
/// of redirecting).
///
/// The closure sees the raw request parts, so a typical implementation
/// reads whatever session extension the host app's auth middleware
/// stashed — e.g. nestrs's `Principal`, a session cookie, or a bearer
/// token. The module ships no default because session shape is
/// app-specific by nature.
pub type ResourceOwnerSource = Arc<dyn Fn(&Parts) -> Option<String> + Send + Sync>;

/// Failure to build the server's key material (bad PEM→JWK/DecodingKey
/// conversion). Signing-key PEM validity itself was already checked by
/// [`AuthorizationServerConfig::new`].
#[derive(Debug, thiserror::Error)]
#[error("authorization server setup failed: {0}")]
pub struct AuthorizationServerSetupError(pub String);

impl From<AuthorizationServerConfigError> for AuthorizationServerSetupError {
    fn from(e: AuthorizationServerConfigError) -> Self {
        Self(e.to_string())
    }
}

/// Why a token request (or client authentication) failed. Maps onto
/// the RFC 6749 §5.2 error responses.
#[derive(Debug)]
pub enum TokenFailure {
    /// Client authentication failed → 401 `invalid_client`.
    InvalidClient { description: String },
    /// Malformed request → 400 `invalid_request`.
    InvalidRequest { description: String },
    /// Bad code / refresh token / verifier → 400 `invalid_grant`.
    InvalidGrant { description: String },
    /// Requested scope not allowed → 400 `invalid_scope`.
    InvalidScope { description: String },
    /// Client not allowed this grant → 400 `unauthorized_client`.
    UnauthorizedClient { description: String },
    /// Unknown grant → 400 `unsupported_grant_type`.
    UnsupportedGrantType { description: String },
    /// Token minting failed (key/signing trouble) → 500 `server_error`.
    ServerError { description: String },
}

impl TokenFailure {
    pub(crate) fn status(&self) -> u16 {
        match self {
            TokenFailure::InvalidClient { .. } => 401,
            TokenFailure::ServerError { .. } => 500,
            _ => 400,
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self {
            TokenFailure::InvalidClient { .. } => "invalid_client",
            TokenFailure::InvalidRequest { .. } => "invalid_request",
            TokenFailure::InvalidGrant { .. } => "invalid_grant",
            TokenFailure::InvalidScope { .. } => "invalid_scope",
            TokenFailure::UnauthorizedClient { .. } => "unauthorized_client",
            TokenFailure::UnsupportedGrantType { .. } => "unsupported_grant_type",
            TokenFailure::ServerError { .. } => "server_error",
        }
    }

    pub(crate) fn description(&self) -> &str {
        match self {
            TokenFailure::InvalidClient { description }
            | TokenFailure::InvalidRequest { description }
            | TokenFailure::InvalidGrant { description }
            | TokenFailure::InvalidScope { description }
            | TokenFailure::UnauthorizedClient { description }
            | TokenFailure::UnsupportedGrantType { description }
            | TokenFailure::ServerError { description } => description,
        }
    }
}

/// Why `/authorize` failed. The split matters for open-redirect safety:
/// while the client and redirect URI are still unverified, failures go
/// back **directly** (never redirect); once they are verified, RFC
/// 6749 §4.1.2.1 requires errors to travel back to the client **via
/// the redirect**.
#[derive(Debug)]
pub enum AuthorizeFailure {
    /// Respond directly — do NOT redirect (unknown client, bad
    /// redirect URI, or no authenticated resource owner).
    Direct {
        status: u16,
        error: &'static str,
        description: String,
    },
    /// Redirect back to the client with the OAuth error appended.
    Redirect {
        redirect_uri: String,
        error: &'static str,
        description: String,
        state: Option<String>,
    },
}

impl AuthorizeFailure {
    fn direct(status: u16, error: &'static str, description: impl Into<String>) -> Self {
        AuthorizeFailure::Direct {
            status,
            error,
            description: description.into(),
        }
    }

    fn redirect(
        redirect_uri: &str,
        error: &'static str,
        description: impl Into<String>,
        state: Option<String>,
    ) -> Self {
        AuthorizeFailure::Redirect {
            redirect_uri: redirect_uri.to_string(),
            error,
            description: description.into(),
            state,
        }
    }
}

/// The raw `/authorize` query parameters + the resolved resource-owner
/// subject. Query parsing happens in the route layer; the semantics
/// live here.
#[derive(Debug, Default)]
pub(crate) struct AuthorizeParams {
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub response_type: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    /// Resolved by the [`ResourceOwnerSource`] in the route.
    pub subject: Option<String>,
}

/// The raw `/token` form fields (client credentials are authenticated
/// separately, before the grant dispatch).
#[derive(Debug, Default)]
pub(crate) struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// Tokens issued by a successful grant.
pub struct IssuedTokens {
    /// The EdDSA-signed access JWT (header carries the configured
    /// `kid`; claims: `iss`, `sub`, `aud`, `exp`, `iat`, `jti`,
    /// `scope`, `client_id`).
    pub access_token: String,
    /// Seconds until the access token expires.
    pub expires_in: i64,
    /// Set for `authorization_code` and `refresh_token` grants
    /// (never for `client_credentials`, per RFC 6749 §4.4.3).
    pub refresh_token: Option<String>,
    /// The granted scope (space-delimited).
    pub scope: String,
}

// Never let a token reach a Debug/trace line.
impl std::fmt::Debug for IssuedTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedTokens")
            .field("access_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .field("scope", &self.scope)
            .finish()
    }
}

#[derive(Serialize)]
struct AccessTokenClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    exp: i64,
    iat: i64,
    jti: &'a str,
    scope: &'a str,
    client_id: &'a str,
}

/// The authorization-server engine. Held behind an `Arc` in the route
/// state; construct via [`super::OAuth2AuthorizationServerModule::register`]
/// or directly when mounting [`super::routes::router`] into an existing
/// axum app.
pub struct AuthorizationServer {
    config: AuthorizationServerConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    jwks_document: Value,
    clients: Arc<dyn ClientStore>,
    codes: Arc<dyn CodeStore>,
    refresh: Arc<dyn RefreshTokenStore>,
    revocations: Arc<dyn AccessTokenRevocationList>,
    resource_owner: ResourceOwnerSource,
}

impl AuthorizationServer {
    /// Build the server: validates the config, derives the
    /// sign/verify key pair, and pre-renders the JWKS document.
    pub fn new(
        config: AuthorizationServerConfig,
        stores: AuthorizationServerStores,
        resource_owner: ResourceOwnerSource,
    ) -> Result<Self, AuthorizationServerSetupError> {
        config.validate()?;

        let encoding_key = EncodingKey::from_ed_pem(config.signing_key_pem().as_bytes())
            .map_err(|e| AuthorizationServerSetupError(format!("signing key: {e}")))?;

        let verifying = config.signing_key().verifying_key();
        let jwk = json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(verifying.to_bytes()),
            "kid": config.kid,
            "use": "sig",
            "alg": "EdDSA",
        });
        let jwk_parsed: jsonwebtoken::jwk::Jwk = serde_json::from_value(jwk.clone())
            .map_err(|e| AuthorizationServerSetupError(format!("JWK build: {e}")))?;
        let decoding_key = DecodingKey::from_jwk(&jwk_parsed)
            .map_err(|e| AuthorizationServerSetupError(format!("decoding key: {e}")))?;

        Ok(Self {
            config,
            encoding_key,
            decoding_key,
            jwks_document: json!({ "keys": [jwk] }),
            clients: stores.clients,
            codes: stores.codes,
            refresh: stores.refresh_tokens,
            revocations: stores.revocations,
            resource_owner,
        })
    }

    pub(crate) fn config(&self) -> &AuthorizationServerConfig {
        &self.config
    }

    pub(crate) fn resource_owner(&self) -> &ResourceOwnerSource {
        &self.resource_owner
    }

    /// The served JWKS document (single Ed25519 verification key).
    pub fn jwks_document(&self) -> &Value {
        &self.jwks_document
    }

    /// The RFC 8414 authorization-server metadata document.
    pub fn discovery_document(&self) -> Value {
        let issuer = &self.config.issuer;
        let prefix = &self.config.endpoint_prefix;
        let mut doc = json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}{prefix}/authorize"),
            "token_endpoint": format!("{issuer}{prefix}/token"),
            "revocation_endpoint": format!("{issuer}{prefix}/revoke"),
            "introspection_endpoint": format!("{issuer}{prefix}/introspect"),
            "jwks_uri": format!("{issuer}{}", self.config.jwks_path),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials"],
            "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
            "code_challenge_methods_supported": ["S256"],
        });
        if !self.config.scopes_supported.is_empty() {
            doc["scopes_supported"] = json!(&self.config.scopes_supported);
        }
        doc
    }

    /// Client authentication for token / introspection / revocation
    /// requests. The route layer has already extracted the credentials
    /// from Basic auth and/or form fields; this is the shared
    /// verification (public clients present no secret, confidential
    /// secrets are compared constant-time over SHA-256 digests).
    pub(crate) async fn authenticate_client(
        &self,
        client_id: Option<&str>,
        secret: Option<&str>,
    ) -> Result<OAuth2ClientRecord, TokenFailure> {
        let client_id = client_id
            .filter(|s| !s.trim().is_empty())
            .ok_or(TokenFailure::InvalidClient {
                description: "client_id is required".to_string(),
            })?;
        let record = self
            .clients
            .find(client_id)
            .await
            .ok_or(TokenFailure::InvalidClient {
                description: "unknown client".to_string(),
            })?;
        if record.is_public {
            if secret.is_some_and(|s| !s.trim().is_empty()) {
                return Err(TokenFailure::InvalidClient {
                    description: "public clients do not authenticate with a secret".to_string(),
                });
            }
        } else {
            let presented = secret
                .filter(|s| !s.trim().is_empty())
                .ok_or(TokenFailure::InvalidClient {
                    description: "client authentication (secret) is required".to_string(),
                })?;
            if !record.check_secret(presented) {
                return Err(TokenFailure::InvalidClient {
                    description: "invalid client credentials".to_string(),
                });
            }
        }
        Ok(record)
    }

    /// The `/authorize` transaction (RFC 6749 §4.1.1 + RFC 7636 PKCE).
    /// Returns the redirect URL carrying `code` + `state` on success.
    pub(crate) async fn authorize(
        &self,
        params: &AuthorizeParams,
    ) -> Result<Url, AuthorizeFailure> {
        let client_id = params
            .client_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AuthorizeFailure::direct(400, "invalid_request", "client_id is required")
            })?;
        let client = self
            .clients
            .find(client_id)
            .await
            .ok_or_else(|| {
                AuthorizeFailure::direct(400, "invalid_client", "unknown client")
            })?;

        // Open-redirect guard: never redirect anywhere until both the
        // client and its redirect URI are verified. Exact-match against
        // the registration; the single-registration shorthand (redirect
        // param omitted) is the only allowed default.
        let redirect_uri = match &params.redirect_uri {
            Some(uri) if client.has_redirect(uri) => uri.clone(),
            None if client.redirect_uris().len() == 1 => client.redirect_uris()[0].clone(),
            _ => {
                return Err(AuthorizeFailure::direct(
                    400,
                    "invalid_request",
                    "redirect_uri is missing or not registered for this client",
                ))
            }
        };
        let fail = |error: &'static str, description: &str| {
            AuthorizeFailure::redirect(&redirect_uri, error, description, params.state.clone())
        };

        let subject = params
            .subject
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AuthorizeFailure::direct(
                    401,
                    "access_denied",
                    "no authenticated resource owner — authenticate the user first",
                )
            })?;

        if params.response_type.as_deref() != Some("code") {
            return Err(fail(
                "unsupported_response_type",
                "only response_type=code is supported",
            ));
        }
        if !client.allows_grant(GrantType::AuthorizationCode) {
            return Err(fail(
                "unauthorized_client",
                "client is not allowed the authorization_code grant",
            ));
        }

        let granted_scope = match &params.scope {
            Some(scope) => {
                let requested = parse_scope(scope);
                if !client.allows_scopes(&requested) {
                    return Err(fail(
                        "invalid_scope",
                        "requested scope exceeds the client's allowed scopes",
                    ));
                }
                requested.join(" ")
            }
            None => client.allowed_scopes().join(" "),
        };

        // PKCE (RFC 7636). Only S256 is supported — `plain` is dead on
        // arrival by design. Public clients always require it; so do
        // confidential clients unless explicitly relaxed in config.
        match (&params.code_challenge, params.code_challenge_method.as_deref()) {
            (Some(challenge), Some("S256")) if is_valid_challenge(challenge) => {}
            (Some(_), _) => {
                return Err(fail(
                    "invalid_request",
                    "code_challenge requires code_challenge_method=S256 (43 chars, base64url)",
                ))
            }
            (None, _) => {
                let pkce_required = client.is_public || self.config.require_pkce_for_confidential;
                if pkce_required {
                    return Err(fail(
                        "invalid_request",
                        "PKCE (code_challenge with method S256) is required for this client",
                    ));
                }
            }
        }

        let now = now_epoch();
        let code = random_string(32);
        let expires_at = now + self.config.authorization_code_ttl.as_secs() as i64;
        let family_id = random_string(16);
        self.codes
            .save(
                &sha256_hex(code.as_bytes()),
                StoredAuthorizationCode {
                    client_id: client.client_id.clone(),
                    redirect_uri: redirect_uri.clone(),
                    subject: subject.to_string(),
                    scope: granted_scope.clone(),
                    expires_at,
                    code_challenge: params.code_challenge.clone(),
                    family_id: family_id.clone(),
                    used: false,
                },
            )
            .await;

        let mut url = Url::parse(&redirect_uri)
            .expect("registered redirect URIs are validated at registration");
        url.query_pairs_mut().append_pair("code", &code);
        if let Some(state) = &params.state {
            url.query_pairs_mut().append_pair("state", state);
        }
        Ok(url)
    }

    /// The `/token` transaction, after client authentication succeeded.
    pub(crate) async fn token(
        &self,
        request: &TokenRequest,
        client: &OAuth2ClientRecord,
    ) -> Result<IssuedTokens, TokenFailure> {
        match GrantType::parse(&request.grant_type) {
            Some(GrantType::AuthorizationCode) => {
                self.authorization_code_grant(request, client).await
            }
            Some(GrantType::RefreshToken) => self.refresh_token_grant(request, client).await,
            Some(GrantType::ClientCredentials) => {
                self.client_credentials_grant(request, client).await
            }
            None => Err(TokenFailure::UnsupportedGrantType {
                description: "grant_type must be one of: authorization_code, refresh_token, client_credentials".to_string(),
            }),
        }
    }

    async fn authorization_code_grant(
        &self,
        request: &TokenRequest,
        client: &OAuth2ClientRecord,
    ) -> Result<IssuedTokens, TokenFailure> {
        let invalid_grant = |description: &str| TokenFailure::InvalidGrant {
            description: description.to_string(),
        };

        let code = request
            .code
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(TokenFailure::InvalidRequest {
                description: "code is required for the authorization_code grant".to_string(),
            })?;

        // Consume BEFORE any validation that can fail on attacker
        // input: a single failed redemption attempt burns the code,
        // so it cannot be probed repeatedly.
        let record = match self.codes.consume(&sha256_hex(code.as_bytes())).await {
            ConsumeCode::Fresh(record) => record,
            ConsumeCode::Reused { family_id } => {
                // Replay. The only way a used code is presented twice is
                // interception — kill the family it seeded.
                self.revoke_family(&family_id).await;
                return Err(invalid_grant("authorization code is unknown, expired, or already used"));
            }
            ConsumeCode::Missing => {
                return Err(invalid_grant("authorization code is unknown, expired, or already used"))
            }
        };

        if record.client_id != client.client_id {
            return Err(invalid_grant("authorization code was issued to a different client"));
        }
        if record.expires_at <= now_epoch() {
            return Err(invalid_grant("authorization code has expired"));
        }
        let redirect_uri = request
            .redirect_uri
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(TokenFailure::InvalidRequest {
                description: "redirect_uri is required".to_string(),
            })?;
        if redirect_uri != record.redirect_uri {
            return Err(invalid_grant("redirect_uri does not match the authorization request"));
        }

        // PKCE: when the code was minted with a challenge, the verifier
        // must match it — SHA-256(verifier) base64url, constant-time.
        if let Some(challenge) = &record.code_challenge {
            let verifier = request
                .code_verifier
                .as_deref()
                .filter(|v| is_valid_verifier(v))
                .ok_or(invalid_grant(
                    "code_verifier is required (43-128 chars, unreserved characters)",
                ))?;
            let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            if !constant_time_eq(computed.as_bytes(), challenge.as_bytes()) {
                return Err(invalid_grant("code_verifier does not match the code_challenge"));
            }
        }

        let now = now_epoch();
        let (access_token, jti, access_exp) = self.mint_access_token(
            &record.subject,
            client.client_id.as_str(),
            &record.scope,
            &client.client_id,
        )?;
        let refresh_token = random_string(32);
        let refresh_exp = now + self.config.refresh_token_ttl.as_secs() as i64;
        self.refresh
            .save(
                &sha256_hex(refresh_token.as_bytes()),
                StoredRefreshToken {
                    client_id: client.client_id.clone(),
                    subject: record.subject.clone(),
                    scope: record.scope.clone(),
                    family_id: record.family_id.clone(),
                    expires_at: refresh_exp,
                    rotated: false,
                    revoked: false,
                    access_jti: jti,
                    access_exp,
                },
            )
            .await;

        Ok(IssuedTokens {
            access_token,
            expires_in: access_exp - now,
            refresh_token: Some(refresh_token),
            scope: record.scope,
        })
    }

    async fn refresh_token_grant(
        &self,
        request: &TokenRequest,
        client: &OAuth2ClientRecord,
    ) -> Result<IssuedTokens, TokenFailure> {
        let invalid_grant = |description: &str| TokenFailure::InvalidGrant {
            description: description.to_string(),
        };

        let presented = request
            .refresh_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(TokenFailure::InvalidRequest {
                description: "refresh_token is required for the refresh_token grant".to_string(),
            })?;

        if !client.allows_grant(GrantType::RefreshToken) {
            return Err(TokenFailure::UnauthorizedClient {
                description: "client is not allowed the refresh_token grant".to_string(),
            });
        }

        let record = match self.refresh.consume(&sha256_hex(presented.as_bytes())).await {
            ConsumeRefresh::Fresh(record) => record,
            ConsumeRefresh::Reused { family_id } => {
                // Reuse of a rotated/revoked token: theft with high
                // probability. Revoke the whole family — every refresh
                // token in the chain AND their access JWTs.
                self.revoke_family(&family_id).await;
                return Err(invalid_grant("refresh token is invalid, expired, or revoked"));
            }
            ConsumeRefresh::Missing => {
                return Err(invalid_grant("refresh token is invalid, expired, or revoked"))
            }
        };

        if record.client_id != client.client_id {
            // A refresh token crossing client boundaries is hostile —
            // burn the family too.
            self.revoke_family(&record.family_id).await;
            return Err(invalid_grant("refresh token was issued to a different client"));
        }
        if record.revoked {
            return Err(invalid_grant("refresh token is invalid, expired, or revoked"));
        }
        if record.expires_at <= now_epoch() {
            return Err(invalid_grant("refresh token has expired"));
        }

        // RFC 6749 §6: refresh may narrow the scope, never widen it.
        let granted_scope = match request.scope.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(scope) => {
                let requested = parse_scope(scope);
                let original: Vec<String> = parse_scope(&record.scope);
                if !requested.iter().all(|s| original.contains(s)) {
                    return Err(TokenFailure::InvalidScope {
                        description: "requested scope exceeds the originally granted scope"
                            .to_string(),
                    });
                }
                requested.join(" ")
            }
            None => record.scope.clone(),
        };

        let now = now_epoch();
        let (access_token, jti, access_exp) = self.mint_access_token(
            &record.subject,
            client.client_id.as_str(),
            &granted_scope,
            &client.client_id,
        )?;
        let successor = random_string(32);
        let refresh_exp = now + self.config.refresh_token_ttl.as_secs() as i64;
        self.refresh
            .save(
                &sha256_hex(successor.as_bytes()),
                StoredRefreshToken {
                    client_id: client.client_id.clone(),
                    subject: record.subject,
                    scope: granted_scope.clone(),
                    family_id: record.family_id.clone(),
                    expires_at: refresh_exp,
                    rotated: false,
                    revoked: false,
                    access_jti: jti,
                    access_exp,
                },
            )
            .await;

        Ok(IssuedTokens {
            access_token,
            expires_in: access_exp - now,
            refresh_token: Some(successor),
            scope: granted_scope,
        })
    }

    async fn client_credentials_grant(
        &self,
        request: &TokenRequest,
        client: &OAuth2ClientRecord,
    ) -> Result<IssuedTokens, TokenFailure> {
        if client.is_public {
            return Err(TokenFailure::UnauthorizedClient {
                description: "client_credentials requires a confidential client".to_string(),
            });
        }
        if !client.allows_grant(GrantType::ClientCredentials) {
            return Err(TokenFailure::UnauthorizedClient {
                description: "client is not allowed the client_credentials grant".to_string(),
            });
        }

        let granted_scope = match request.scope.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(scope) => {
                let requested = parse_scope(scope);
                if !client.allows_scopes(&requested) {
                    return Err(TokenFailure::InvalidScope {
                        description: "requested scope exceeds the client's allowed scopes"
                            .to_string(),
                    });
                }
                requested.join(" ")
            }
            None => client.allowed_scopes().join(" "),
        };

        let now = now_epoch();
        // The client is the subject and its own audience.
        let (access_token, _, access_exp) = self.mint_access_token(
            client.client_id.as_str(),
            client.client_id.as_str(),
            &granted_scope,
            &client.client_id,
        )?;

        // RFC 6749 §4.4.3: no refresh token for client_credentials.
        Ok(IssuedTokens {
            access_token,
            expires_in: access_exp - now,
            refresh_token: None,
            scope: granted_scope,
        })
    }

    /// RFC 7662 introspection. Returns the response body — `active:
    /// false` for anything unverifiable; never an error status for a
    /// bad token (only client-auth failures, handled by the route).
    pub(crate) async fn introspect(&self, token: &str) -> Value {
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = 0;
        // No single expected audience here: tokens are issued for
        // client-specific audiences and `aud` is echoed back as a claim
        // (RFC 7662 §2.2), not validated against one configured value.
        // Authenticity is the signature check against our own key.
        validation.validate_aud = false;
        // Access-token (JWT) path: verify with our own key. Issuer is
        // implicit — decoding against our key IS the issuer check.
        match jsonwebtoken::decode::<Value>(token, &self.decoding_key, &validation) {
            Ok(data) => {
                let claims = &data.claims;
                let jti = claims.get("jti").and_then(Value::as_str).unwrap_or("");
                if !jti.is_empty() && self.revocations.is_revoked(jti).await {
                    return json!({ "active": false });
                }
                json!({
                    "active": true,
                    "token_type": "Bearer",
                    "scope": claims.get("scope"),
                    "client_id": claims.get("client_id"),
                    "sub": claims.get("sub"),
                    "aud": claims.get("aud"),
                    "exp": claims.get("exp"),
                    "iat": claims.get("iat"),
                    "iss": claims.get("iss"),
                    "jti": claims.get("jti"),
                })
            }
            Err(_) => {
                // Refresh-token path: opaque value, hashed lookup.
                match self.refresh.find(&sha256_hex(token.as_bytes())).await {
                    Some(record)
                        if !record.rotated
                            && !record.revoked
                            && record.expires_at > now_epoch() =>
                    {
                        let mut body = json!({
                            "active": true,
                            "token_type": "refresh_token",
                            "scope": record.scope,
                            "client_id": record.client_id,
                            "exp": record.expires_at,
                            "iss": self.config.issuer,
                            "jti": record.access_jti,
                        });
                        if !record.subject.is_empty() {
                            body["sub"] = json!(record.subject);
                        }
                        body
                    }
                    _ => json!({ "active": false }),
                }
            }
        }
    }

    /// RFC 7009 revocation. Token type is auto-detected (`token_type_hint`
    /// is advisory); an unrecognized token still yields success — the
    /// response conveys "nothing more to do", not "error" (§2.2).
    pub(crate) async fn revoke(&self, token: &str) {
        // Access JWT: record its jti until the token would have
        // expired. Signature still verified (don't grow the revocation
        // list on attacker garbage), expiry not (a token about to die
        // is still worth revoking). No audience check for the same
        // reason as introspection: the `aud` is client-specific data,
        // not something this endpoint validates against.
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = 0;
        validation.validate_exp = false;
        validation.validate_aud = false;
        if let Ok(data) = jsonwebtoken::decode::<Value>(token, &self.decoding_key, &validation) {
            if let (Some(jti), Some(exp)) = (
                data.claims.get("jti").and_then(Value::as_str),
                data.claims.get("exp").and_then(Value::as_i64),
            ) {
                if !jti.is_empty() {
                    self.revocations.revoke(jti, exp).await;
                }
            }
        }

        // Refresh token: revoking one revokes its whole family (chain
        // + the access JWTs minted alongside them).
        if let Some(record) = self.refresh.find(&sha256_hex(token.as_bytes())).await {
            self.revoke_family(&record.family_id).await;
        }
    }

    /// Revoke a refresh family: the store flips every member, and the
    /// `access_jti`s it reports go onto the revocation list so
    /// outstanding access JWTs die with the chain.
    async fn revoke_family(&self, family_id: &str) {
        for record in self.refresh.revoke_family(family_id).await {
            self.revocations.revoke(&record.access_jti, record.access_exp).await;
        }
    }

    fn mint_access_token(
        &self,
        subject: &str,
        audience: &str,
        scope: &str,
        client_id: &str,
    ) -> Result<(String, String, i64), TokenFailure> {
        let now = now_epoch();
        let exp = now + self.config.access_token_ttl.as_secs() as i64;
        let jti = random_string(16);
        let claims = AccessTokenClaims {
            iss: &self.config.issuer,
            sub: subject,
            aud: audience,
            exp,
            iat: now,
            jti: &jti,
            scope,
            client_id,
        };
        let header = Header {
            alg: Algorithm::EdDSA,
            kid: Some(self.config.kid.to_string()),
            ..Default::default()
        };
        let token = jsonwebtoken::encode(&header, &claims, &self.encoding_key).map_err(|e| {
            TokenFailure::ServerError {
                description: format!("failed to sign the access token: {e}"),
            }
        })?;
        Ok((token, jti, exp))
    }
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn random_string(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn parse_scope(scope: &str) -> Vec<String> {
    scope.split_whitespace().map(str::to_string).collect()
}

/// S256 challenge: base64url, exactly 43 characters (SHA-256 of the
/// verifier, unpadded) — per RFC 7636 §4.2.
fn is_valid_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// PKCE verifier: 43-128 characters of `[A-Za-z0-9-._~]` per RFC 7636
/// §4.1.
fn is_valid_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
        })
}
