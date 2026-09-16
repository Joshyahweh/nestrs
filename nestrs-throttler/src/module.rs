//! Module wiring: [`ThrottlerModule::register`] builds a [`DynamicModule`]
//! with the [`ThrottlerService`] installed in a [`ProviderRegistry`] so guards
//! can resolve it; [`ThrottlerState`] is the per-application state the global
//! middleware reads.

use crate::keys::{ThrottleKeyGenerator, ThrottleSkipper};
use crate::options::ThrottlerOptions;
use crate::service::ThrottlerService;
use crate::spec::ThrottleSpec;
use axum::Router;
use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
use std::sync::Arc;

/// Marker for [`ThrottlerModule::register`]. Carries no state — the work
/// happens at registration time, returning a [`DynamicModule`] the caller
/// mounts in its application.
pub struct ThrottlerModule;

impl ThrottlerModule {
    /// Build a [`DynamicModule`] that registers the throttler service so
    /// guards can `Injectable::construct::<ThrottlerService>(...)`.
    pub fn register(options: ThrottlerOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(ThrottlerService::from_options(&options));
        registry.register::<ThrottlerService>(service);
        DynamicModule::from_parts(registry, Router::new(), Default::default())
    }
}

/// State the global throttler middleware reads from `State<Arc<ThrottlerState>>`.
pub struct ThrottlerState {
    pub service: ThrottlerService,
    /// Fallback spec applied to routes without `#[throttle(...)]`.
    pub global: Option<ThrottleSpec>,
    pub trusted_proxy_hops: Option<u16>,
    pub key_generator: Arc<dyn ThrottleKeyGenerator>,
    pub skipper: Arc<dyn ThrottleSkipper>,
}

impl ThrottlerState {
    pub fn new(options: ThrottlerOptions) -> Self {
        let key_generator = options.resolved_key_generator();
        let skipper = options.resolved_skipper();
        Self {
            service: ThrottlerService::from_options(&options),
            global: options.global,
            trusted_proxy_hops: options.trusted_proxy_hops,
            key_generator,
            skipper,
        }
    }
}

/// Resolve the throttle spec for a route, applying precedence
/// `skip → decorated → global`.
///
/// - `skip_throttle = true` → no throttling (route opt-out).
/// - `decorated` (a `#[throttle(n, "per")]` decorator that parsed) wins over
///   the global spec.
/// - `global` is the fallback the module was constructed with (often `None`,
///   in which case only decorated routes are throttled).
pub struct ThrottleSpecFor;

impl ThrottleSpecFor {
    pub fn resolve(
        skip_throttle: bool,
        decorated: Option<ThrottleSpec>,
        global: Option<ThrottleSpec>,
    ) -> Option<ThrottleSpec> {
        if skip_throttle {
            return None;
        }
        decorated.or(global)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(limit: u64, window_secs: u64) -> ThrottleSpec {
        ThrottleSpec { limit, window_secs }
    }

    #[test]
    fn throttle_spec_for_route_precedence() {
        // skip wins over decorated and global
        assert_eq!(
            ThrottleSpecFor::resolve(true, Some(spec(5, 60)), Some(spec(100, 60))),
            None
        );
        // decorated wins over global
        assert_eq!(
            ThrottleSpecFor::resolve(false, Some(spec(5, 60)), Some(spec(100, 60))),
            Some(spec(5, 60))
        );
        // no decorator → fall back to global
        assert_eq!(
            ThrottleSpecFor::resolve(false, None, Some(spec(100, 60))),
            Some(spec(100, 60))
        );
        // neither → no throttle
        assert_eq!(ThrottleSpecFor::resolve(false, None, None), None);
    }
}
