//! First-class authentication: `JwtService` (EdDSA via `jsonwebtoken`), `Argon2idPasswordHasher`
//! (via `argon2`), the `Principal` / `OptionalPrincipal` request extractors, the `AuthnGuard`
//! (which combines JWT verification with `#[roles(...)]` route metadata), and the
//! `AuthnModule::register(AuthnOptions)` entry point.
//!
//! All symbols are gated behind the `authn` Cargo feature.

use crate::core::{
    AuthError, AuthStrategy, CanActivate, DynamicModule, GuardError, Injectable, ProviderRegistry,
};
use crate::module;
use crate::security::{parse_authorization_bearer, route_roles_csv};
use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use std::any::TypeId;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// AuthnOptions
// ---------------------------------------------------------------------------

/// Configuration for [`AuthnModule::register`].
#[derive(Clone, Debug)]
pub struct AuthnOptions {
    /// Expected `iss` (issuer) claim; `None` to skip the check.
    pub issuer: Option<String>,
    /// Expected `aud` (audience) claim; `None` to skip the check.
    pub audience: Option<String>,
    /// Ed25519 public key in **PEM SPKI** form (matches `EncodingKey::from_ed_pem`).
    pub public_key_pem: String,
    /// Clock-skew leeway applied to `exp` / `nbf` checks (seconds). Default: `30`.
    pub leeway_seconds: u64,
    /// Parameters used by [`Argon2idPasswordHasher`].
    pub argon2id_params: Argon2idParams,
}

impl Default for AuthnOptions {
    fn default() -> Self {
        Self {
            issuer: None,
            audience: None,
            public_key_pem: String::new(),
            leeway_seconds: 30,
            argon2id_params: Argon2idParams::default(),
        }
    }
}

impl AuthnOptions {
    pub fn new(public_key_pem: impl Into<String>) -> Self {
        Self {
            public_key_pem: public_key_pem.into(),
            ..Self::default()
        }
    }

    pub fn with_issuer(mut self, iss: impl Into<String>) -> Self {
        self.issuer = Some(iss.into());
        self
    }

    pub fn with_audience(mut self, aud: impl Into<String>) -> Self {
        self.audience = Some(aud.into());
        self
    }

    pub fn with_leeway_seconds(mut self, secs: u64) -> Self {
        self.leeway_seconds = secs;
        self
    }

    pub fn with_argon2id_params(mut self, params: Argon2idParams) -> Self {
        self.argon2id_params = params;
        self
    }
}

// ---------------------------------------------------------------------------
// Argon2id hasher
// ---------------------------------------------------------------------------

/// Argon2id cost parameters. Defaults match OWASP minimums for interactive use
/// (m=19456 KiB, t=2, p=1).
#[derive(Clone, Copy, Debug)]
pub struct Argon2idParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for Argon2idParams {
    fn default() -> Self {
        Self {
            memory_kib: 19456,
            iterations: 2,
            parallelism: 1,
        }
    }
}

/// Password hashing abstraction. Implementors may swap in a different KDF (scrypt, bcrypt).
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    async fn hash(&self, plain: &str) -> Result<String, AuthError>;
    async fn verify(&self, plain: &str, encoded: &str) -> Result<bool, AuthError>;
}

/// Argon2id implementation. Emits PHC strings (`$argon2id$v=19$m=…,t=…,p=…$salt$hash`).
pub struct Argon2idPasswordHasher {
    params: Argon2idParams,
}

impl Argon2idPasswordHasher {
    /// Construct a hasher with the given cost parameters.
    pub fn new(params: Argon2idParams) -> Self {
        Self { params }
    }
}

#[async_trait]
impl PasswordHasher for Argon2idPasswordHasher {
    async fn hash(&self, plain: &str) -> Result<String, AuthError> {
        use argon2::password_hash::{PasswordHasher as _, SaltString};
        let argon = argon2::Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(
                self.params.memory_kib,
                self.params.iterations,
                self.params.parallelism,
                None,
            )
            .map_err(|e| AuthError::unauthorized(format!("argon2 params: {e}")))?,
        );
        // Generate 16 random bytes for the salt using getrandom (avoids the
        // `rand_core`/`getrandom` feature tangle from `argon2::password_hash::rand_core`).
        let mut salt_bytes = [0u8; 16];
        getrandom::getrandom(&mut salt_bytes)
            .map_err(|e| AuthError::unauthorized(format!("salt rng: {e}")))?;
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|e| AuthError::unauthorized(format!("salt encode: {e}")))?;
        argon
            .hash_password(plain.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AuthError::unauthorized(format!("argon2 hash: {e}")))
    }

    async fn verify(&self, plain: &str, encoded: &str) -> Result<bool, AuthError> {
        use argon2::password_hash::{PasswordHash, PasswordVerifier as _};
        let parsed = PasswordHash::new(encoded)
            .map_err(|e| AuthError::unauthorized(format!("malformed encoded hash: {e}")))?;
        let argon = argon2::Argon2::default();
        Ok(argon.verify_password(plain.as_bytes(), &parsed).is_ok())
    }
}

// ---------------------------------------------------------------------------
// PrincipalIdentity + JwtService
// ---------------------------------------------------------------------------

/// Caller identity extracted from a verified JWT. The `roles` field is the union
/// of (a) a top-level `roles` array claim and (b) a `"role"` or `"roles"` nested
/// claim under `realm_access` (Keycloak-style). Other shapes can be supported by
/// post-processing the raw `claims` JSON.
#[derive(Clone, Debug)]
pub struct PrincipalIdentity {
    pub subject: String,
    pub roles: Vec<String>,
    pub claims: serde_json::Value,
}

/// EdDSA JWT verifier. Constructed by [`AuthnModule::register`]; the configured
/// instance is what [`AuthnGuard`] pulls from the DI registry.
pub struct JwtService {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtService {
    /// Verifies the `Authorization: Bearer <token>` header on `parts` and returns
    /// the decoded [`PrincipalIdentity`]. Returns [`AuthError::unauthorized`] on
    /// any failure (missing header, bad scheme, signature mismatch, expired, etc).
    pub fn verify(&self, parts: &Parts) -> Result<PrincipalIdentity, AuthError> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AuthError::unauthorized("missing Authorization header"))?;
        let token = parse_authorization_bearer(header)
            .ok_or_else(|| AuthError::unauthorized("expected Bearer token"))?;

        let data = decode::<serde_json::Value>(token, &self.decoding_key, &self.validation)
            .map_err(|e| AuthError::unauthorized(format!("jwt: {e}")))?;

        let subject = data
            .claims
            .get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut roles: Vec<String> = Vec::new();
        if let Some(arr) = data.claims.get("roles").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    roles.push(s.to_string());
                }
            }
        }
        if let Some(ra) = data.claims.get("realm_access").and_then(|v| v.as_object()) {
            for k in ["roles", "role"] {
                if let Some(arr) = ra.get(k).and_then(|v| v.as_array()) {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            roles.push(s.to_string());
                        }
                    }
                }
            }
        }

        Ok(PrincipalIdentity {
            subject,
            roles,
            claims: data.claims,
        })
    }
}

/// Helper: time-since-epoch in seconds. Exposed so tests can mint exp/nbf values
/// without depending on a clock-injection library.
#[allow(dead_code)] // Public API; used by downstream tests.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// JwtStrategy (implements the generic AuthStrategy trait)
// ---------------------------------------------------------------------------

/// Stateless `AuthStrategy` that uses a [`JwtService`] resolved from the registry.
/// Implements `Default` because [`AuthStrategyGuard`] requires it; in practice
/// [`AuthnGuard`] is preferred (it pulls the configured service from DI).
#[allow(dead_code)] // Public API; paired with the `AuthStrategy` trait impl below.
#[derive(Debug, Default)]
pub struct JwtStrategy;

#[async_trait]
impl AuthStrategy for JwtStrategy {
    type Payload = PrincipalIdentity;

    async fn validate(&self, _parts: &Parts) -> Result<Self::Payload, AuthError> {
        Err(AuthError::unauthorized(
            "JwtStrategy::default() has no configured JwtService; use AuthnGuard instead",
        ))
    }
}

// ---------------------------------------------------------------------------
// AuthnGuard
// ---------------------------------------------------------------------------

/// Production guard: checks that a [`PrincipalIdentity`] is present in
/// `parts.extensions` (set by [`install_authn_middleware`]) and, when the
/// route has `#[roles(...)]`, enforces that the principal's roles intersect
/// the allowed set.
///
/// `CanActivate::can_activate` receives `&Parts` (immutable) so the JWT must
/// have been verified already by [`install_authn_middleware`]. This split
/// keeps guard logic cheap and side-effect free.
#[derive(Debug, Default)]
pub struct AuthnGuard;

#[async_trait]
impl CanActivate for AuthnGuard {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        // The install middleware must have run and stashed a verified principal.
        let principal: PrincipalIdentity = parts
            .extensions
            .get::<PrincipalIdentity>()
            .cloned()
            .ok_or_else(|| {
                GuardError::unauthorized(
                    "AuthnGuard used without install_authn_middleware — no PrincipalIdentity in extensions",
                )
            })?;

        if let Some(allowed_csv) = route_roles_csv(parts) {
            let allowed: Vec<String> = allowed_csv
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let has_any = principal
                .roles
                .iter()
                .any(|r: &String| allowed.iter().any(|a| a == r));
            if !has_any {
                return Err(GuardError::forbidden(
                    "principal does not satisfy required roles",
                ));
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Principal / OptionalPrincipal extractors
// ---------------------------------------------------------------------------

/// Extractor that fails with 401 when no `AuthnGuard` ran on this request
/// (or ran but rejected). Use in handlers you want to require a verified principal.
#[derive(Clone, Debug)]
pub struct Principal(pub PrincipalIdentity);

#[async_trait]
impl<S> FromRequestParts<S> for Principal
where
    S: Send + Sync,
{
    type Rejection = crate::HttpException;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<PrincipalIdentity>()
            .cloned()
            .map(Principal)
            .ok_or_else(|| {
                crate::UnauthorizedException::new("no principal on request (AuthnGuard missing?)")
            })
    }
}

/// Same as [`Principal`] but yields `None` when no principal is present. Use on
/// routes that may run with or without auth.
#[derive(Clone, Debug, Default)]
pub struct OptionalPrincipal(pub Option<PrincipalIdentity>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalPrincipal
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalPrincipal(
            parts.extensions.get::<PrincipalIdentity>().cloned(),
        ))
    }
}

// ---------------------------------------------------------------------------
// AuthnModule
// ---------------------------------------------------------------------------

#[module(providers = [JwtService, Argon2idPasswordHasher], exports = [JwtService, Argon2idPasswordHasher])]
pub struct AuthnModule;

impl AuthnModule {
    /// Build a `DynamicModule` that registers a configured `JwtService` and a
    /// default `Argon2idPasswordHasher`. The `JwtService` is the **single
    /// configured instance** — `AuthnGuard` will use it via the request
    /// extensions that `install_authn_middleware` puts there.
    pub fn register(options: AuthnOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();

        let jwt_svc = build_jwt_service(&options);
        registry.override_provider::<JwtService>(jwt_svc);
        registry.override_provider::<Argon2idPasswordHasher>(Arc::new(Argon2idPasswordHasher {
            params: options.argon2id_params,
        }));

        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![
                TypeId::of::<JwtService>(),
                TypeId::of::<Argon2idPasswordHasher>(),
            ],
        )
    }
}

/// Axum middleware that verifies the `Authorization: Bearer …` header and
/// stashes the resulting [`PrincipalIdentity`] into the request extensions.
/// Pair with `AuthnGuard` on protected routes (the guard reads the stashed
/// principal and enforces `#[roles(...)]` metadata).
///
/// The `AuthnModule::register` builder provides the [`JwtService`] instance —
/// pull it from the DI registry and pass as `State<Arc<JwtService>>`.
///
/// When the request has **no** `Authorization` header (e.g. an unauthenticated
/// probe), the middleware leaves `PrincipalIdentity` absent so that
/// `AuthnGuard` (which is required) rejects with 401 — and the
/// `OptionalPrincipal` extractor (which is allowed) yields `None`.
pub async fn install_authn_middleware(
    axum::extract::State(jwt): axum::extract::State<Arc<JwtService>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut parts, body) = req.into_parts();
    let verified = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_authorization_bearer)
        .and_then(|_| jwt.verify(&parts).ok());
    if let Some(identity) = verified {
        parts.extensions.insert(identity);
    }
    let req = axum::extract::Request::from_parts(parts, body);
    // Under `authz`, also install the verified principal into the per-task
    // slot so row-level predicates (Repository authorized paths, CrudService
    // under `authz-row-level`) can read it — mirrors the extensions entry.
    #[cfg(feature = "authz")]
    match req
        .extensions()
        .get::<PrincipalIdentity>()
        .cloned()
        .map(crate::policies::Principal::from)
    {
        Some(p) => {
            return crate::core::with_principal_erased(
                std::sync::Arc::new(p) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
                next.run(req),
            )
            .await
        }
        None => return next.run(req).await,
    }
    #[cfg(not(feature = "authz"))]
    next.run(req).await
}

/// Internal: build a `JwtService` from options. Public so `AuthnModule::install`
/// and tests can mint the same instance from a known config.
pub fn build_jwt_service(options: &AuthnOptions) -> Arc<JwtService> {
    let key = DecodingKey::from_ed_pem(options.public_key_pem.as_bytes())
        .expect("AuthnOptions.public_key_pem is not a valid Ed25519 SPKI PEM");
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.leeway = options.leeway_seconds;
    validation.validate_exp = true;
    if let Some(iss) = &options.issuer {
        validation.set_issuer(&[iss.as_str()]);
    }
    if let Some(aud) = &options.audience {
        validation.set_audience(&[aud.as_str()]);
    } else {
        // jsonwebtoken defaults to validating `aud` if present. Disable to avoid rejecting
        // tokens that simply don't carry the claim.
        validation.validate_aud = false;
    }
    Arc::new(JwtService {
        decoding_key: key,
        validation,
    })
}

// ---------------------------------------------------------------------------
// Injectable impls (mirror i18n.rs:134-138)
// ---------------------------------------------------------------------------

#[async_trait]
impl Injectable for JwtService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        // No-config default: rejects all tokens. Real configuration comes through
        // `AuthnModule::register` which `override_provider`s this entry.
        let key = DecodingKey::from_ed_pem(b"").expect("empty PEM rejected");
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_aud = false;
        Arc::new(Self {
            decoding_key: key,
            validation,
        })
    }
}

#[async_trait]
impl Injectable for Argon2idPasswordHasher {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            params: Argon2idParams::default(),
        })
    }
}
