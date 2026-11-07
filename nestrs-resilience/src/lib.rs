//! NestJS [`@nestjs/resilience`](https://docs.nestjs.com/reliability/resilience) analogue
//! for NestRS — retry (with jitter), timeout, circuit breaker, bulkhead, and fallback.
//!
//! # Quick start
//!
//! ```ignore
//! use nestrs_resilience::{
//!     PolicyBundle, ResilienceModule, ResilienceOptions, ResilienceService, RetryPolicy,
//! };
//! use std::time::Duration;
//!
//! let dm = ResilienceModule::register(ResilienceOptions::default());
//! // import `dm` from `#[module(imports = [...])]`, inject `Arc<ResilienceService>`:
//! # async fn demo(svc: std::sync::Arc<ResilienceService>) {
//! let policy = PolicyBundle {
//!     retry: Some(RetryPolicy {
//!         attempts: 3,
//!         delay: Duration::from_millis(25),
//!         factor: 2.0,
//!         max_delay: Duration::from_secs(1),
//!         jitter: true,
//!     }),
//!     ..Default::default()
//! };
//! let _ = svc.execute("label", policy, || async { Ok::<_, nestrs_resilience::ResilienceError>(1) }).await;
//! # }
//! ```
//!
//! # HTTP mapping
//!
//! Map [`ResilienceError::Timeout`] → 504 and [`ResilienceError::CircuitOpen`] → 503 +
//! `Retry-After`. Prefer stable `errorCode` values on `nestrs::HttpException`.
//!
//! # Umbrella feature
//!
//! Enable `nestrs` feature `resilience` to re-export this crate as `nestrs::resilience`.
//!
//! See also the Mintlify guide **Resilience** and lab binary `lab9_nestjs12`.

#![doc(html_root_url = "https://docs.rs/nestrs-resilience/1.7.0")]

mod circuit;
mod module;
mod options;
mod policy;
mod service;

pub use circuit::{CircuitBreaker, CircuitState};
pub use module::ResilienceModule;
pub use options::ResilienceOptions;
pub use policy::{
    BulkheadPolicy, FallbackFn, PolicyBundle, ResilienceError, ResilienceEvent, RetryPolicy,
    TimeoutPolicy,
};
pub use service::ResilienceService;

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
