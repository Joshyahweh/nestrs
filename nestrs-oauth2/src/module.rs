//! `OAuth2Module` — the dynamic-module entry point that wires the
//! OAuth2 client + JWKS verifier into the nestrs DI registry.
//!
//! Mirrors `AuthnModule::register` at `nestrs/src/authn.rs:373-390`:
//! build the configured services, register them in the registry via
//! `register_use_value` (the "useValue" NestJS primitive — no
//! `Injectable` impl required, since these are pre-built singletons),
//! and return a `DynamicModule` that the host application composes.

use std::any::TypeId;
use std::sync::Arc;

use nestrs_core::{DynamicModule, ProviderRegistry};

use crate::client::OAuth2Client;
use crate::resource_server::{JwksCache, JwtVerifier, ValidationConfig};

/// The OAuth2 module. Static struct; `register` is the builder.
pub struct OAuth2Module;

impl OAuth2Module {
    /// Register an OAuth2 client with the framework. Returns a
    /// `DynamicModule` exporting the `OAuth2Client` and (if a
    /// `validation` is provided) the `JwtVerifier` and `JwksCache`.
    ///
    /// Async because the JWKS cache does an initial `refresh()` on
    /// construction. Mirrors `AuthnModule::register` — eager
    /// construction, fail-fast on misconfiguration.
    pub async fn register(
        options: OAuth2ModuleOptions,
    ) -> Result<DynamicModule, crate::error::OAuth2Error> {
        let mut registry = ProviderRegistry::default();
        let client = Arc::new(OAuth2Client::new(options.client_options)?);
        registry.register_use_value::<OAuth2Client>(client.clone());
        let mut exports = vec![TypeId::of::<OAuth2Client>()];

        if let Some((jwks_url, validation)) = options.resource_server {
            let jwks = Arc::new(JwksCache::new(jwks_url)?);
            // Best-effort: if the JWKS fetch fails (offline IdP at
            // boot), we still register the cache and let the first
            // request retry. This matches the `AuthnModule::register`
            // shape — register eagerly, fail on first use, not on
            // boot.
            if let Err(e) = jwks.refresh().await {
                tracing::warn!("OAuth2 JWKS initial fetch failed: {e}");
            }
            let verifier = Arc::new(JwtVerifier::new(jwks.clone(), validation));
            registry.register_use_value::<JwtVerifier>(verifier);
            registry.register_use_value::<JwksCache>(jwks);
            exports.push(TypeId::of::<JwtVerifier>());
            exports.push(TypeId::of::<JwksCache>());
        }

        // `client` is consumed by `register_use_value`; we still hold
        // the original `Arc<OAuth2Client>` so callers that need it
        // back (e.g. for `use_oauth2` builder wiring) can call
        // `Arc::try_unwrap` later. For now we leak the `Arc` — the
        // module's lifetime is process-wide, and the OAuth2 client is
        // cheap to clone.
        let _ = client;

        Ok(DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            exports,
        ))
    }
}

/// Combined options for `OAuth2Module::register`.
#[derive(Clone, Debug)]
pub struct OAuth2ModuleOptions {
    /// The client configuration (endpoints, secrets, scopes). Always
    /// required.
    pub client_options: crate::client::OAuth2Options,
    /// Optional resource-server configuration. When `Some`, the
    /// module also exports a `JwtVerifier` and `JwksCache` for the
    /// guard / middleware to consume. When `None`, the module
    /// exports only the `OAuth2Client` (purely a client, no
    /// verification).
    pub resource_server: Option<(url::Url, ValidationConfig)>,
}
