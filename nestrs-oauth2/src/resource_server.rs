//! JWKS-backed resource server. Validates JWTs issued by an IdP whose
//! public keys are served at a JWKS URL (RFC 7517).
//!
//! We hand-roll the JWKS cache because the `jsonwebtoken` ecosystem
//! expects you to own JWKS fetching + rotation. The cache is a small
//! state machine:
//!
//!   * `keys`: `ArcSwap<HashMap<kid, DecodingKey>>` — hot read path,
//!     no lock.
//!   * `jwks`: `ArcSwap<JwkSet>` — full set, used for `from_jwk`
//!     re-derivation if the parsed `DecodingKey` needs to be refreshed.
//!   * `fetched_at`: `Mutex<Instant>` — single-flight refresh window.
//!
//! Refresh is single-flight: if 10 concurrent requests miss the
//! cache, only one HTTP fetch runs; the other 9 wait on the
//! refresh and see the new keys.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::OAuth2Error;

/// JWKS-rotated cache. Clone-cheap (all internal state is `Arc`).
pub struct JwksCache {
    url: url::Url,
    http: reqwest::Client,
    refresh_window: Duration,
    keys: ArcSwap<HashMap<String, DecodingKey>>,
    jwks: ArcSwap<JwkSet>,
    fetched_at: Mutex<Option<Instant>>,
    /// Single-flight gate: if a refresh is already in progress, waiters
    /// observe the result via the same mutex instead of issuing
    /// their own HTTP fetch.
    in_flight: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for JwksCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksCache")
            .field("url", &self.url)
            .field("refresh_window", &self.refresh_window)
            .field("kids", &self.keys_snapshot())
            .finish_non_exhaustive()
    }
}

impl JwksCache {
    /// Build a new cache. Refresh window defaults to 15 minutes; the
    /// caller can override via `with_refresh_window`.
    pub fn new(url: url::Url) -> Result<Self, OAuth2Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(OAuth2Error::Transport)?;
        Ok(Self {
            url,
            http,
            refresh_window: Duration::from_secs(15 * 60),
            keys: ArcSwap::from_pointee(HashMap::new()),
            jwks: ArcSwap::from_pointee(JwkSet { keys: vec![] }),
            fetched_at: Mutex::new(None),
            in_flight: tokio::sync::Mutex::new(()),
        })
    }

    pub fn with_refresh_window(mut self, w: Duration) -> Self {
        self.refresh_window = w;
        self
    }

    /// Resolve a `kid` to a `DecodingKey`. If the key is missing,
    /// refresh the JWKS (once, coalesced) and try again. Returns
    /// `OAuth2Error::UnknownKid` if still missing.
    pub async fn key_for(&self, kid: &str) -> Result<DecodingKey, OAuth2Error> {
        // Fast path: hot read with no lock.
        if let Some(k) = self.keys.load().get(kid).cloned() {
            return Ok(k);
        }
        // Cache miss — acquire the single-flight gate, then re-check
        // (a concurrent refresh may have populated the kid while we
        // waited on the gate).
        let _gate = self.in_flight.lock().await;
        if let Some(k) = self.keys.load().get(kid).cloned() {
            return Ok(k);
        }
        // Still missing — actually fetch.
        self.fetch_jwks().await?;
        self.keys
            .load()
            .get(kid)
            .cloned()
            .ok_or_else(|| OAuth2Error::UnknownKid(kid.to_string()))
    }

    /// Force a refresh. Used by tests; the production hot path calls
    /// `key_for` which auto-refreshes on miss.
    pub async fn refresh(&self) -> Result<(), OAuth2Error> {
        // Single-flight: hold the gate while we fetch so concurrent
        // callers wait on us instead of issuing their own requests.
        let _gate = self.in_flight.lock().await;
        let now = Instant::now();
        {
            let last = *self.fetched_at.lock();
            if let Some(t) = last {
                if now.duration_since(t) < self.refresh_window && !self.keys.load().is_empty() {
                    return Ok(());
                }
            }
        }
        self.fetch_jwks().await
    }

    async fn fetch_jwks(&self) -> Result<(), OAuth2Error> {
        let now = Instant::now();
        let resp =
            self.http
                .get(self.url.as_str())
                .send()
                .await
                .map_err(|e| OAuth2Error::JwksFetch {
                    retries: 0,
                    source: e,
                })?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(OAuth2Error::Unexpected(format!(
                "JWKS fetch returned {status}"
            )));
        }
        let jwks: JwkSet = resp.json().await.map_err(|e| OAuth2Error::JwksFetch {
            retries: 0,
            source: e,
        })?;
        // Build a kid -> DecodingKey map. JWKs without a `kid` are
        // skipped (we never know which one to use).
        let mut new_keys = HashMap::new();
        for jwk in &jwks.keys {
            if let Some(kid) = jwk.common.key_id.as_ref() {
                if let Ok(dk) = DecodingKey::from_jwk(jwk) {
                    new_keys.insert(kid.clone(), dk);
                }
            }
        }
        self.keys.store(Arc::new(new_keys));
        self.jwks.store(Arc::new(jwks));
        *self.fetched_at.lock() = Some(now);
        Ok(())
    }

    /// Snapshot of the current kid -> DecodingKey map. For tests /
    /// diagnostics.
    pub fn keys_snapshot(&self) -> Vec<String> {
        self.keys.load().keys().cloned().collect()
    }
}

/// Validation configuration for `JwtVerifier`. Algorithm pinning
/// prevents the `alg=none` downgrade attack and constrains
/// cross-algorithm confusion (RS256 token replayed as HS256).
#[derive(Clone, Debug)]
pub struct ValidationConfig {
    /// Acceptable algorithms. Default: `vec![Algorithm::EdDSA]`
    /// (matches what `nestrs/src/authn.rs:429` already pins for the
    /// EdDSA-only path). Override when validating IdPs that sign
    /// with RS256 / ES256 / PS256.
    pub algorithms: Vec<Algorithm>,
    /// Expected `iss` claim. `None` to skip.
    pub issuer: Option<String>,
    /// Expected `aud` claim. `None` to skip.
    pub audience: Option<String>,
    /// Clock-skew leeway in seconds. Default: 30 (matches
    /// `nestrs/src/authn.rs:34`).
    pub leeway: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            algorithms: vec![Algorithm::EdDSA],
            issuer: None,
            audience: None,
            leeway: 30,
        }
    }
}

impl ValidationConfig {
    pub fn new(algorithm: Algorithm) -> Self {
        Self {
            algorithms: vec![algorithm],
            ..Self::default()
        }
    }

    pub fn with_algorithms(mut self, algs: Vec<Algorithm>) -> Self {
        self.algorithms = algs;
        self
    }

    pub fn with_issuer(mut self, iss: impl Into<String>) -> Self {
        self.issuer = Some(iss.into());
        self
    }

    pub fn with_audience(mut self, aud: impl Into<String>) -> Self {
        self.audience = Some(aud.into());
        self
    }

    pub fn with_leeway(mut self, secs: u64) -> Self {
        self.leeway = secs;
        self
    }
}

/// The token payload returned by `JwtVerifier::verify`. We materialise
/// claims as a generic `serde_json::Value` (rather than a typed
/// struct) because each IdP's claim shape is different. Callers
/// extract what they need.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub header: serde_json::Value,
    pub claims: serde_json::Value,
}

/// The verifier: take a JWT, look up the kid in the JWKS, validate
/// claims, return the parsed payload.
#[derive(Clone)]
pub struct JwtVerifier {
    jwks: Arc<JwksCache>,
    validation: ValidationConfig,
}

impl JwtVerifier {
    pub fn new(jwks: Arc<JwksCache>, validation: ValidationConfig) -> Self {
        Self { jwks, validation }
    }

    /// Build a verifier from a JWKS URL. Convenience constructor
    /// that creates the cache and verifier in one call.
    pub async fn from_url(
        jwks_url: url::Url,
        validation: ValidationConfig,
    ) -> Result<Self, OAuth2Error> {
        let cache = Arc::new(JwksCache::new(jwks_url)?);
        cache.refresh().await?;
        Ok(Self::new(cache, validation))
    }

    pub fn jwks(&self) -> &JwksCache {
        &self.jwks
    }

    pub fn validation(&self) -> &ValidationConfig {
        &self.validation
    }

    /// Verify a token. Returns the parsed claims on success.
    pub async fn verify(&self, token: &str) -> Result<TokenData, OAuth2Error> {
        // First decode the header to get the `kid` (and `alg`).
        let header = decode_header(token)
            .map_err(|e| OAuth2Error::InvalidSignature(format!("header decode: {e}")))?;
        let kid = header
            .kid
            .clone()
            .ok_or_else(|| OAuth2Error::InvalidConfig("token has no kid".into()))?;
        // Algorithm pinning — must be in our accepted set BEFORE we
        // look up the key. This is the alg-confusion guard.
        if !self.validation.algorithms.contains(&header.alg) {
            return Err(OAuth2Error::Validation(format!(
                "alg {:?} not in accepted set",
                header.alg
            )));
        }
        let key = self.jwks.key_for(&kid).await?;
        let mut validation = Validation::new(header.alg);
        validation.leeway = self.validation.leeway;
        // `jsonwebtoken` defaults `validate_nbf = false`; flip it on
        // so an `nbf` claim is enforced (matches RFC 7519 §4.1.5).
        validation.validate_nbf = true;
        if let Some(iss) = &self.validation.issuer {
            validation.set_issuer(&[iss.as_str()]);
        }
        if let Some(aud) = &self.validation.audience {
            validation.set_audience(&[aud.as_str()]);
        } else {
            // `jsonwebtoken` defaults to validating `aud` if present;
            // explicit opt-out keeps the verifier consistent with
            // `nestrs/src/authn.rs:440`.
            validation.validate_aud = false;
        }
        let data = decode::<serde_json::Value>(token, &key, &validation)
            .map_err(|e| OAuth2Error::Validation(e.to_string()))?;
        Ok(TokenData {
            header: serde_json::to_value(&header).unwrap_or(serde_json::Value::Null),
            claims: data.claims,
        })
    }
}
