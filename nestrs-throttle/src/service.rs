//! [`ThrottlerService`]: the framework-side facade that combines the configured
//! [`ThrottlerBackend`] with the resolved key generator and skipper.
//!
//! Apps don't typically construct it directly — `ThrottlerModule::register`
//! and `NestApplication::use_throttler` do. It implements [`Injectable`] so
//! guards can resolve it from a [`ProviderRegistry`].

use crate::backend::{InMemoryThrottler, ThrottlerBackend, ThrottlerBackendKind};
use crate::keys::{ThrottleKeyGenerator, ThrottleSkipper};
use crate::options::ThrottlerOptions;
use crate::spec::{ThrottleOutcome, ThrottleSpec};
use nestrs_core::{Injectable, ProviderRegistry};
use std::sync::Arc;

/// Per-application throttle facade. Cloning is cheap (three Arcs).
#[derive(Clone)]
pub struct ThrottlerService {
    backend: Arc<dyn ThrottlerBackend>,
    key_generator: Arc<dyn ThrottleKeyGenerator>,
    skipper: Arc<dyn ThrottleSkipper>,
}

impl std::fmt::Debug for ThrottlerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThrottlerService").finish_non_exhaustive()
    }
}

impl ThrottlerService {
    /// Build a service from the resolved options. The Redis variant falls
    /// back to in-memory on connection-error init (same fail-open posture as
    /// the global Redis rate limiter: throttling is an optimization, not an
    /// availability gate).
    pub fn from_options(options: &ThrottlerOptions) -> Self {
        let backend: Arc<dyn ThrottlerBackend> = match &options.backend {
            ThrottlerBackendKind::InMemory => Arc::new(InMemoryThrottler::new()),
            #[cfg(feature = "cache-redis")]
            ThrottlerBackendKind::Redis { url, key_prefix } => {
                match crate::backend::RedisThrottler::new(url, key_prefix.clone()) {
                    Ok(b) => Arc::new(b),
                    Err(e) => {
                        tracing::warn!(
                            target: "nestrs_throttle",
                            "redis throttler init failed ({e}); falling back to in-memory"
                        );
                        Arc::new(InMemoryThrottler::new())
                    }
                }
            }
            ThrottlerBackendKind::Custom(b) => b.clone(),
        };
        Self {
            backend,
            key_generator: options.resolved_key_generator(),
            skipper: options.resolved_skipper(),
        }
    }

    /// Run the throttle check against a `{handler}:{key}` scoped key.
    pub async fn check(&self, handler: &str, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome {
        let scoped = format!("{handler}:{key}");
        self.backend.check(&scoped, spec).await
    }

    pub fn key_generator(&self) -> Arc<dyn ThrottleKeyGenerator> {
        self.key_generator.clone()
    }

    pub fn skipper(&self) -> Arc<dyn ThrottleSkipper> {
        self.skipper.clone()
    }
}

impl Injectable for ThrottlerService {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        if let Some(svc) = registry.try_get::<ThrottlerService>() {
            return svc;
        }
        // Last-resort: build a default in-memory service. Lets unit tests
        // construct a guard without going through `ThrottlerModule::register`.
        Arc::new(ThrottlerService::from_options(&ThrottlerOptions::default()))
    }
}
