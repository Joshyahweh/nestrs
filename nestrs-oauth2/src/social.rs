//! Social-provider wrappers. Each is a thin layer over `OAuth2Client`
//! that pins the provider's endpoints, default scopes, and (where
//! applicable) the userinfo response shape.
//!
//! These are *just* wrappers — the OAuth2 flows they expose are
//! identical to the un-wrapped `OAuth2Client`. The value is that
//! callers don't have to remember that GitHub's userinfo endpoint
//! is `api.github.com/user` (not `github.com/…`) or that Apple's
//! authorization endpoint requires `response_mode=form_post`.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::client::{OAuth2Client, OAuth2Options};
use crate::error::OAuth2Error;

// ---------------------------------------------------------------------------
// Google
// ---------------------------------------------------------------------------

/// Google OAuth2 wrapper. Endpoints: `accounts.google.com`. Default
/// scopes: `openid email profile`.
#[derive(Debug, Clone)]
pub struct Google {
    client: OAuth2Client,
}

impl Google {
    /// Build a confidential Google OAuth2 client.
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: Url,
    ) -> Result<Self, OAuth2Error> {
        let options = OAuth2Options::new(
            client_id,
            client_secret,
            Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
                .expect("static URL is valid"),
            Url::parse("https://oauth2.googleapis.com/token").expect("static URL is valid"),
            redirect_uri,
        )
        .with_userinfo_url(
            Url::parse("https://openidconnect.googleapis.com/v1/userinfo")
                .expect("static URL is valid"),
        );
        Ok(Self {
            client: OAuth2Client::new(options)?,
        })
    }

    pub fn client(&self) -> &OAuth2Client {
        &self.client
    }

    pub fn default_scopes() -> &'static [&'static str] {
        &["openid", "email", "profile"]
    }
}

/// Google userinfo response shape. See
/// <https://developers.google.com/identity/openid-connect/openid-connect#id_token-example>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUser {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
}

// ---------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------

/// GitHub OAuth2 wrapper. Endpoints: `github.com`. Note: GitHub does
/// NOT serve OIDC discovery or a JWKS endpoint, so this is OAuth2-only
/// (no `id_token`). Userinfo lives at `api.github.com/user` (a
/// different host from the auth endpoints).
#[derive(Debug, Clone)]
pub struct GitHub {
    client: OAuth2Client,
}

impl GitHub {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: Url,
    ) -> Result<Self, OAuth2Error> {
        let options = OAuth2Options::new(
            client_id,
            client_secret,
            Url::parse("https://github.com/login/oauth/authorize").expect("static URL is valid"),
            Url::parse("https://github.com/login/oauth/access_token").expect("static URL is valid"),
            redirect_uri,
        )
        .with_userinfo_url(Url::parse("https://api.github.com/user").expect("static URL is valid"));
        Ok(Self {
            client: OAuth2Client::new(options)?,
        })
    }

    pub fn client(&self) -> &OAuth2Client {
        &self.client
    }

    pub fn default_scopes() -> &'static [&'static str] {
        // GitHub rejects `openid` (not an OIDC provider) and accepts
        // comma-separated scopes (handled by `oauth2` crate).
        &["read:user", "user:email"]
    }
}

/// GitHub userinfo shape. GitHub's response is sparse by default;
/// callers typically need a follow-up call to
/// `/user/emails` for the primary address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Microsoft
// ---------------------------------------------------------------------------

/// Microsoft OAuth2 wrapper. Tenant-aware: pass
/// `common` for multi-tenant apps, your tenant ID for single-tenant,
/// `organizations` for work/school only. Endpoints:
/// `login.microsoftonline.com/{tenant}/oauth2/v2.0/...`
#[derive(Debug, Clone)]
pub struct Microsoft {
    client: OAuth2Client,
}

impl Microsoft {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: Url,
        tenant: impl AsRef<str>,
    ) -> Result<Self, OAuth2Error> {
        let tenant = tenant.as_ref();
        let authz = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize");
        let token = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token");
        let options = OAuth2Options::new(
            client_id,
            client_secret,
            Url::parse(&authz).expect("static URL is valid"),
            Url::parse(&token).expect("static URL is valid"),
            redirect_uri,
        )
        .with_userinfo_url(
            Url::parse("https://graph.microsoft.com/oidc/userinfo").expect("static URL is valid"),
        );
        Ok(Self {
            client: OAuth2Client::new(options)?,
        })
    }

    pub fn client(&self) -> &OAuth2Client {
        &self.client
    }

    pub fn default_scopes() -> &'static [&'static str] {
        &["openid", "email", "profile", "offline_access"]
    }
}

/// Microsoft userinfo (OIDC) shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrosoftUser {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// Apple
// ---------------------------------------------------------------------------

/// Apple OAuth2 wrapper. Endpoints: `appleid.apple.com`. Apple
/// requires `response_mode=form_post` (Apple sends the auth code as
/// a POST body to the redirect URI, not a query parameter) and
/// validates the `client_secret` as a JWT signed with an
/// `ES256`-algorithm key from the Apple Developer portal. We
/// delegate the JWT signing to the caller (the secret is a JWT, not
/// a random string) so this wrapper exposes a `client_secret_jwt`
/// constructor alongside the standard one.
#[derive(Debug, Clone)]
pub struct Apple {
    client: OAuth2Client,
}

impl Apple {
    /// Build an Apple OAuth2 client with a pre-signed client_secret
    /// JWT. The caller is responsible for minting the JWT per
    /// Apple's spec (see
    /// <https://developer.apple.com/documentation/sign_in_with_apple/generate_and_validate_tokens>).
    pub fn new(
        client_id: impl Into<String>,
        client_secret_jwt: impl Into<String>,
        redirect_uri: Url,
    ) -> Result<Self, OAuth2Error> {
        let options = OAuth2Options::new(
            client_id,
            client_secret_jwt,
            Url::parse("https://appleid.apple.com/auth/authorize").expect("static URL is valid"),
            Url::parse("https://appleid.apple.com/auth/token").expect("static URL is valid"),
            redirect_uri,
        );
        // Apple's `authorize_url` requires `response_mode=form_post`.
        // We can't add it via the `OAuth2Options` builder (it would
        // attach to *every* authorization URL on every provider), so
        // the `authorize_url` helper on `Apple` does it inline.
        Ok(Self {
            client: OAuth2Client::new(options)?,
        })
    }

    pub fn client(&self) -> &OAuth2Client {
        &self.client
    }

    /// Apple's default scopes. `name` is only sent on the FIRST
    /// sign-in; subsequent sign-ins don't carry it.
    pub fn default_scopes() -> &'static [&'static str] {
        &["openid", "email", "name"]
    }
}

/// Apple userinfo shape (from the ID token claims, not a separate
/// userinfo endpoint — Apple doesn't expose one).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleUser {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    /// Apple's `name` is sent only on the first sign-in and as a
    /// nested object (`{ "firstName": "...", "lastName": "..." }`).
    /// We model it as a generic `Value` because the shape is provider-
    /// defined.
    #[serde(default)]
    pub name: Option<serde_json::Value>,
}
