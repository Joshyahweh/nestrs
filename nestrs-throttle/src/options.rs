//! [`ThrottlerOptions`]: the knobs `ThrottlerModule::register` and
//! `NestApplication::use_throttler` consume.

use crate::backend::ThrottlerBackendKind;
use crate::keys::{NeverSkip, ThrottleKeyGenerator, ThrottleSkipper};
use crate::spec::ThrottleSpec;
use std::fmt;
use std::sync::Arc;

/// Options for [`crate::ThrottlerModule::register`] / `NestApplication::use_throttler`.
///
/// There is no builder — construct the struct (or `..Default::default()`).
/// `global: None` means only routes with `#[throttle(n, "per")]` are limited.
#[derive(Clone, Default)]
pub struct ThrottlerOptions {
    /// Fallback spec applied to routes without `#[throttle(...)]`.
    /// `None` means only explicitly decorated routes are throttled.
    pub global: Option<ThrottleSpec>,
    pub backend: ThrottlerBackendKind,
    /// Forwarded-header trust level for client-IP resolution. `None` (default)
    /// inherits the application-wide hop count from
    /// `NestApplication::use_trusted_proxy_headers`, so the throttler and the
    /// `ClientIp` extractor resolve the same client identity;
    /// `Some(hops)` overrides it (forwarded headers untrusted when `Some(0)`).
    pub trusted_proxy_hops: Option<u16>,
    /// Per-request key generator. `None` defaults to [`crate::IpKeyGenerator`].
    /// Wrap your own with `Arc::new(...) as Arc<dyn ThrottleKeyGenerator>`.
    pub key_generator: Option<Arc<dyn ThrottleKeyGenerator>>,
    /// Pre-check skipper. `None` defaults to [`NeverSkip`].
    pub skipper: Option<Arc<dyn ThrottleSkipper>>,
}

/// Manual Debug impl to avoid requiring `Debug` on trait objects.
impl fmt::Debug for ThrottlerOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottlerOptions")
            .field("global", &self.global)
            .field("backend", &self.backend)
            .field("trusted_proxy_hops", &self.trusted_proxy_hops)
            .field("key_generator", &self.key_generator.is_some())
            .field("skipper", &self.skipper.is_some())
            .finish()
    }
}

impl ThrottlerOptions {
    /// Resolve the configured key generator, defaulting to [`crate::IpKeyGenerator`].
    pub fn resolved_key_generator(&self) -> Arc<dyn ThrottleKeyGenerator> {
        match &self.key_generator {
            Some(g) => g.clone(),
            None => Arc::new(IpKeyGeneratorShim),
        }
    }

    /// Resolve the configured skipper, defaulting to [`NeverSkip`].
    pub fn resolved_skipper(&self) -> Arc<dyn ThrottleSkipper> {
        match &self.skipper {
            Some(s) => s.clone(),
            None => Arc::new(NeverSkip),
        }
    }
}

// `ThrottleKeyGenerator` lives in `keys`; this tiny newtype just wraps it
// so the default branch in `resolved_key_generator` can produce a stable
// `Arc<dyn ThrottleKeyGenerator>` without leaking the trait definition.
struct IpKeyGeneratorShim;
impl ThrottleKeyGenerator for IpKeyGeneratorShim {
    fn key(&self, req: &crate::keys::ThrottlerRequest<'_>) -> String {
        req.ip.clone()
    }
}
