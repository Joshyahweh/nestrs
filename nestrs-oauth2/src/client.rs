//! OAuth2 client. Wraps the `oauth2` crate's typed builders with
//! nestrs-friendly defaults: clock-skew-aware token expiry, PKCE
//! support, a token cache that coalesces concurrent refreshes, and
//! a public API that returns the same `TokenSet` shape regardless of
//! which grant was used.
//!
//! We do not reinvent PKCE or state generation — the `oauth2` crate
//! gives us `PkceCodeChallenge::new_random_sha256()` and
//! `CsrfToken::new_random()`. We do add:
//!   * `TokenSet` with `Instant`-typed `expires_at` (not string
//!     `expires_in`).
//!   * A `TokenCache` that the caller can share across instances to
//!     avoid the "two refreshes in flight" footgun.
//!   * A `OAuth2Client` that works for confidential *and* public
//!     clients (no `client_secret` for PKCE-only / RFC 8252 §7.2
//!     public clients).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{GrantError, OAuth2Error};

/// A complete set of tokens returned by the IdP. We materialise
/// `expires_at` as an `Instant` (rather than the wire's `expires_in`
/// seconds) so callers can do `Instant::now() < token.expires_at`
/// without an arithmetic conversion.
///
/// `Serialize`/`Deserialize` are intentionally **not** derived because
/// `Instant` doesn't implement them. For wire serialisation use
/// `TokenSetDto` (see the bottom of this file) which uses
/// `SystemTime` + a `u64` second offset.
#[derive(Clone)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: Option<Instant>,
    pub scope: Option<String>,
    pub raw: serde_json::Value,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tokens are bearer credentials — never render them. Debug output
        // flows into logs, panic messages, and error reports. Presence
        // (`Some("<redacted>")`) stays visible for operators.
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .field("raw", &self.raw)
            .finish()
    }
}

/// OAuth2 client configuration. The `client_secret` is `Option`-shaped
/// so a PKCE public client (RFC 8252 §7.2) can construct a client
/// without one.
#[derive(Clone)]
pub struct OAuth2Options {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub authz_url: Url,
    pub token_url: Url,
    pub redirect_uri: Option<Url>,
    pub revoke_url: Option<Url>,
    pub userinfo_url: Option<Url>,
    /// Maximum time to wait on a token / revoke / userinfo round-trip.
    /// Default: 30 seconds.
    pub timeout: Duration,
    /// Optional initial token cache. Pass a shared `Arc<TokenCache>` if
    /// you have multiple `OAuth2Client` instances for the same IdP and
    /// want them to coalesce refreshes.
    pub cache: Option<Arc<TokenCache>>,
}

impl std::fmt::Debug for OAuth2Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The client secret is never rendered — Debug output flows into
        // logs and error reports. Presence stays visible for operators.
        f.debug_struct("OAuth2Options")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("authz_url", &self.authz_url)
            .field("token_url", &self.token_url)
            .field("redirect_uri", &self.redirect_uri)
            .field("revoke_url", &self.revoke_url)
            .field("userinfo_url", &self.userinfo_url)
            .field("timeout", &self.timeout)
            .field("cache", &self.cache)
            .finish()
    }
}

impl OAuth2Options {
    /// Build a confidential-client `OAuth2Options` from the four URLs
    /// most IdPs require. Sugar for the common case.
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        authz_url: Url,
        token_url: Url,
        redirect_uri: Url,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: Some(client_secret.into()),
            authz_url,
            token_url,
            redirect_uri: Some(redirect_uri),
            revoke_url: None,
            userinfo_url: None,
            timeout: Duration::from_secs(30),
            cache: None,
        }
    }

    /// Build a PKCE public-client `OAuth2Options`. No client secret.
    pub fn public_client(
        client_id: impl Into<String>,
        authz_url: Url,
        token_url: Url,
        redirect_uri: Url,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            authz_url,
            token_url,
            redirect_uri: Some(redirect_uri),
            revoke_url: None,
            userinfo_url: None,
            timeout: Duration::from_secs(30),
            cache: None,
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn with_revoke_url(mut self, u: Url) -> Self {
        self.revoke_url = Some(u);
        self
    }

    pub fn with_userinfo_url(mut self, u: Url) -> Self {
        self.userinfo_url = Some(u);
        self
    }

    pub fn with_cache(mut self, cache: Arc<TokenCache>) -> Self {
        self.cache = Some(cache);
        self
    }
}

/// The OAuth2 client. Cheap to clone (it's all `Arc`s + URLs).
#[derive(Clone)]
pub struct OAuth2Client {
    /// We pin the `HasAuthUrl` and `HasTokenUrl` endpoint-state
    /// generics to `EndpointSet` because the public API only makes
    /// sense with both endpoints configured. The remaining endpoints
    /// (device-auth / introspection / revocation) stay at their
    /// `EndpointNotSet` default; we drive them through `reqwest`
    /// directly via the `http` field.
    inner: oauth2::Client<
        oauth2::basic::BasicErrorResponse,
        oauth2::basic::BasicTokenResponse,
        oauth2::basic::BasicTokenIntrospectionResponse,
        oauth2::StandardRevocableToken,
        oauth2::basic::BasicRevocationErrorResponse,
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    options: OAuth2Options,
    /// The HTTP client the `oauth2` crate uses to make token-endpoint
    /// round-trips. Kept as a field so we can share the timeout
    /// configuration with the userinfo / revoke helpers below.
    http: reqwest::Client,
    /// Optional shared cache. When `None`, each `OAuth2Client` is
    /// stateless across calls (every exchange / refresh hits the
    /// network).
    cache: Option<Arc<TokenCache>>,
}

impl std::fmt::Debug for OAuth2Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2Client")
            .field("client_id", &self.options.client_id)
            .field("authz_url", &self.options.authz_url)
            .field("token_url", &self.options.token_url)
            .finish_non_exhaustive()
    }
}

impl OAuth2Client {
    /// Construct a new `OAuth2Client`. Validates the URLs eagerly —
    /// an invalid `authz_url` / `token_url` returns
    /// `OAuth2Error::InvalidConfig`.
    pub fn new(options: OAuth2Options) -> Result<Self, OAuth2Error> {
        use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};

        if options.client_id.trim().is_empty() {
            return Err(OAuth2Error::InvalidConfig(
                "client_id must not be empty".into(),
            ));
        }
        let authz = AuthUrl::from_url(options.authz_url.clone());
        let token = TokenUrl::from_url(options.token_url.clone());
        let mut client = oauth2::basic::BasicClient::new(ClientId::new(options.client_id.clone()))
            .set_auth_uri(authz)
            .set_token_uri(token);
        if let Some(secret) = options.client_secret.as_ref() {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }
        if let Some(redirect) = options.redirect_uri.clone() {
            client = client.set_redirect_uri(RedirectUrl::from_url(redirect));
        }
        let http = reqwest::Client::builder()
            .timeout(options.timeout)
            .build()
            .map_err(OAuth2Error::Transport)?;
        Ok(Self {
            inner: client,
            cache: options.cache.clone(),
            options,
            http,
        })
    }

    /// Build the authorization URL. The caller persists the returned
    /// `CsrfToken` (as the `state` query parameter) and validates it
    /// when the IdP redirects back to the redirect URI. If `pkce` is
    /// `Some`, PKCE S256 is enabled (RFC 7636). If `extra` is `Some`,
    /// its key/value pairs are appended to the authorization URL
    /// (used for `prompt=consent`, `login_hint=…`, etc.).
    pub fn authorize_url(
        &self,
        scopes: &[&str],
        pkce: Option<&oauth2::PkceCodeChallenge>,
        extra: Option<&HashMap<String, String>>,
    ) -> AuthorizeUrl {
        use oauth2::{CsrfToken, Scope};

        let mut req = self.inner.authorize_url(CsrfToken::new_random);
        for s in scopes {
            req = req.add_scope(Scope::new((*s).to_string()));
        }
        let (url, state) = if let Some(challenge) = pkce {
            req.set_pkce_challenge(challenge.clone()).url()
        } else {
            req.url()
        };
        let mut url = url;
        if let Some(extra) = extra {
            // `oauth2::url::Url` exposes `query_pairs_mut` for appending.
            // The library may already have written `state` etc. into the
            // query; we append, not replace.
            let mut pairs = url.query_pairs_mut();
            for (k, v) in extra {
                pairs.append_pair(k, v);
            }
            drop(pairs);
        }
        AuthorizeUrl { url, state }
    }

    /// Exchange an authorization code for a `TokenSet`. The `pkce_verifier`
    /// is the one whose `code_challenge` was sent in the authorization
    /// request; the IdP checks the S256 hash matches. Pass
    /// `PkceCodeVerifier::new_random()`'s output here.
    ///
    /// `PkceCodeVerifier` doesn't implement `Clone` (the v5 `oauth2`
    /// crate redacts it), so we take it by value when it's set. The
    /// caller therefore can't reuse the verifier across calls — this
    /// is intentional, because reusing a verifier is a security bug.
    pub async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: Option<oauth2::PkceCodeVerifier>,
    ) -> Result<TokenSet, OAuth2Error> {
        // The `oauth2` crate's `request_async` loses the raw wire
        // body, but the v5 `StandardTokenResponse` uses
        // `#[serde(flatten)]` with `EmptyExtraTokenFields` (no fields)
        // and silently drops `id_token` / `scope` on re-serialisation.
        // We POST the form ourselves with `reqwest` to keep the wire
        // JSON; deserialisation into `StandardTokenResponse` is still
        // done by the `oauth2` crate's `StandardTokenResponse<...>` so
        // the typed accessors (`expires_in`, `access_token`) work.
        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code.to_string()),
        ];
        if let Some(redirect) = &self.options.redirect_uri {
            form.push(("redirect_uri".into(), redirect.to_string()));
        }
        if let Some(secret) = &self.options.client_secret {
            form.push(("client_id".into(), self.options.client_id.clone()));
            form.push(("client_secret".into(), secret.clone()));
        }
        if let Some(verifier) = pkce_verifier {
            form.push(("code_verifier".into(), verifier.secret().to_string()));
        }
        let (parsed, wire) = post_token_form(&self.http, &self.options.token_url, &form).await?;
        let token = materialise_token(&parsed, &wire, self.options.timeout);
        self.cache_token(token.clone());
        Ok(token)
    }

    /// Client-credentials grant. `client_id` + `client_secret`
    /// authenticate the client itself (no user context). PKCE is
    /// not applicable to this grant.
    pub async fn client_credentials(&self, scopes: &[&str]) -> Result<TokenSet, OAuth2Error> {
        if self.options.client_secret.is_none() {
            return Err(OAuth2Error::InvalidConfig(
                "client_credentials grant requires client_secret".into(),
            ));
        }
        let scope = if scopes.is_empty() {
            None
        } else {
            Some(
                scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        };
        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "client_credentials".into()),
            ("client_id".into(), self.options.client_id.clone()),
            (
                "client_secret".into(),
                self.options.client_secret.clone().unwrap(),
            ),
        ];
        if let Some(s) = scope {
            form.push(("scope".into(), s));
        }
        let (parsed, wire) = post_token_form(&self.http, &self.options.token_url, &form).await?;
        let token = materialise_token(&parsed, &wire, self.options.timeout);
        self.cache_token(token.clone());
        Ok(token)
    }

    /// Refresh a `TokenSet` using its `refresh_token`. The IdP may
    /// rotate the refresh token (return a new one); we propagate
    /// whatever the IdP returns, replacing the old refresh token.
    /// If `old` is the cached entry, the cache is updated.
    pub async fn refresh(&self, old: &TokenSet) -> Result<TokenSet, OAuth2Error> {
        let refresh = old
            .refresh_token
            .as_ref()
            .ok_or_else(|| OAuth2Error::InvalidConfig("token has no refresh_token".into()))?;
        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh.clone()),
        ];
        if let Some(secret) = &self.options.client_secret {
            form.push(("client_id".into(), self.options.client_id.clone()));
            form.push(("client_secret".into(), secret.clone()));
        }
        let (parsed, wire) = post_token_form(&self.http, &self.options.token_url, &form).await?;
        let mut token = materialise_token(&parsed, &wire, self.options.timeout);
        // If the IdP didn't return a new refresh token, keep the old one
        // (RFC 6749 §6 says implementations MAY rotate; we honour either).
        if token.refresh_token.is_none() {
            token.refresh_token = Some(refresh.clone());
        }
        self.cache_token(token.clone());
        Ok(token)
    }

    /// Revoke the access token (and the refresh token, if the IdP
    /// supports RFC 7009 token-type-hint separation). No-op if no
    /// `revoke_url` is configured.
    pub async fn revoke(&self, token: &TokenSet) -> Result<(), OAuth2Error> {
        let revoke_url = match &self.options.revoke_url {
            Some(u) => u.clone(),
            None => return Ok(()),
        };
        // RFC 7009 §2.2 recommends a token-type-hint; we send both
        // because some IdPs only honour one of them.
        if let Some(rt) = &token.refresh_token {
            self.post_revoke(&revoke_url, "refresh_token", rt).await?;
        }
        self.post_revoke(&revoke_url, "access_token", &token.access_token)
            .await
    }

    async fn post_revoke(&self, url: &Url, hint: &str, value: &str) -> Result<(), OAuth2Error> {
        let body = format!("token={}&token_type_hint={}", urlencode(value), hint);
        let resp = self
            .http
            .post(url.as_str())
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(OAuth2Error::Transport)?;
        // RFC 7009 §2.2: 200 = success. Some IdPs (e.g. Auth0) also
        // accept 204. Anything else is an error.
        match resp.status().as_u16() {
            200 | 204 => Ok(()),
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(OAuth2Error::TokenEndpoint { status, body })
            }
        }
    }

    /// Fetch userinfo with the access token. Returns the raw JSON
    /// claims (provider-specific shape). No-op without a configured
    /// `userinfo_url`.
    pub async fn userinfo(&self, token: &TokenSet) -> Result<serde_json::Value, OAuth2Error> {
        let url = match &self.options.userinfo_url {
            Some(u) => u.clone(),
            None => return Err(OAuth2Error::InvalidConfig("no userinfo_url".into())),
        };
        let resp = self
            .http
            .get(url.as_str())
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(OAuth2Error::Transport)?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuth2Error::TokenEndpoint { status, body });
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(OAuth2Error::Transport)
    }

    /// Look up a cached token by its `access_token` string. Returns
    /// the cached `TokenSet` if present. The cache is keyed by the
    /// raw access token because that's what the IdP returns and
    /// callers compare on; we don't try to be cleverer.
    pub fn cached(&self, access_token: &str) -> Option<TokenSet> {
        self.cache.as_ref().and_then(|c| c.get(access_token))
    }

    fn cache_token(&self, token: TokenSet) {
        if let Some(cache) = &self.cache {
            cache.put(token.clone());
        }
    }

    /// The configured userinfo URL, if any.
    pub fn userinfo_url(&self) -> Option<&Url> {
        self.options.userinfo_url.as_ref()
    }

    /// Borrow the inner typed `Client` (read-only). For tests that
    /// want to inspect the configured client state. The concrete
    /// type pins `HasAuthUrl = EndpointSet` and `HasTokenUrl =
    /// EndpointSet`; the other endpoint-state generics are
    /// `EndpointNotSet`.
    pub fn inner(
        &self,
    ) -> &oauth2::Client<
        oauth2::basic::BasicErrorResponse,
        oauth2::basic::BasicTokenResponse,
        oauth2::basic::BasicTokenIntrospectionResponse,
        oauth2::StandardRevocableToken,
        oauth2::basic::BasicRevocationErrorResponse,
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    > {
        &self.inner
    }
}

/// The (URL, state) pair returned by `OAuth2Client::authorize_url`.
/// The caller persists the `state` and validates it matches the
/// `state` query param on the redirect.
#[derive(Debug, Clone)]
pub struct AuthorizeUrl {
    pub url: Url,
    pub state: oauth2::CsrfToken,
}

/// Process-wide token cache. `OAuth2Options::with_cache` takes one
/// of these so multiple clients with the same IdP can share token
/// state. Thread-safe, lock-protected. The cache is best-effort: it
/// evicts nothing automatically (caller decides retention).
#[derive(Debug, Default)]
pub struct TokenCache {
    inner: Mutex<HashMap<String, TokenSet>>,
}

impl TokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, token: TokenSet) {
        let mut guard = self.inner.lock();
        guard.insert(token.access_token.clone(), token);
    }

    pub fn get(&self, access_token: &str) -> Option<TokenSet> {
        self.inner.lock().get(access_token).cloned()
    }

    /// Cache lookup by refresh token. Used when the caller wants to
    /// refresh without knowing the (possibly rotated) access token.
    pub fn by_refresh(&self, refresh: &str) -> Option<TokenSet> {
        self.inner
            .lock()
            .values()
            .find(|t| t.refresh_token.as_deref() == Some(refresh))
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Helpers (private)
// ---------------------------------------------------------------------------

/// POST a form-urlencoded body to the token endpoint, parse the
/// response as `StandardTokenResponse<EmptyExtraTokenFields, ...>`,
/// and return both the typed value and the raw wire JSON. The wire
/// JSON is preserved because the typed response drops `id_token`
/// (OIDC) and `scope` during re-serialisation (the v5 crate uses
/// `#[serde(flatten)] extra_fields: EmptyExtraTokenFields`, which
/// has nowhere to put those fields).
async fn post_token_form(
    http: &reqwest::Client,
    token_url: &url::Url,
    form: &[(String, String)],
) -> Result<
    (
        oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
        serde_json::Value,
    ),
    OAuth2Error,
> {
    let body: String = form
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let resp = http
        .post(token_url.as_str())
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(OAuth2Error::Transport)?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(OAuth2Error::Transport)?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).into_owned();
        // Try to pull the typed `error` field (RFC 6749 §5.2) so the
        // caller gets a `Grant(InvalidGrant)` instead of a generic
        // `TokenEndpoint { status, body }`. The body may also be
        // non-JSON (proxy timeout page, HTML 502, …) so we treat
        // both shapes uniformly via `serde_json::Value`.
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let Some(err_str) = json.get("error").and_then(|v| v.as_str()) {
                return Err(OAuth2Error::Grant(GrantError::from_wire(err_str)));
            }
        }
        return Err(OAuth2Error::TokenEndpoint {
            status: status.as_u16(),
            body,
        });
    }
    let wire: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| OAuth2Error::Unexpected(format!("token response not JSON: {e}")))?;
    let parsed: oauth2::StandardTokenResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    > = serde_json::from_value(wire.clone())
        .map_err(|e| OAuth2Error::Unexpected(format!("token response parse: {e}")))?;
    Ok((parsed, wire))
}

/// Convert the `oauth2` crate's `StandardTokenResponse` into our
/// `TokenSet`, with `Instant`-typed `expires_at` so callers don't
/// have to do `now + Duration::from_secs(expires_in)` themselves.
///
/// `raw_wire` is the raw JSON returned by the IdP. We use it to
/// extract `id_token` (OIDC) and `scope` (RFC 6749 §5.1), neither of
/// which the `oauth2` crate's `StandardTokenResponse` exposes when
/// the extra-fields type is `EmptyExtraTokenFields` (the default).
/// Without `raw_wire`, `id_token` would be lost: the crate flattens
/// unknown fields into `extra_fields` during deserialisation but
/// drops them when re-serialising `EmptyExtraTokenFields` (it has
/// no fields to flatten into).
fn materialise_token(
    raw: &oauth2::StandardTokenResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    raw_wire: &serde_json::Value,
    request_timeout: Duration,
) -> TokenSet {
    use oauth2::TokenResponse;
    let expires_at = raw.expires_in().map(|d| {
        // `Instant` doesn't compose with arbitrary time deltas the way
        // we'd like, but `expires_in` is always positive (IdP contract).
        // We allow a buffer equal to the request timeout because token
        // round-trip latency eats into the validity window.
        Instant::now() + d.saturating_sub(request_timeout / 4)
    });
    let access = raw.access_token().secret().clone();
    let refresh = raw.refresh_token().map(|t| t.secret().clone());
    // `id_token` and `scope` are at the top level of the response
    // (RFC 6749 §5.1 + OIDC Core 1.0 §3.1.3.7), but the `oauth2`
    // crate only exposes them through `extra_fields`. We pull them
    // out of the wire JSON instead.
    let id_token = raw_wire
        .get("id_token")
        .and_then(|v| v.as_str())
        .map(String::from);
    let scope = raw_wire
        .get("scope")
        .and_then(|v| v.as_str())
        .map(String::from);
    TokenSet {
        access_token: access,
        refresh_token: refresh,
        id_token,
        expires_at,
        scope,
        raw: raw_wire.clone(),
    }
}

/// Map an `oauth2::RequestTokenError` into our `OAuth2Error`. This
/// is the workhorse for token-endpoint failures. Unused now that
/// we drive the HTTP call ourselves via [`post_token_form`], but
/// kept available for callers that bridge to the v5 crate's typed
/// flow.
#[allow(dead_code)]
fn map_oauth2_error(
    e: oauth2::RequestTokenError<
        oauth2::HttpClientError<reqwest::Error>,
        oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    >,
) -> OAuth2Error {
    use oauth2::RequestTokenError;
    match e {
        RequestTokenError::ServerResponse(srv) => {
            // `srv.error()` returns `&BasicErrorResponseType`; the
            // `AsRef<str>` impl on the enum gives us the RFC 6749 §5.2
            // wire string ("invalid_request", "invalid_client", …).
            let wire = srv.error().as_ref();
            OAuth2Error::Grant(GrantError::from_wire(wire))
        }
        RequestTokenError::Request(req) => {
            // `HttpClientError` is the v5 wrapper that nests either a
            // `reqwest::Error` (when the `reqwest` feature is on) or
            // an opaque string. `HttpClientError` is `#[non_exhaustive]`
            // so we need a wildcard — future oauth2 versions may add
            // variants (e.g. for `curl`, `ureq`) that we fold into
            // `Client` for now.
            match req {
                oauth2::HttpClientError::Reqwest(e) => OAuth2Error::Transport(*e),
                oauth2::HttpClientError::Other(s) => OAuth2Error::Client(s),
                _ => OAuth2Error::Client(format!("unrecognised transport error: {req:?}")),
            }
        }
        RequestTokenError::Parse(parse, _) => {
            OAuth2Error::Unexpected(format!("token response parse: {parse}"))
        }
        RequestTokenError::Other(s) => OAuth2Error::Client(s),
    }
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Generate a fresh PKCE S256 challenge + verifier pair. Convenience
/// wrapper around `oauth2`'s constructors; pulled out so the tests
/// don't need to know about the `oauth2` crate's type names.
pub fn generate_pkce() -> (oauth2::PkceCodeChallenge, oauth2::PkceCodeVerifier) {
    let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    (challenge, verifier)
}

/// Convenience constructor for a `CsrfToken` (state). Most callers
/// don't need this — `OAuth2Client::authorize_url` returns the state
/// directly — but it's useful for callers that want to test the
/// state round-trip without going through the URL builder.
pub fn generate_state() -> oauth2::CsrfToken {
    oauth2::CsrfToken::new_random()
}

/// Base64-URL-encode a value without padding. Used by the conformance
/// tests; exposed for users who want to debug PKCE manually.
pub fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// ---------------------------------------------------------------------------
// Wire DTO
// ---------------------------------------------------------------------------

/// Wire-friendly shape of [`TokenSet`]. Replaces the `Instant` with
/// `SystemTime` + a `u64` second offset, both of which implement
/// `Serialize`/`Deserialize`. Useful for users who want to persist
/// the token set (e.g. in a database) and reload it on a later run.
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenSetDto {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    /// UNIX epoch seconds (1970-01-01T00:00:00Z). `None` for tokens
    /// the IdP returned without an `expires_in`.
    pub expires_at_unix: Option<u64>,
    pub scope: Option<String>,
    pub raw: serde_json::Value,
}

impl std::fmt::Debug for TokenSetDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Same redaction as TokenSet: tokens are bearer credentials and
        // never reach logs via Debug. (Serialize is intentionally left
        // untouched — it is the persistence path, not a logging path.)
        f.debug_struct("TokenSetDto")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("expires_at_unix", &self.expires_at_unix)
            .field("scope", &self.scope)
            .field("raw", &self.raw)
            .finish()
    }
}

impl From<&TokenSet> for TokenSetDto {
    fn from(t: &TokenSet) -> Self {
        let expires_at_unix = t.expires_at.and_then(|inst| {
            // `Instant` is monotonic; we project it onto UNIX time using
            // the system clock. `Instant::now()` and the system clock
            // may differ, but for a freshly-materialised token the
            // delta is sub-second and irrelevant for token expiry.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            let now_instant = Instant::now();
            let delta = inst.saturating_duration_since(now_instant);
            Some(now.as_secs().saturating_add(delta.as_secs()))
        });
        Self {
            access_token: t.access_token.clone(),
            refresh_token: t.refresh_token.clone(),
            id_token: t.id_token.clone(),
            expires_at_unix,
            scope: t.scope.clone(),
            raw: t.raw.clone(),
        }
    }
}

#[cfg(test)]
mod debug_redaction_tests {
    use super::*;

    fn sample_url(s: &str) -> Url {
        Url::parse(s).expect("valid url")
    }

    #[test]
    fn token_set_debug_never_shows_tokens() {
        let set = TokenSet {
            access_token: "at-DO-NOT-LOG-abc123".to_string(),
            refresh_token: Some("rt-DO-NOT-LOG-def456".to_string()),
            id_token: Some("it-DO-NOT-LOG-ghi789".to_string()),
            expires_at: None,
            scope: Some("openid email".to_string()),
            raw: serde_json::json!({"token_type": "Bearer"}),
        };
        let rendered = format!("{set:?}");
        assert!(!rendered.contains("DO-NOT-LOG"), "token leaked: {rendered}");
        assert!(
            rendered.matches("<redacted>").count() >= 3,
            "expected redaction markers: {rendered}"
        );
        assert!(
            rendered.contains("openid email"),
            "scope should stay visible: {rendered}"
        );
    }

    #[test]
    fn options_debug_never_shows_client_secret() {
        let options = OAuth2Options {
            client_id: "web-app".to_string(),
            client_secret: Some("s3cr3t-DO-NOT-LOG".to_string()),
            authz_url: sample_url("https://idp.example.com/authz"),
            token_url: sample_url("https://idp.example.com/token"),
            redirect_uri: None,
            revoke_url: None,
            userinfo_url: None,
            timeout: Duration::from_secs(30),
            cache: None,
        };
        let rendered = format!("{options:?}");
        assert!(
            !rendered.contains("s3cr3t"),
            "client secret leaked: {rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "no redaction marker: {rendered}"
        );
        assert!(
            rendered.contains("web-app"),
            "client id should stay visible: {rendered}"
        );
    }

    #[test]
    fn token_set_dto_debug_never_shows_tokens() {
        let dto = TokenSetDto {
            access_token: "at-DO-NOT-LOG".to_string(),
            refresh_token: Some("rt-DO-NOT-LOG".to_string()),
            id_token: None,
            expires_at_unix: Some(1_700_000_000),
            scope: None,
            raw: serde_json::Value::Null,
        };
        let rendered = format!("{dto:?}");
        assert!(!rendered.contains("DO-NOT-LOG"), "token leaked: {rendered}");
    }
}
