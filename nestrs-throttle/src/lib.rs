//! Route throttling (`#[throttle(n, "per")]` / `#[skip_throttle]`) — NestJS
//! [`@nestjs/throttler`](https://docs.nestjs.com/security/rate-limiting) parity,
//! lifted out of `nestrs` so apps that don't need it (or need it as a focused
//! dependency without the rest of the framework) can pull just this crate.
//!
//! Two enforcement points:
//!
//! - **Global middleware** ([`throttler_middleware`], installed by
//!   `nestrs::NestApplication::use_throttler`) — runs before auth guards and
//!   attaches `Retry-After` / `X-RateLimit-*` response headers on 429.
//! - **[`ThrottlerGuard`]** — a regular [`CanActivate`] guard for parity with
//!   Nest's `ThrottlerGuard` shape (rejects with [`GuardError::TooManyRequests`]).
//!
//! Per-route decorators are stored in the [`MetadataRegistry`] via the
//! standard decorator pipeline: `#[throttle(5, "minute")]` records
//! `"throttle" => "5/minute"`, `#[skip_throttle]` records
//! `"skip_throttle" => "true"`. A route with `#[throttle]` overrides the
//! global spec; `#[skip_throttle]` exempts the route entirely.
//!
//! ## Backends
//!
//! [`ThrottlerBackendKind::InMemory`] (default) is a 32-shard poison-tolerant
//! fixed-window counter. With the `cache-redis` feature,
//! `ThrottlerBackendKind::Redis` gives a cross-process counter (one Redis
//! key per scope), and [`ThrottlerBackendKind::Custom`] lets callers plug in
//! any `Arc<dyn ThrottlerBackend>` implementation (DynamoDB, Memcached, etc.).
//!
//! ## Extensibility (Phase D surface)
//!
//! - [`ThrottleKeyGenerator`] produces the per-request key. Defaults:
//!   [`IpKeyGenerator`], [`ApiKeyHeaderKeyGenerator`], [`PrincipalKeyGenerator`].
//!   The middleware/guard apply a `{handler}:{key}` scope on top.
//! - [`ThrottleSkipper`] decides whether a request is exempt *before* the
//!   key check (e.g. health probes, internal IPs). Default: [`NeverSkip`].
//! - [`ThrottlerRequest`] is the input to both traits.

#![doc(html_root_url = "https://docs.rs/nestrs-throttle/1.4.0")]

mod backend;
mod guard;
mod keys;
mod middleware;
mod module;
mod options;
mod service;
mod spec;

// Re-exports from nestrs-core so callers can write
// `nestrs_throttle::ThrottlerGuard + nestrs_throttle::CanActivate` without
// depending on nestrs-core directly. Mirrors the umbrella `nestrs::core::*`
// surface, scoped to the symbols this crate actually uses.
pub use nestrs_core::{
    CanActivate, DynamicModule, GuardError, HandlerKey, Injectable, MetadataRegistry,
    ProviderRegistry, RouteRegistry,
};

#[cfg(feature = "cache-redis")]
pub use backend::RedisThrottler;
pub use backend::{InMemoryThrottler, ThrottlerBackend, ThrottlerBackendKind};
pub use guard::ThrottlerGuard;
pub use keys::{
    ApiKeyHeaderKeyGenerator, IpKeyGenerator, NeverSkip, PrincipalId, PrincipalKeyGenerator,
    ThrottleKeyGenerator, ThrottleSkipper, ThrottlerRequest,
};
pub use middleware::throttler_middleware;
pub use module::{ThrottleSpecFor, ThrottlerModule, ThrottlerState};
pub use options::ThrottlerOptions;
pub use service::ThrottlerService;
pub use spec::{ThrottleOutcome, ThrottleSpec};
