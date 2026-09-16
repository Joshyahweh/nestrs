//! [`ThrottlerGuard`]: per-route enforcement point (Nest parity).
//!
//! Reads the `HandlerKey` extension + the per-route decorator metadata
//! (`throttle` / `skip_throttle`) and runs the configured key generator +
//! backend through [`crate::ThrottlerService`]. On `Limited`, returns
//! [`GuardError::TooManyRequests`].

use crate::keys::ThrottlerRequest;
use crate::module::ThrottleSpecFor;
use crate::service::ThrottlerService;
use crate::spec::{ThrottleOutcome, ThrottleSpec};
use async_trait::async_trait;
use axum::http::request::Parts;
use nestrs_core::client_ip::{rate_limit_key_ip_or_unknown, trusted_hops_from_parts};
use nestrs_core::{CanActivate, GuardError, HandlerKey, Injectable, MetadataRegistry, ProviderRegistry};
use std::sync::Arc;

pub struct ThrottlerGuard {
    service: Arc<ThrottlerService>,
    /// Global fallback spec (mirrors `ThrottlerModule::register`'s `global`).
    pub global: Option<ThrottleSpec>,
    pub trusted_proxy_hops: Option<u16>,
}

impl std::fmt::Debug for ThrottlerGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThrottlerGuard").finish_non_exhaustive()
    }
}

impl ThrottlerGuard {
    /// Build a guard with the service resolved from the registry and no
    /// global fallback.
    pub fn resolve(registry: &ProviderRegistry) -> Self {
        Self::with_options(registry, None, None)
    }

    /// Build a guard with the service resolved from the registry plus the
    /// module-level options the middleware path would also see.
    pub fn with_options(
        registry: &ProviderRegistry,
        global: Option<ThrottleSpec>,
        trusted_proxy_hops: Option<u16>,
    ) -> Self {
        Self {
            service: <ThrottlerService as Injectable>::construct(registry),
            global,
            trusted_proxy_hops,
        }
    }
}

#[async_trait]
impl CanActivate for ThrottlerGuard {
    async fn can_activate(&self, parts: &mut Parts) -> Result<(), GuardError> {
        let handler = parts
            .extensions
            .get::<HandlerKey>()
            .map(|h| h.0)
            .unwrap_or("");

        let decorated = MetadataRegistry::get(parts, "throttle")
            .and_then(|s| ThrottleSpec::parse(&s));
        let skip = MetadataRegistry::get(parts, "skip_throttle")
            .map(|v| v == "true")
            .unwrap_or(false);

        let Some(spec) = ThrottleSpecFor::resolve(skip, decorated, self.global) else {
            return Ok(());
        };

        let trusted_hops = trusted_hops_from_parts(parts, self.trusted_proxy_hops);
        let ip = rate_limit_key_ip_or_unknown(&parts.headers, &parts.extensions, trusted_hops);
        let tr_req = ThrottlerRequest {
            handler,
            ip: ip.clone(),
            parts,
        };

        if self.service.skipper().skip(&tr_req) {
            return Ok(());
        }
        let key = self.service.key_generator().key(&tr_req);
        match self.service.check(handler, &key, &spec).await {
            ThrottleOutcome::Allowed { .. } => Ok(()),
            ThrottleOutcome::Limited { .. } => Err(GuardError::TooManyRequests),
        }
    }
}
