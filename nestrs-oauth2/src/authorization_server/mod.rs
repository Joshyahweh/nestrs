//! nestrs **as an OAuth2 authorization server** — issue tokens, don't
//! just verify them.
//!
//! The rest of this crate is the *client* side (talk to an IdP) and the
//! *resource server* side (verify an IdP's JWTs). This module is the
//! *third* role (RFC 6749): the IdP itself, for apps that want nestrs
//! to be their identity provider — first-party auth without an external
//! Auth0/Keycloak.
//!
//! # Surface
//!
//! | endpoint | protocol |
//! |---|---|
//! | `GET {prefix}/authorize` | RFC 6749 §4.1.1 + PKCE (RFC 7636, **S256 only**) |
//! | `POST {prefix}/token` | grants: `authorization_code`, `refresh_token` (rotating), `client_credentials` |
//! | `POST {prefix}/introspect` | RFC 7662 |
//! | `POST {prefix}/revoke` | RFC 7009 |
//! | `GET /.well-known/oauth-authorization-server` | RFC 8414 discovery |
//! | `GET /.well-known/jwks.json` | Ed25519 `OKP` JWK |
//!
//! Access tokens are **Ed25519-signed JWTs** (`EdDSA`) with the
//! standard claims (`iss`, `sub`, `aud`, `exp`, `iat`, `jti`) plus
//! `scope` and `client_id` — verifiable by any compliant resource
//! server, including this crate's own [`JwksCache`]/[`JwtVerifier`](crate::JwtVerifier).
//!
//! # Security model
//!
//! - **PKCE S256 is required** for public clients, and for confidential
//!   clients too by default (OAuth 2.1 posture;
//!   [`AuthorizationServerConfig::require_pkce_for_confidential`] can
//!   relax it for legacy confidential clients). `plain` is not
//!   supported.
//! - **Authorization codes**: single-use, 32 random bytes, stored
//!   SHA-256-hashed, exact `redirect_uri` + client binding. **Replay of
//!   a code revokes the refresh-token family it seeded.** Codes are
//!   consumed before validation, so a failed redemption cannot be
//!   probed repeatedly.
//! - **Refresh tokens rotate** (each use consumes and reissues). A
//!   **reused** (rotated/revoked) token is theft with high probability
//!   → the **entire family dies**: every refresh token in the chain
//!   *and* their outstanding access JWTs.
//! - **Client secrets are stored hashed** (SHA-256) and compared
//!   constant-time; PKCE verifiers/challenges likewise.
//! - **Open-redirect safe**: `/authorize` only redirects to a
//!   byte-for-byte registered `redirect_uri`, and never redirects at
//!   all until client + redirect URI are validated.
//! - **No secrets in logs**: `Debug` on every token/secret-carrying
//!   type is redacted, and errors never echo presented credentials.
//!
//! # State & the stores
//!
//! All server state lives behind four small async traits
//! ([`stores`]) — clients, codes, refresh tokens, access-token
//! revocation. In-memory implementations ship for dev/tests/small
//! deployments ([`AuthorizationServerStores::in_memory`]); back them
//! with Postgres/Redis for production scale. Codes and refresh tokens
//! are always handled **hashed** — a store compromise yields
//! unredeemable digests.
//!
//! # Mounting
//!
//! Via the nestrs DI registry (composes into any `NestFactory` app):
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! use nestrs_oauth2::authorization_server::*;
//! use nestrs_oauth2::authorization_server::model::*;
//!
//! let config = AuthorizationServerConfig::new(
//!     "https://idp.example.com",
//!     include_str!("../../tests/data/signing_key.pem"), // your Ed25519 PKCS#8 PEM
//!     "2026-09",
//! )?;
//!
//! let web_app = OAuth2ClientRecord::confidential(
//!     "web-app",
//!     "5f4dcc3b5aa765d61d8327deb882cf99", // generate with a CSPRNG
//!     vec!["https://app.example.com/callback".into()],
//!     vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
//!     vec!["read".into(), "write".into()],
//! )?;
//!
//! let module = OAuth2AuthorizationServerModule::register(
//!     config,
//!     AuthorizationServerStores::in_memory(vec![web_app]),
//!     // resolve the logged-in user from your session middleware:
//!     Arc::new(|parts| {
//!         parts
//!             .extensions
//!             .get::<MySession>()
//!             .map(|s| s.user_id.clone())
//!     }),
//! )?;
//! # Ok(())
//! # }
//! # struct MySession { user_id: String }
//! ```
//! (Or mount [`routes::router`] directly into an existing axum app.)
//!
//! [`JwksCache`]: crate::JwksCache

use std::any::TypeId;
use std::sync::Arc;

use nestrs_core::{DynamicModule, ProviderRegistry};

pub mod config;
pub mod model;
pub mod routes;
pub mod service;
pub mod stores;

pub use config::{AuthorizationServerConfig, AuthorizationServerConfigError};
pub use model::{
    ClientRecordError, GrantType, OAuth2ClientRecord, StoredAuthorizationCode, StoredRefreshToken,
};
pub use service::{
    AuthorizationServer, AuthorizationServerSetupError, IssuedTokens, ResourceOwnerSource,
    TokenFailure,
};
pub use stores::{
    AccessTokenRevocationList, AuthorizationServerStores, ClientStore, CodeStore, ConsumeCode,
    ConsumeRefresh, InMemoryClientStore, InMemoryCodeStore, InMemoryRefreshTokenStore,
    InMemoryRevocationList, RefreshTokenStore,
};

/// SHA-256 hex digest — the storage form of codes, refresh tokens, and
/// client secrets. Hex (not base64) to keep digests directly usable as
/// SQL text keys.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-time equality over equal-length secrets. Length is checked
/// first (all callers pass fixed-length digests, where a length
/// mismatch is a format error, not a secret leak).
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The dynamic-module entry point: builds the server, registers it in
/// the DI registry, and returns a `DynamicModule` whose router carries
/// all six endpoints (merged into the host app by `NestFactory`).
///
/// The `resource_owner` closure resolves the authenticated user for
/// `/authorize` — see [`ResourceOwnerSource`] (there is no default:
/// session shape is app-specific by nature).
pub struct OAuth2AuthorizationServerModule;

impl OAuth2AuthorizationServerModule {
    pub fn register(
        config: AuthorizationServerConfig,
        stores: AuthorizationServerStores,
        resource_owner: ResourceOwnerSource,
    ) -> Result<DynamicModule, AuthorizationServerSetupError> {
        let server = Arc::new(AuthorizationServer::new(config, stores, resource_owner)?);

        let mut registry = ProviderRegistry::default();
        registry.register_use_value::<AuthorizationServer>(server.clone());
        let exports = vec![TypeId::of::<AuthorizationServer>()];

        Ok(DynamicModule::from_parts(
            registry,
            routes::router(server),
            exports,
        ))
    }
}
