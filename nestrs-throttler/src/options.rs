//! [`ThrottlerOptions`]: the knobs `ThrottlerModule::register` and
//! `NestApplication::use_throttler` consume.

use crate::backend::ThrottlerBackendKind;
use crate::keys::{NeverSkip, ThrottleKeyGenerator, ThrottleSkipper};
use crate::spec::ThrottleSpec;
use std::sync::Arc;

/// Options for [`crate::ThrottlerModule::register`] / `NestApplication::use_throttler`.
#[derive(Debug, Clone, Default)]
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
